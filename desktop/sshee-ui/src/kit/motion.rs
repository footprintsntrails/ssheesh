//! How what opens over the window comes and goes: short, quiet, and the same everywhere.
//!
//! - a dialog's dimmed layer fades in; its card fades in from 96% of its size;
//! - a popover or menu fades in from 97%, growing from the edge it hangs from;
//! - a toast fades in and rises from its foot;
//! - Settings fades in over the page.
//!
//! An entrance is a one-shot [`Sequence`] on Rune's render clock, not a transition: it plays
//! whether the commit that opened it animates or lands at once (most handlers call
//! `tx.instant()` so the terminal never springs), it asks for frames only while it plays, and
//! nothing after. Leaving is the element's transition: a quick fade (and a slight shrink for a
//! card or menu) in a commit that animates; one that lands at once removes it at once. Reduce
//! Motion (the system's, or Settings → Appearance → Animations off) lands an entrance at its
//! end and turns a leave into Rune's brief fade, without the shrink.

use rune::{Beat, Easing, El, Sequence, Spring, SpringPair, Transition};

/// What is coming in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrance {
    /// A dialog's dimmed layer.
    Scrim,
    /// A dialog's card, in the middle of the window.
    Card,
    /// A popover or menu that hangs below what opened it.
    Pop,
    /// A toast, at the foot of the window.
    Toast,
    /// A page over the page (Settings).
    Page,
}

/// Ease-out: fast at first, settling gently.
const OUT: Easing = Easing::cubic(0.16, 1.0, 0.3, 1.0);

static SCRIM: [Beat; 2] = [Beat::new(0.0).opacity(0.0), Beat::new(0.16).easing(OUT)];
static CARD: [Beat; 2] = [Beat::new(0.0).opacity(0.0).scale(0.96), Beat::new(0.18).easing(OUT)];
static POP: [Beat; 2] = [Beat::new(0.0).opacity(0.0).scale(0.97), Beat::new(0.12).easing(OUT)];
static TOAST: [Beat; 2] = [Beat::new(0.0).opacity(0.0).scale(0.94), Beat::new(0.2).easing(OUT)];
static PAGE: [Beat; 2] = [Beat::new(0.0).opacity(0.0), Beat::new(0.14).easing(OUT)];

/// `e` comes in as `how` says, and goes with a quick fade.
pub fn enters(e: El, how: Entrance) -> El {
    let (beats, origin, leave): (&'static [Beat], [f32; 2], Option<Transition>) = match how {
        Entrance::Scrim => (&SCRIM, [0.5, 0.5], Some(fade(0.12))),
        Entrance::Card => (&CARD, [0.5, 0.5], Some(fade(0.12).combined(Transition::scale(0.98)))),
        // From the edge it hangs from: the top.
        Entrance::Pop => (&POP, [0.5, 0.0], Some(fade(0.1).combined(Transition::scale(0.98)))),
        // From its foot, as if it rose into place.
        Entrance::Toast => (&TOAST, [0.5, 1.0], Some(fade(0.14))),
        // Settings gives way to the page at once: a fade would show both at the same time.
        Entrance::Page => (&PAGE, [0.5, 0.5], None),
    };
    let e = e.sequence(Sequence::new(beats)).origin(origin[0], origin[1]);
    // The entrance is the sequence's: the transition only plays the leave.
    let leave = leave.unwrap_or_else(Transition::none);
    e.transition(Transition::asymmetric(Transition::identity(), leave).springs(SpringPair::new(Spring::NONE, leave.springs.exit)))
}

/// A fade out over `seconds`.
fn fade(seconds: f32) -> Transition {
    Transition::opacity().springs(SpringPair::new(Spring::NONE, Spring::new(seconds, 0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrances_are_short_and_end_whole() {
        for beats in [&SCRIM[..], &CARD, &POP, &TOAST, &PAGE] {
            let last = beats.last().unwrap();
            assert!(last.at <= 0.2, "{last:?}");
            assert_eq!((last.opacity, last.scale), (1.0, 1.0));
            assert_eq!(beats[0].opacity, 0.0);
        }
    }
}
