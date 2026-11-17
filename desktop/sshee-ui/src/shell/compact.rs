//! Compact mode (0.10's `EdgeReveal.tsx`): the sidebar, the top row, or both stand aside and
//! slide in while the pointer is at the window's edge, over the page; "Both" also hides the
//! status bar. Settings → Layout → Compact mode picks which, and its shortcut turns it off and
//! back to what it was (`compactLast`).
//!
//! ```text
//! window
//! ├── edge-top    a thin band along the top (room for a Mac's window buttons): the row comes
//! ├── edge-side   a thin band along the sidebar's side: the sidebar comes
//! ├── out-top     the top row over the page, while the pointer is on it
//! └── out-side    the sidebar over the page, likewise
//! ```

use rune::{el, El, Tx};
use sshee_app::model::CompactMode;

use super::state::Shell;
use crate::theme::{self, Frame, Theme};

/// The band at an edge that brings a bar in.
const BAND: f32 = 6.0;

/// Which bars are out now.
#[derive(Default)]
pub struct Compact {
    pub(crate) top: bool,
    pub(crate) side: bool,
}

impl Shell {
    fn compact_mode(&self) -> CompactMode {
        self.settings.get().compact_mode.clone()
    }

    /// The sidebar stands aside until the pointer comes to its edge.
    pub fn sidebar_away(&self) -> bool {
        matches!(self.compact_mode(), CompactMode::Sidebar | CompactMode::Both)
    }

    /// The top row stands aside likewise.
    pub fn bar_away(&self) -> bool {
        matches!(self.compact_mode(), CompactMode::Tabbar | CompactMode::Both)
    }

    /// The shortcut: compact mode off, or back to the kind it was last.
    pub fn toggle_compact(&mut self, tx: &mut Tx) {
        self.compact = Compact::default();
        self.change_settings(tx, |st| {
            st.set(|s| {
                if s.compact_mode == CompactMode::Off {
                    s.compact_mode = if s.compact_last == CompactMode::Off { CompactMode::Both } else { s.compact_last.clone() };
                } else {
                    s.compact_last = s.compact_mode.clone();
                    s.compact_mode = CompactMode::Off;
                }
            })
        });
        self.settle_in_place(tx);
    }

    /// The pointer came to an edge, or left the bar it brought out.
    pub(crate) fn reveal(&mut self, top: bool, out: bool, tx: &mut Tx) {
        let now = if top { &mut self.compact.top } else { &mut self.compact.side };
        if *now != out {
            *now = out;
            tx.instant();
        }
    }
}

/// The band at the top that stands in for the row: it moves the window, holds a Mac's window
/// buttons, and brings the row in.
pub(crate) fn top_band(s: &Shell, t: &Theme) -> El {
    let h = if s.options.frame == Frame::Mac { 28.0 } else { BAND };
    el().key("edge-top").w_full().h(h).fill(t.frame).window_drag().on_hover_change(|s: &mut Shell, over, tx| {
        if over {
            s.reveal(true, true, tx)
        }
    })
}

/// The band along the sidebar's side.
pub(crate) fn side_band(s: &Shell) -> El {
    let band = el().key("edge-side").w(BAND).on_hover_change(|s: &mut Shell, over, tx| {
        if over {
            s.reveal(false, true, tx)
        }
    });
    if s.sidebar_at_right() {
        band.pin(None, Some(0.0), Some(0.0), Some(0.0))
    } else {
        band.pin(Some(0.0), Some(0.0), None, Some(0.0))
    }
}

/// The bars that are out, over the page: the top row, the sidebar.
pub(crate) fn out(s: &Shell, t: &'static Theme, bar: Option<El>, rail: Option<El>) -> Vec<El> {
    let mut out = Vec::new();
    let shadow = rune::Color::BLACK.alpha(if t.dark { 0.45 } else { 0.16 });
    if let (true, Some(bar)) = (s.compact.top, bar) {
        out.push(
            el().key("out-top")
                .pin(Some(0.0), Some(0.0), Some(0.0), None)
                .h(theme::BAR_HEIGHT)
                .fill(t.frame)
                .shadow(12.0, 4.0, shadow)
                .border_sides([0.0, 0.0, 1.0, 0.0], t.edge)
                .on_hover_change(|s: &mut Shell, over, tx| {
                    if !over {
                        s.reveal(true, false, tx)
                    }
                })
                .child(bar),
        );
    }
    if let (true, Some(rail)) = (s.compact.side, rail) {
        let top = if s.bar_away() { 0.0 } else { theme::BAR_HEIGHT };
        let e = el()
            .key("out-side")
            .fill(t.frame)
            .shadow(12.0, 4.0, shadow)
            .on_hover_change(|s: &mut Shell, over, tx| {
                if !over {
                    s.reveal(false, false, tx)
                }
            })
            .child(rail);
        out.push(if s.sidebar_at_right() {
            e.pin(None, Some(top), Some(0.0), Some(0.0)).border_sides([0.0, 0.0, 0.0, 1.0], t.edge)
        } else {
            e.pin(Some(0.0), Some(top), None, Some(0.0)).border_sides([0.0, 1.0, 0.0, 0.0], t.edge)
        });
    }
    out
}
