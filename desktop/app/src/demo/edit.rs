//! "Open in your own editor" in `--demo`: no editor is started and nothing is copied. A file
//! "opens", its status goes through what a real session says (opened, saving, saved), and the
//! demo's contested file (`files::CONTESTED`) runs into a conflict, so every part of the
//! screen can be seen and photographed:
//!
//! - web-7731's files are saved at the demo's fixed time;
//! - web-staging's wait for a connection that doesn't come ("Offline, will retry");
//! - `~/shop/CHANGELOG.md` changed on the server meanwhile (the same line Files' own editor
//!   meets there);
//! - over 50 MB (`~/logs/access-2024-09.log`) asks first, over 1 GB is refused;
//! - with `--demo-leftovers`, two copies left from an earlier run are offered at start.
//!
//! The editors "found" are made up too.

use std::collections::HashMap;
use std::path::PathBuf;

use sshee_app::editors::Editor;
use sshee_app::files::edit::{Comparison, DiffKind, EditAsk, EditEvent, EditReply, Leftover, Opened};
use sshee_app::files::{Failure, FailureKind};
use sshee_app::paths;

use super::files::{DemoFiles, CONTESTED, NOW};

const STAGING: &str = "host:demo-staging";
const MB: u64 = 1024 * 1024;

/// Where the demo says its copies are: a made-up folder, nothing is written.
fn local(id: &str, path: &str) -> PathBuf {
    PathBuf::from("/home/dev/.sshee-preview/edit").join(id).join(paths::name(path))
}

/// The made-up editors on the demo's computer.
pub fn editors() -> Vec<Editor> {
    let e = |id: &str, name: &str, program: &str, vscode_like: bool| Editor { id: id.into(), name: name.into(), program: program.into(), args: vec!["{file}".into()], vscode_like };
    vec![
        e("vscode", "VS Code", "/usr/share/code/code", true),
        e("zed", "Zed", "/usr/bin/zed", false),
        e("gnome-text-editor", "Text Editor", "/usr/bin/gnome-text-editor", false),
        e("system", "the default text editor", "/usr/bin/gnome-text-editor", false),
    ]
}

/// The demo's sessions.
#[derive(Default)]
pub struct DemoEdit {
    /// Open ones: their server and path.
    open: HashMap<String, (String, String)>,
    next: u32,
    /// Offer two leftovers at start (`--demo-leftovers`).
    pub leftovers: bool,
}

fn event(id: &str, event: EditEvent) -> EditReply {
    EditReply::Event { id: id.into(), event }
}

impl DemoEdit {
    /// Everything `ask` is answered with, in order.
    pub fn answer(&mut self, ask: EditAsk, files: &mut DemoFiles) -> Vec<EditReply> {
        let saved = |id: &str| vec![event(id, EditEvent::Uploading), event(id, EditEvent::Uploaded { at: NOW.now as u64 })];
        match ask {
            EditAsk::Start { .. } => {
                let mut out = vec![EditReply::Editors { found: editors(), custom: None }];
                let left = if self.leftovers {
                    vec![
                        Leftover { id: "left-1".into(), host: "host:demo-web".into(), remote_path: "/home/deploy/nginx.conf".into(), local: local("left-1", "nginx.conf"), size: 178 },
                        Leftover { id: "left-2".into(), host: STAGING.into(), remote_path: "/home/deploy/README.md".into(), local: local("left-2", "README.md"), size: 66 },
                    ]
                } else {
                    Vec::new()
                };
                out.push(EditReply::Leftovers(left));
                out
            }
            EditAsk::ChooseApp { path } => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "your editor".into());
                vec![EditReply::Chosen(Ok(Editor { id: format!("custom:{}", path.display()), name, program: path, args: Vec::new(), vscode_like: false }))]
            }
            EditAsk::Open { tab, host, path, editor, confirmed } => {
                let name = editor.name.clone();
                let opened = |result| EditReply::Opened { tab, host: host.clone(), path: path.clone(), editor: name.clone(), result };
                let Some(size) = files.size_of(&host, &path) else {
                    return vec![opened(Err(Failure::new(FailureKind::NotFound, "That file doesn't exist anymore.")))];
                };
                if size > 1024 * MB {
                    let why = format!("This file is {}. SSHeesh opens files up to 1 GB in another editor; download it instead.", sshee_app::files::bytes(size));
                    return vec![opened(Err(Failure::new(FailureKind::TooLarge, why)))];
                }
                if size > 50 * MB && !confirmed {
                    return vec![opened(Ok(Opened::NeedsConfirm { size }))];
                }
                self.next += 1;
                let id = format!("demo-{}", self.next);
                let at = local(&id, &path);
                self.open.insert(id.clone(), (host.clone(), path.clone()));
                let mut out = vec![opened(Ok(Opened::Session { id: id.clone(), local: at.clone() })), event(&id, EditEvent::Opened { local: at })];
                // What happens after a save in the editor, at once.
                if path == CONTESTED {
                    out.push(event(&id, EditEvent::Uploading));
                    out.push(event(&id, EditEvent::Conflict { gone: false }));
                } else if host == STAGING {
                    out.push(event(&id, EditEvent::Uploading));
                    out.push(event(&id, EditEvent::Offline { pending: true, retry_in_ms: 15_000, message: "Can't reach the server: connection timed out".into() }));
                } else {
                    files.touch(&host, &path);
                    out.extend(saved(&id));
                }
                out
            }
            EditAsk::RetryNow { id } => {
                // The staging server stays out of reach.
                match self.open.get(&id) {
                    Some((host, _)) if host == STAGING => vec![event(&id, EditEvent::Offline { pending: true, retry_in_ms: 15_000, message: "Can't reach the server: connection timed out".into() })],
                    Some(_) => saved(&id),
                    None => Vec::new(),
                }
            }
            EditAsk::RetryHost { .. } | EditAsk::MarkOffered { .. } | EditAsk::DiscardLeftover { .. } | EditAsk::ShowFolder { .. } => Vec::new(),
            EditAsk::KeepMine { id } => {
                if let Some((host, path)) = self.open.get(&id) {
                    files.touch(host, path);
                }
                saved(&id)
            }
            EditAsk::TakeTheirs { id } => vec![event(&id, EditEvent::Reloaded { at: NOW.now as u64 })],
            EditAsk::SaveMineAsCopy { id } => {
                let Some((_, path)) = self.open.get(&id) else { return Vec::new() };
                let copy = paths::join(&paths::parent(path), "CHANGELOG (my copy).md");
                vec![EditReply::CopySaved { id: id.clone(), result: Ok(copy) }, event(&id, EditEvent::Reloaded { at: NOW.now as u64 })]
            }
            EditAsk::Compare { id } => {
                let line = |kind, text: &str| (kind, text.to_owned());
                let lines = vec![
                    line(DiffKind::Same, "# Changelog"),
                    line(DiffKind::Same, ""),
                    line(DiffKind::Same, "## 2024-10-12"),
                    line(DiffKind::Mine, "- Checkout: free shipping banner over 50 €"),
                    line(DiffKind::Theirs, "- Checkout: free shipping banner"),
                    line(DiffKind::Same, "- Fixed the cart badge on small screens"),
                    line(DiffKind::Same, ""),
                    line(DiffKind::Same, "## 2024-10-05"),
                    line(DiffKind::Same, "- New product page"),
                    line(DiffKind::Theirs, "- Hotfix: prices round to two places (added on the server)"),
                ];
                vec![EditReply::Compared { id, result: Ok(Comparison { binary: false, lines }) }]
            }
            EditAsk::Close { id } => match self.open.remove(&id) {
                Some(_) => vec![event(&id, EditEvent::Closed { kept: None })],
                None => Vec::new(),
            },
            EditAsk::CloseHost { host } => {
                let ids: Vec<String> = self.open.iter().filter(|(_, (h, _))| *h == host).map(|(id, _)| id.clone()).collect();
                ids.into_iter().flat_map(|id| self.answer(EditAsk::Close { id }, files)).collect()
            }
            EditAsk::CloseAll => {
                let ids: Vec<String> = self.open.keys().cloned().collect();
                ids.into_iter().flat_map(|id| self.answer(EditAsk::Close { id }, files)).collect()
            }
            EditAsk::ResumeLeftover { id, host } => {
                let path = if id == "left-1" { "/home/deploy/nginx.conf" } else { "/home/deploy/README.md" };
                let mut out = vec![EditReply::Resumed { id: id.clone(), host: host.clone(), path: path.into(), local: local(&id, path) }];
                self.open.insert(id.clone(), (host.clone(), path.into()));
                if host == STAGING {
                    out.push(event(&id, EditEvent::Offline { pending: true, retry_in_ms: 15_000, message: "Can't reach the server: connection timed out".into() }));
                } else {
                    out.extend(saved(&id));
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::workbench::TabId;

    const WEB: &str = "host:demo-web";

    fn files() -> DemoFiles {
        DemoFiles::new(WEB, STAGING, "host:demo-backups", "host:demo-db")
    }

    fn open(edit: &mut DemoEdit, host: &str, path: &str, confirmed: bool) -> Vec<EditReply> {
        edit.answer(EditAsk::Open { tab: TabId(2), host: host.into(), path: path.into(), editor: editors()[0].clone(), confirmed }, &mut files())
    }

    fn last_event(replies: &[EditReply]) -> Option<&EditEvent> {
        replies.iter().rev().find_map(|r| match r {
            EditReply::Event { event, .. } => Some(event),
            _ => None,
        })
    }

    #[test]
    fn each_kind_of_file_shows_its_part_of_the_flow() {
        let mut edit = DemoEdit::default();
        assert!(matches!(last_event(&open(&mut edit, WEB, "/home/deploy/.env", false)), Some(EditEvent::Uploaded { .. })));
        assert!(matches!(last_event(&open(&mut edit, WEB, CONTESTED, false)), Some(EditEvent::Conflict { gone: false })));
        assert!(matches!(last_event(&open(&mut edit, STAGING, "/home/deploy/README.md", false)), Some(EditEvent::Offline { pending: true, .. })));
        let big = open(&mut edit, WEB, "/home/deploy/logs/access-2024-09.log", false);
        assert!(matches!(&big[..], [EditReply::Opened { result: Ok(Opened::NeedsConfirm { size: 241_172_480 }), .. }]));
        assert!(matches!(last_event(&open(&mut edit, WEB, "/home/deploy/logs/access-2024-09.log", true)), Some(EditEvent::Uploaded { .. })));
        let huge = open(&mut edit, WEB, super::super::files::ENDLESS, true);
        assert!(matches!(&huge[..], [EditReply::Opened { result: Err(f), .. }] if f.message.contains("1.6 GB")));
        // Closing the server's tabs ends its sessions.
        let closed = edit.answer(EditAsk::CloseHost { host: WEB.into() }, &mut files());
        assert_eq!(closed.len(), 3);
        assert!(closed.iter().all(|r| matches!(r, EditReply::Event { event: EditEvent::Closed { kept: None }, .. })));
    }

    #[test]
    fn the_start_finds_made_up_editors_and_only_offers_leftovers_when_asked() {
        let mut edit = DemoEdit::default();
        let start = edit.answer(EditAsk::Start { custom: None }, &mut files());
        assert!(matches!(&start[..], [EditReply::Editors { found, .. }, EditReply::Leftovers(l)] if found[0].name == "VS Code" && l.is_empty()));
        edit.leftovers = true;
        let start = edit.answer(EditAsk::Start { custom: None }, &mut files());
        assert!(matches!(&start[1], EditReply::Leftovers(l) if l.len() == 2));
        // Nothing real: every path is made up.
        assert!(editors().iter().all(|e| !e.program.starts_with("/root")));
    }
}
