//! What 0.10's pane header said of a terminal's session besides its name (`SessionPill` and
//! `ProgressLine` in `Pane.tsx`). A pane beside another has a header of its own (`split.rs`);
//! the terminal alone on the page says "Not kept" in the status bar instead, where its coming
//! and going moves nothing in the row of tabs.
//!
//! ```text
//! not-kept-N    "Not kept": a server's shell outside tmux, which a dropped connection ends;
//!               under the pointer, what would keep it
//! progress-N    a 2-point line along the top of the terminal while a program says how far it
//!               is (OSC 9;4): filling as it goes, a segment sweeping when it can't tell
//! ```

use rune::{el, text, El, LoopSpec};
use sshee_app::workbench::{TabId, NOT_KEPT};
use sshee_term::osc::ProgressState;

use super::state::Shell;
use crate::kit::tab::{progress_color, progress_words};
use crate::theme::{self, Theme};

/// How long the sweeping segment of a progress that can't tell how far it is takes to cross.
const SWEEP: f64 = 1.4;

/// The terminal the status bar speaks for: the one on screen, alone on the page (beside others,
/// each pane has its own header).
fn alone(s: &Shell) -> Option<TabId> {
    s.bench.active_id().filter(|id| !s.screens.settings.open && !s.bench.showing_home() && s.bench.layout().shown_with(*id).is_none())
}

/// How wide "Not kept" is in the status bar.
pub(crate) fn status_width() -> f32 {
    rune::text::advance(NOT_KEPT.0, theme::STATUS)
}

/// "Not kept" in the status bar, for the terminal on screen alone, in the bar's own type.
pub(crate) fn status_not_kept(s: &Shell, t: &Theme) -> Option<El> {
    let id = alone(s)?;
    s.bench.tab(id).filter(|tab| tab.not_kept())?;
    let (words, hint) = NOT_KEPT;
    Some(text(words).key(format!("not-kept-{}", id.0)).font(theme::STATUS).color(t.l2).single_line().help(hint).label(format!("{words}. {hint}")))
}

/// "Not kept", quietly, after the terminal's name, while its shell on a server isn't in tmux.
/// In a narrow header it gives way before the buttons do.
pub(crate) fn not_kept(s: &Shell, id: TabId, t: &Theme) -> Option<El> {
    s.bench.tab(id).filter(|tab| tab.not_kept())?;
    let (words, hint) = NOT_KEPT;
    Some(text(words).key(format!("not-kept-{}", id.0)).font(theme::TAB_SERVER).color(t.l2).single_line().truncate().shrink().min_w(0.0).help(hint).label(format!("{words}. {hint}")))
}

/// The line a program's progress draws along the top of its terminal, pinned over it.
pub(crate) fn progress_line(s: &Shell, id: TabId, t: &Theme) -> Option<El> {
    let p = s.pane(id)?.progress()?;
    let color = progress_color(t, p.state);
    let fill = match p.state {
        ProgressState::Indeterminate => el().w_frac(1.0 / 3.0).h_full().fill(color).looping(LoopSpec::slide(SWEEP, [-1.0, 0.0], [3.0, 0.0])),
        _ => el().w_frac((p.value / 100.0).clamp(0.0, 1.0)).h_full().fill(color),
    };
    Some(
        el().key(format!("progress-{}", id.0))
            .pin(Some(0.0), Some(0.0), Some(0.0), None)
            .h(2.0)
            .clip_rounded(0.0)
            .role("progressbar")
            .label(progress_words(Some(p)))
            .child(fill),
    )
}
