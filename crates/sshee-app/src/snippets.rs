//! Snippets without a screen: commands you keep and run again, with blanks to fill in
//! (0.10's `src/lib/snippets.ts` and `Snippets.tsx`).
//!
//! - A blank is written `{{name}}`, or `{{name:what it is if you leave it empty}}`; each is
//!   asked once, in order. `{{container}}` offers the server's containers to pick from. Go
//!   templates' own words (`{{end}}`, `{{else}}`) aren't blanks, so `docker inspect -f` works.
//! - A snippet runs in the terminal of the server on screen. 1.0 adds one thing: a snippet can
//!   be kept for one server (`only_on`), and then it's offered only there. That key lives in
//!   the snippet's extra fields, which 0.10 keeps as they are, so its own list still reads.
//! - Running types the command into the terminal as a paste (a shell doesn't run each line of
//!   a long one as it arrives) and presses Return; "Paste" types it without Return.
//!
//! The list itself is [`crate::model::Snippets`], 0.10's store, as 1.0 keeps it in its own
//! folder. A snippet's command may hold a secret: nothing here prints one.

use crate::model::{Snippet, Snippets};

/// The key in a snippet's extra fields that keeps it to one server (a server's key in the
/// sidebar, `host:…`), 1.0's own.
pub const ONLY_ON: &str = "onlyOn";

/// One blank of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blank {
    pub name: String,
    /// Used when it's left empty.
    pub fallback: String,
}

impl Blank {
    /// It asks for one of the server's containers.
    pub fn wants_container(&self) -> bool {
        self.name.eq_ignore_ascii_case("container") || self.name.eq_ignore_ascii_case("containers")
    }

    /// "max_lines" → "Max lines".
    pub fn label(&self) -> String {
        blank_label(&self.name)
    }
}

/// Go templates (docker inspect -f, kubectl -o go-template) use these: they aren't blanks.
const GO_WORDS: [&str; 5] = ["end", "else", "break", "continue", "nil"];

/// A blank's place in `command`: where `{{` starts, where `}}` ends, its name and fallback.
struct Found<'a> {
    start: usize,
    end: usize,
    name: &'a str,
    fallback: Option<&'a str>,
}

/// Every `{{ name }}` / `{{ name : fallback }}` in order, Go's words included.
fn find(command: &str) -> Vec<Found<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = command[at..].find("{{").map(|i| i + at) {
        let inner_start = open + 2;
        let Some(close) = command[inner_start..].find("}}").map(|i| i + inner_start) else { break };
        let inner = &command[inner_start..close];
        // A fallback may not hold a `}` (0.10's pattern: `[^}]*`).
        let parsed = (|| {
            let (head, fallback) = match inner.split_once(':') {
                Some((h, f)) => (h, Some(f)),
                None => (inner, None),
            };
            let name = head.trim();
            let mut chars = name.chars();
            let first = chars.next()?;
            let ok = (first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            (ok && !fallback.is_some_and(|f| f.contains('}'))).then_some((name, fallback))
        })();
        match parsed {
            Some((name, fallback)) => {
                out.push(Found { start: open, end: close + 2, name, fallback });
                at = close + 2;
            }
            None => at = open + 2,
        }
    }
    out
}

/// The blanks in a command, each once, in order.
pub fn blanks(command: &str) -> Vec<Blank> {
    let mut seen: Vec<Blank> = Vec::new();
    for f in find(command) {
        if GO_WORDS.contains(&f.name) || seen.iter().any(|b| b.name == f.name) {
            continue;
        }
        seen.push(Blank { name: f.name.to_owned(), fallback: f.fallback.unwrap_or("").trim().to_owned() });
    }
    seen
}

/// The command with its blanks filled in: what was typed, else the fallback, else nothing.
pub fn fill(command: &str, values: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(command.len());
    let mut at = 0;
    for f in find(command) {
        out.push_str(&command[at..f.start]);
        if GO_WORDS.contains(&f.name) {
            out.push_str(&command[f.start..f.end]);
        } else {
            let typed = values(f.name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
            out.push_str(&typed.unwrap_or_else(|| f.fallback.unwrap_or("").trim().to_owned()));
        }
        at = f.end;
    }
    out.push_str(&command[at..]);
    out
}

/// Whether every blank without a fallback has something typed in it.
pub fn complete(command: &str, values: &dyn Fn(&str) -> Option<String>) -> bool {
    blanks(command).iter().all(|b| !b.fallback.is_empty() || values(&b.name).is_some_and(|v| !v.trim().is_empty()))
}

/// "max_lines" → "Max lines".
pub fn blank_label(name: &str) -> String {
    let words = name.replace(['_', '-'], " ");
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The server a snippet is kept for, if it is kept for one.
pub fn only_on(snippet: &Snippet) -> Option<&str> {
    snippet.extra.get(ONLY_ON).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

/// Whether a snippet is offered on `server` (a server's key; `None` on the main menu, where
/// only snippets for every server are offered).
pub fn offered_on(snippet: &Snippet, server: Option<&str>) -> bool {
    match only_on(snippet) {
        None => true,
        Some(only) => server == Some(only),
    }
}

/// The snippets offered on `server`, in the list's order.
pub fn for_server<'a>(list: &'a [Snippet], server: Option<&str>) -> Vec<&'a Snippet> {
    list.iter().filter(|s| offered_on(s, server)).collect()
}

/// Why a snippet can't be saved yet, or `None` when it can.
pub fn check(name: &str, command: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        Some("Give it a name.")
    } else if command.trim().is_empty() {
        Some("Write the command.")
    } else {
        None
    }
}

/// A snippet saved from the editor: `id` keeps an existing one's place, else it goes at the
/// end with a new id. `only` keeps it to one server.
pub fn save(list: &mut Snippets, id: Option<&str>, name: &str, command: &str, only: Option<&str>) -> String {
    let id = id.map(str::to_owned).unwrap_or_else(|| new_id(&list.snippets));
    let mut snippet = list.snippets.iter().find(|s| s.id == id).cloned().unwrap_or_default();
    snippet.id = id.clone();
    snippet.name = name.trim().to_owned();
    snippet.command = command.trim().to_owned();
    match only.filter(|o| !o.is_empty()) {
        Some(server) => {
            snippet.extra.insert(ONLY_ON.into(), crate::model::Json::String(server.to_owned()));
        }
        None => {
            snippet.extra.shift_remove(ONLY_ON);
        }
    }
    match list.snippets.iter_mut().find(|s| s.id == id) {
        Some(old) => *old = snippet,
        None => list.snippets.push(snippet),
    }
    id
}

/// Takes a snippet out of the list.
pub fn remove(list: &mut Snippets, id: &str) {
    list.snippets.retain(|s| s.id != id);
}

/// The tab the main window's fill-in form asks the server's containers for (a
/// `containers::Ask` is addressed to a tab, and no tab has this number, the last of the
/// window's range): its answer comes back to the form. Another window's is [`form_tab`].
pub const FORM_TAB: crate::workbench::TabId = form_tab(0);

/// [`FORM_TAB`] in window `window`'s range of tab ids, so the answer finds that window.
pub const fn form_tab(window: u16) -> crate::workbench::TabId {
    crate::workbench::TabId::in_window(window, u64::MAX)
}

/// One container a `{{container}}` blank offers: its name, and how the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    /// "web-api", or "worker (stopped)".
    pub label: String,
}

/// What a `{{container}}` blank offers from a look at the server: running containers first,
/// then by name (0.10's form).
pub fn container_choices(list: &crate::containers::ContainerList) -> Vec<Choice> {
    let mut all: Vec<&crate::containers::Container> = list.containers.iter().collect();
    all.sort_by(|a, b| b.running.cmp(&a.running).then_with(|| a.name.cmp(&b.name)));
    all.dedup_by(|a, b| a.name == b.name);
    all.into_iter().map(|c| Choice { name: c.name.clone(), label: if c.running { c.name.clone() } else { format!("{} (stopped)", c.name) } }).collect()
}

/// An id no snippet has: `s-1`, `s-2`… (0.10's are `s-` and a random word; any unique text
/// does).
fn new_id(list: &[Snippet]) -> String {
    (1..).map(|n| format!("s-{n}")).find(|id| !list.iter().any(|s| &s.id == id)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_container_blank_offers_running_containers_first() {
        use crate::containers::{Container, ContainerList};
        use crate::model::Engine;
        let c = |name: &str, running: bool| Container { engine: Engine::Docker, id: name.into(), name: name.into(), image: "img".into(), running, state: String::new(), status: String::new(), ports: String::new(), project: None };
        let list = ContainerList { containers: vec![c("worker", false), c("web-api", true), c("db", true), c("cache", false)], ..ContainerList::default() };
        let labels: Vec<String> = container_choices(&list).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["db", "web-api", "cache (stopped)", "worker (stopped)"]);
        assert_eq!(container_choices(&list)[3].name, "worker");
        assert!(container_choices(&ContainerList::default()).is_empty());
    }

    #[test]
    fn blanks_are_found_once_in_order_and_go_words_are_not_blanks() {
        let b = blanks("docker logs -f --tail {{lines:100}} {{container}} && echo {{ container }}");
        assert_eq!(b, [Blank { name: "lines".into(), fallback: "100".into() }, Blank { name: "container".into(), fallback: String::new() }]);
        assert!(b[1].wants_container() && !b[0].wants_container());
        assert!(blanks("docker inspect -f '{{range .Mounts}}{{.Source}}{{end}}' x").is_empty());
        assert!(blanks("echo {{end}} {{else}}").is_empty());
        assert!(blanks("df -h").is_empty());
        // Not a name: left as it is.
        assert!(blanks("echo {{1x}} {{}} {{a b}}").is_empty());
        assert_eq!(blanks("du {{folder:.}} {{max_lines: 20 }}")[1].fallback, "20");
    }

    #[test]
    fn filling_uses_what_was_typed_else_the_fallback() {
        let cmd = "sudo systemctl restart {{service}} && systemctl status --no-pager {{service}} | head -n {{lines:20}}";
        assert_eq!(fill(cmd, &none), "sudo systemctl restart  && systemctl status --no-pager  | head -n 20");
        let typed = |n: &str| (n == "service").then(|| " nginx ".to_owned());
        assert_eq!(fill(cmd, &typed), "sudo systemctl restart nginx && systemctl status --no-pager nginx | head -n 20");
        assert!(!complete(cmd, &none) && complete(cmd, &typed));
        // Go templates stay.
        assert_eq!(fill("docker inspect -f '{{.State}}{{end}}' {{c}}", &|_| Some("web".into())), "docker inspect -f '{{.State}}{{end}}' web");
        assert_eq!(fill("echo {{ unclosed", &none), "echo {{ unclosed");
    }

    #[test]
    fn labels_read_as_words() {
        assert_eq!(blank_label("max_lines"), "Max lines");
        assert_eq!(blank_label("api-token"), "Api token");
        assert_eq!(blank_label("x"), "X");
    }

    #[test]
    fn a_snippet_kept_for_one_server_is_offered_only_there() {
        let mut list = Snippets::default();
        let n = list.snippets.len();
        let id = save(&mut list, None, " Tail the shop's log ", "tail -f /var/log/shop.log", Some("host:web"));
        assert_eq!(list.snippets.len(), n + 1);
        assert_eq!(list.snippets[n].name, "Tail the shop's log");
        assert_eq!(only_on(&list.snippets[n]), Some("host:web"));
        assert_eq!(for_server(&list.snippets, Some("host:web")).len(), n + 1);
        assert_eq!(for_server(&list.snippets, Some("host:db")).len(), n);
        assert_eq!(for_server(&list.snippets, None).len(), n);
        // Saved again for every server, in its place.
        let again = save(&mut list, Some(&id), "Tail it", "tail -f x", None);
        assert_eq!((again.as_str(), list.snippets.len(), only_on(&list.snippets[n])), (id.as_str(), n + 1, None));
        // 0.10's own fields stay as they were, and the new key goes out with the rest.
        let json = serde_json::to_value(&list).unwrap();
        assert!(json["snippets"][n].get(ONLY_ON).is_none());
        save(&mut list, Some(&id), "Tail it", "tail -f x", Some("host:db"));
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json["snippets"][n][ONLY_ON], "host:db");
        remove(&mut list, &id);
        assert_eq!(list.snippets.len(), n);
        // New ids don't repeat.
        let a = save(&mut list, None, "a", "a", None);
        let b = save(&mut list, None, "b", "b", None);
        assert_ne!(a, b);
    }

    #[test]
    fn a_snippet_needs_a_name_and_a_command() {
        assert_eq!(check("", "df"), Some("Give it a name."));
        assert_eq!(check("Disks", "  "), Some("Write the command."));
        assert_eq!(check("Disks", "df -h"), None);
    }
}
