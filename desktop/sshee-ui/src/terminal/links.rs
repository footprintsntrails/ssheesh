//! Links in a terminal's output: web links (a program's OSC 8 ones too) and file paths, as the
//! presentation pass found them (`sshee_term::present::Link`). The one under the pointer is
//! underlined (a path's underline is dotted) and says how to open it; Ctrl+click (⌘-click on a
//! Mac) opens it: a web link in the browser, a path through the window (`Wish::OpenPath`, for
//! the Files screen).
//!
//! Only links the pass handed out exist here, so one that runs over a hidden secret or a name
//! hidden by streamer mode is no link at all, and a hint never names anything hidden.
//!
//! A program's link whose text isn't where it goes (`Link::disguised`) is asked about first, as
//! 0.10 asked: the first Ctrl+click only says where it goes, the second opens it.

use std::ops::Range;
use std::time::Duration;

use rune::{text, El, Mods, Tx};
use sshee_term::input::Platform;
use sshee_term::links::{resolve_path, LinkClick, LinkKind};
use sshee_term::present::{Link, Role};

use super::metrics::CellMetrics;
use super::overlays::Wish;
use super::pane::{TermPane, PADDING};
use super::rows::LinkCells;
use crate::theme::{self, Theme};

/// How long the pointer rests on a link before the hint shows (Rune's tooltips wait as long).
const HINT_DELAY: f64 = 0.5;

/// How long a note about a click stays ("Copied the path").
const NOTE_FOR: f64 = 2.5;

/// The link under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    /// The cell the hint hangs under: the link's first on the pointer's row.
    pub line: usize,
    pub col: u16,
    pub link: Link,
    /// The link's cells, by view line: a long link runs over several rows.
    pub parts: LinkCells,
    /// When the hint shows (pane clock).
    hint_at: f64,
    hint_shown: bool,
}

impl Hover {
    /// A path's underline is dotted; a web link's is a line.
    pub fn dotted(&self) -> bool {
        self.link.kind == LinkKind::Path
    }
}

/// A pane's links.
#[derive(Default)]
pub struct Links {
    /// The folder the shell last reported (OSC 7): a relative path is found from there.
    pub(crate) cwd: Option<String>,
    pub(crate) hover: Option<Hover>,
    /// The open key was down when the pointer last moved: the pointer is a hand over a link.
    open_key: bool,
    /// The web link asked about (see the module's docs): its target.
    asked: Option<String>,
    /// What the last click did, and until when it says so.
    note: Option<(String, f64)>,
}

impl Links {
    /// The pointer is a hand: over a link, with the key that opens it held.
    pub(crate) fn pointing(&self) -> bool {
        self.hover.is_some() && self.open_key
    }
}

/// The key that opens a link with a click: ⌘ on a Mac, Ctrl elsewhere.
fn open_key(mods: Mods, platform: Platform) -> bool {
    if platform == Platform::Mac {
        mods.logo
    } else {
        mods.ctrl
    }
}

/// What a link says under the pointer: where a disguised one goes, and how to open it.
pub(crate) fn hint_text(link: &Link, platform: Platform, asked: bool) -> String {
    let key = if platform == Platform::Mac { "Cmd" } else { "Ctrl" };
    match (link.disguised, asked) {
        (true, true) => format!("Goes to {}. {key}+click again to open", link.target),
        (true, false) => format!("Goes to {}. {key}+click to open", link.target),
        (false, _) => format!("{key}+click to open"),
    }
}

impl TermPane {
    /// The link at a cell of the view, with all its cells.
    fn link_at(&self, line: usize, col: u16) -> Option<(Link, u16, LinkCells)> {
        let mut view = self.view.borrow_mut();
        let (rows, cols) = (view.size.rows as usize, view.size.cols);
        let row = view.shown(line)?;
        let span = row.span_at(col, |r| matches!(r, Role::Link(_)))?;
        let Role::Link(i) = span.role else { return None };
        let link = row.links.get(i)?.clone();
        let here = span.cols.clone();
        // The same link on a neighbouring row, where it runs to this row's edge.
        let part_of = |view: &mut super::rows::PaneView, l: usize, at_start: bool| -> Option<Range<u16>> {
            let row = view.shown(l)?;
            row.spans.iter().find_map(|s| match s.role {
                Role::Link(k) if row.links.get(k) == Some(&link) && (if at_start { s.cols.start == 0 } else { s.cols.end >= cols }) => Some(s.cols.clone()),
                _ => None,
            })
        };
        let mut parts = vec![(line, here.clone())];
        let (mut up, mut edge) = (line, here.start);
        while up > 0 && edge == 0 {
            let Some(cols) = part_of(&mut view, up - 1, false) else { break };
            up -= 1;
            edge = cols.start;
            parts.insert(0, (up, cols));
        }
        let (mut down, mut edge) = (line, here.end);
        while down + 1 < rows && edge >= cols {
            let Some(cols) = part_of(&mut view, down + 1, true) else { break };
            down += 1;
            edge = cols.end;
            parts.push((down, cols));
        }
        Some((link, here.start, parts))
    }

    /// The pointer moved to a cell: the link there is the one underlined. True when what is
    /// drawn changed.
    pub(crate) fn links_moved(&mut self, line: usize, col: u16, tx: &mut Tx) -> bool {
        let key = open_key(tx.mods(), self.platform);
        let found = self.link_at(line, col);
        let links = &mut self.over.links;
        let mut changed = std::mem::replace(&mut links.open_key, key) != key && links.hover.is_some();
        let same = match (&links.hover, &found) {
            (Some(h), Some((link, _, parts))) => h.link == *link && h.parts == *parts,
            (None, None) => true,
            _ => false,
        };
        if same {
            return changed;
        }
        changed = true;
        links.hover = found.map(|(link, col, parts)| Hover { line, col, link, parts, hint_at: self.clock + HINT_DELAY, hint_shown: false });
        if links.hover.is_some() {
            tx.wake_after(Duration::from_secs_f64(HINT_DELAY));
        }
        self.underline_hover();
        changed
    }

    /// The pointer left the terminal.
    pub(crate) fn links_left(&mut self) -> bool {
        if self.over.links.hover.take().is_none() {
            return false;
        }
        self.underline_hover();
        true
    }

    fn underline_hover(&mut self) {
        let marks = self.over.links.hover.as_ref().map(|h| (h.parts.clone(), h.dotted()));
        self.view.borrow_mut().set_underline(marks);
    }

    /// A press on a cell with the open key held: opens the link there. True when there was one
    /// (the press is then no selection and no report to the program).
    pub(crate) fn link_pressed(&mut self, line: usize, col: u16, clicks: u32, tx: &mut Tx) -> bool {
        if !open_key(tx.mods(), self.platform) {
            return false;
        }
        let Some((link, ..)) = self.link_at(line, col) else { return false };
        let has_selection = self.view.borrow().selection.is_some();
        let asked = self.over.links.asked.take();
        match link.click(true, clicks, has_selection) {
            LinkClick::AskNow if asked.as_deref() != Some(link.target.as_str()) => {
                // Not opened blind: the hint names the place, and the next click goes there.
                self.over.links.asked = Some(link.target.clone());
                if let Some(h) = &mut self.over.links.hover {
                    h.hint_shown = true;
                }
            }
            LinkClick::OpenNow | LinkClick::AskNow => self.open_link(&link, tx),
            _ => {}
        }
        tx.instant();
        true
    }

    fn open_link(&mut self, link: &Link, tx: &mut Tx) {
        match link.kind {
            LinkKind::Url => {
                if tx.open_url(&link.target).is_err() {
                    self.say("Couldn't open the link", tx);
                }
            }
            LinkKind::Path => {
                let path = resolve_path(&link.target, self.over.links.cwd.as_deref().unwrap_or("~"));
                self.over.wish(Wish::OpenPath { path, line: link.line });
            }
        }
    }

    /// Says something about the last click where the hint goes, for a moment: "Copied the
    /// path". The window calls it with what it did with a wish.
    pub fn say(&mut self, note: &str, tx: &mut Tx) {
        self.over.links.note = Some((note.to_owned(), self.clock + NOTE_FOR));
        tx.wake_after(Duration::from_secs_f64(NOTE_FOR));
        tx.instant();
    }

    /// The hint shows once the pointer has rested, and a note goes when its time is up.
    pub(crate) fn poll_links(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let links = &mut self.over.links;
        let mut changed = false;
        if let Some(h) = links.hover.as_mut().filter(|h| !h.hint_shown) {
            if clock + 1e-6 >= h.hint_at {
                h.hint_shown = true;
                changed = true;
            } else {
                tx.wake_after(Duration::from_secs_f64(h.hint_at - clock));
            }
        }
        if let Some((_, until)) = &links.note {
            if clock + 1e-6 >= *until {
                links.note = None;
                changed = true;
            } else {
                tx.wake_after(Duration::from_secs_f64(*until - clock));
            }
        }
        changed
    }
}

/// The hint under the link the pointer rests on, or what the last click did.
pub(crate) fn hint(pane: &TermPane, t: &'static Theme, m: &CellMetrics) -> Option<El> {
    let links = &pane.over.links;
    let said = match (&links.note, &links.hover) {
        (Some((note, _)), _) => note.clone(),
        (None, Some(h)) if h.hint_shown => hint_text(&h.link, pane.platform, links.asked.as_deref() == Some(h.link.target.as_str())),
        _ => return None,
    };
    let [w, h] = pane.area?;
    // Under the link's first cell on the pointer's row; above it when there's no room below,
    // and never off the pane's sides. A note with no link under the pointer sits at the top.
    let (line, col) = links.hover.as_ref().map_or((0, 0), |h| (h.line, h.col));
    let width = (rune::text::advance(&said, theme::SMALL) + 18.0).min(w - 16.0);
    let x = (PADDING + f32::from(col) * m.cell_w).min(w - width - 8.0).max(8.0);
    let top = pane.line_y(m, line);
    let below = top + m.cell_h + 4.0;
    let y = if below + 24.0 > h - 4.0 { (top - 28.0).max(4.0) } else { below };
    Some(
        rune::row()
            .key("link-hint")
            .absolute(x, y)
            .w(width)
            .h(24.0)
            .px(9.0)
            .items_center()
            .radius(theme::RADIUS_ROW)
            .fill(t.pop)
            .border(1.0, t.rim)
            .shadow(4.0, 12.0, rune::Color::BLACK.alpha(0.25))
            .child(text(said).font(theme::SMALL).color(t.l1).single_line().truncate().shrink()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(disguised: bool) -> Link {
        Link { kind: LinkKind::Url, target: "https://example.com/docs".into(), line: None, end_line: None, from_program: disguised, disguised }
    }

    #[test]
    fn a_link_says_how_to_open_it_and_a_disguised_one_where_it_goes() {
        assert_eq!(hint_text(&link(false), Platform::Other, false), "Ctrl+click to open");
        assert_eq!(hint_text(&link(false), Platform::Mac, false), "Cmd+click to open");
        assert_eq!(hint_text(&link(true), Platform::Windows, false), "Goes to https://example.com/docs. Ctrl+click to open");
        assert_eq!(hint_text(&link(true), Platform::Other, true), "Goes to https://example.com/docs. Ctrl+click again to open");
    }

    #[test]
    fn the_open_key_is_command_on_a_mac_and_ctrl_elsewhere() {
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        let logo = Mods { logo: true, ..Mods::default() };
        assert!(open_key(ctrl, Platform::Other) && open_key(ctrl, Platform::Windows) && !open_key(ctrl, Platform::Mac));
        assert!(open_key(logo, Platform::Mac) && !open_key(logo, Platform::Other));
    }
}
