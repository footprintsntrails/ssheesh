//! Starting an editor on a copy, safely.
//!
//! The copy's name comes from the server and can be hostile: `x & calc.exe`,
//! `-rf`, `run.hta`, a `.desktop` file. So an editor is always started by its
//! program's full path, with the file as one argument of its own: no shell,
//! no `cmd.exe`, no `.cmd`/`.bat` shim (on Windows those go through
//! `cmd.exe`, which reads the arguments again: BatBadBut, CVE-2024-24576),
//! and never the system's "open this file" (`ShellExecute open`, `xdg-open`),
//! which picks the program by the file's type, a type the server chose: a
//! `.hta` would run. The file's path is absolute, so it never starts with
//! `-` and can't be taken for an option.
//!
//! "The system's default editor" means the default *text* editor, which
//! `sshee-app`'s `editors` finds per system (`open -t` on macOS, the handler
//! of `.txt` on Windows, the `text/plain` desktop entry on Linux), not the
//! default app for the file's type.

use crate::error::{ErrorCode, OpError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An argument that's replaced by the file's path. Only a whole argument
/// is: `--goto={file}` stays as it is, so the path is always one argument.
/// With no such argument, the path goes last.
pub const FILE_ARG: &str = "{file}";

/// How to start an editor: a program, by its full path, and its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCommand {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

impl EditorCommand {
    pub fn new(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self { program: program.into(), args: args.iter().map(|a| a.to_string()).collect() }
    }

    /// Whether this can be started the safe way: a full path to a program
    /// that's there (on Windows an `.exe`, never a script `cmd.exe` would
    /// read).
    pub fn check(&self) -> Result<(), OpError> {
        let unsafe_editor = |why: &str| Err(OpError::new(ErrorCode::Unsupported, format!("This editor can't be started safely: {why}.")));
        if !self.program.is_absolute() {
            return unsafe_editor("SSHeesh needs the full path to its program");
        }
        if cfg!(windows) {
            let ext = self.program.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext != "exe" {
                return unsafe_editor("on Windows only an .exe program is started, never a .cmd or .bat file");
            }
        }
        match std::fs::metadata(&self.program) {
            Ok(meta) if meta.is_file() => Ok(()),
            Ok(_) => unsafe_editor("its program isn't a file"),
            Err(_) => Err(OpError::new(ErrorCode::NotFound, format!("The editor isn't at {} anymore.", self.program.display()))),
        }
    }

    /// The command that opens `file`, which must be an absolute path.
    pub fn command(&self, file: &Path) -> Result<Command, OpError> {
        self.check()?;
        if !file.is_absolute() {
            return Err(OpError::failed("The copy to open has no full path."));
        }
        let mut cmd = Command::new(&self.program);
        let mut placed = false;
        for arg in &self.args {
            if arg == FILE_ARG {
                cmd.arg(file);
                placed = true;
            } else {
                cmd.arg(arg);
            }
        }
        if !placed {
            cmd.arg(file);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        Ok(cmd)
    }

    /// Starts the editor on `file` and doesn't wait for it: editors stay
    /// open for hours, and some hand the file to a window already open and
    /// quit at once. Nobody can tell when an editor is done with a file, so
    /// nothing here tries; a session ends when you close it (see
    /// `EditBridge::close`).
    pub fn launch(&self, file: &Path) -> Result<(), OpError> {
        let mut child = self.command(file)?.spawn().map_err(|e| OpError::failed(format!("Couldn't start the editor: {e}")))?;
        // Reaped when it ends, so it doesn't linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
