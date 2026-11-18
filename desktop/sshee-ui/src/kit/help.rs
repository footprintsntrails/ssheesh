//! A quiet "?" beside words a newcomer might stop at (tmux, a tunnel, Tor), and the card it
//! opens: what the thing is, why you'd want it, what happens when it's on, in a few plain
//! sentences.
//!
//! The "?" opens its card when the pointer rests on it, and on a click, Return or Space; the
//! card closes when the pointer leaves, on Esc, and on another click. Opened from the keys,
//! the card holds the keyboard until Esc or a click outside, and hands focus back to its "?".
//! A screen reader hears the "?" as "About …" with the explanation as its description, so it
//! needn't open anything.
//!
//! The kit keeps no state: the caller holds which card is open (an `Option<Tip>`) and hands
//! the "?" a way to reach it, and draws the card ([`help_card`]) as a late child of the
//! window's root, so nothing clips it. Only one card is open at a time.

use std::time::Duration;

use rune::{text, Align, El, Key, Side, Tx};

use crate::kit::popover::popover;
use crate::theme::{self, Theme};

/// The circle's size.
pub const SIZE: f32 = 14.0;
/// The square the pointer can hit, around the circle.
const HIT: f32 = 20.0;
/// The widest a card grows before its words wrap.
const CARD_MAX: f32 = 300.0;
/// The card's padding, across and down.
const PAD: [f32; 2] = [12.0, 9.0];
/// How long the pointer rests on the "?" before its card opens, and stays off it before it
/// closes: a pass across a row opens nothing.
const ENTER: Duration = Duration::from_millis(300);
const LEAVE: Duration = Duration::from_millis(120);

/// How a card was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// The pointer: it closes when the pointer leaves the "?".
    Pointer,
    /// Return or Space on the focused "?": it holds the keyboard until Esc or a click outside.
    Keys,
}

/// The card that is open: under which "?", what it says and how it was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tip {
    /// The "?"'s element key, which the card hangs under.
    pub key: String,
    pub text: &'static str,
    pub by: Opened,
}

/// Where the caller keeps the open card.
pub type Slot<S> = fn(&mut S) -> &mut Option<Tip>;

/// The "?" keyed `key` that explains `about` (its accessible name is "About {about}") in
/// `words`. `open`: its card is the one showing. `slot` reaches the caller's open card.
pub fn help_mark<S: 'static>(t: &Theme, key: &str, about: &str, words: &'static str, open: bool, slot: Slot<S>) -> El {
    let ink = if open { t.l1 } else { t.l2 };
    let circle = rune::el()
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .border(1.0, if open { t.l2 } else { t.l2.alpha(0.55) })
        .items_center()
        .justify_center()
        .a11y_hidden()
        .child(text("?").font(MARK).color(ink).single_line());
    let (hover_key, press_key, key_key) = (key.to_owned(), key.to_owned(), key.to_owned());
    rune::el()
        .key(key.to_owned())
        .size(HIT, HIT)
        .radius(HIT / 2.0)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(format!("About {about}"))
        .description_static(words)
        .expanded(open)
        .child(circle)
        .hover_delays(ENTER, LEAVE)
        .on_hover_change(move |s: &mut S, inside, tx| {
            let tip = slot(s);
            if inside {
                // Another "?"'s card gives way; one already open here stays as it was opened.
                if tip.as_ref().is_none_or(|o| o.key != hover_key) {
                    *tip = Some(Tip { key: hover_key.clone(), text: words, by: Opened::Pointer });
                    tx.instant();
                }
            } else if tip.as_ref().is_some_and(|o| o.key == hover_key && o.by == Opened::Pointer) {
                *tip = None;
            }
        })
        .on_press_tx(move |s: &mut S, tx| {
            let tip = slot(s);
            if tip.as_ref().is_some_and(|o| o.key == press_key) {
                *tip = None;
            } else {
                let by = if tx.by_keys() { Opened::Keys } else { Opened::Pointer };
                *tip = Some(Tip { key: press_key.clone(), text: words, by });
            }
            tx.instant();
        })
        .on_key(move |s: &mut S, ev, tx| {
            let tip = slot(s);
            // Return opens it as Space does (a button hands Return to the window's default
            // button otherwise), and closes it again.
            if ev.key == Key::Enter && ev.mods == rune::Mods::default() {
                *tip = match tip {
                    Some(o) if o.key == key_key => None,
                    _ => Some(Tip { key: key_key.clone(), text: words, by: Opened::Keys }),
                };
                tx.instant();
                return true;
            }
            if !tip.as_ref().is_some_and(|o| o.key == key_key) {
                return false;
            }
            match ev.key {
                Key::Escape => {
                    *tip = None;
                    true
                }
                // Focus moving on takes the card with it.
                Key::Tab => {
                    *tip = None;
                    false
                }
                _ => false,
            }
        })
}

/// The "?"'s glyph: small and a touch heavy, so it reads at 14 points.
const MARK: rune::tok::TextStyle = rune::tok::TextStyle::new(10.0, 600.0).lh(12.0);

/// The open card, under its "?" (above it when there's no room below), as wide as its words
/// up to [`CARD_MAX`]. Opened from the keys it is modal: Esc or a click outside runs `slot`'s
/// close, and focus goes back to the "?".
pub fn help_card<S: 'static>(t: &Theme, tip: &Tip, slot: Slot<S>) -> El {
    let width = (rune::text::advance(tip.text, theme::NOTE) + 2.0 * PAD[0] + 2.0).ceil().min(CARD_MAX);
    // A popover's surface with a tighter corner: a note, not a panel.
    let card = popover(t, width)
        .radius(theme::RADIUS)
        .key("help-card")
        .anchored(tip.key.clone(), Side::Bottom, Align::Center, 4.0)
        .px(PAD[0])
        .py(PAD[1])
        .role("tooltip")
        .child(text(tip.text).key("help-text").font(theme::NOTE).color(t.l1).wrap().w_full());
    match tip.by {
        Opened::Pointer => card,
        Opened::Keys => card
            .focusable(true)
            .no_focus_ring()
            .autofocus()
            .focus_return(tip.key.clone())
            .on_key(move |s: &mut S, ev, tx| {
                // Return or Space closes it again, as they opened it.
                let again = matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(c) if c == " ");
                if again {
                    *slot(s) = None;
                    tx.instant();
                }
                again
            })
            .modal_tx(true, move |s: &mut S, tx: &mut Tx| {
                *slot(s) = None;
                tx.instant();
            }),
    }
}

#[cfg(test)]
mod tests {
    use rune::{Env, KeyEvent, Mods, Runtime};

    use super::*;

    const WORDS: &str = "tmux keeps a terminal's programs running on the server while you're away.";

    #[derive(Default)]
    struct Page {
        tip: Option<Tip>,
    }

    fn slot(p: &mut Page) -> &mut Option<Tip> {
        &mut p.tip
    }

    fn view(p: &Page, cx: &mut rune::Cx<Page>) -> El {
        let t = Theme::of(cx.env().system_dark);
        let open = p.tip.as_ref().is_some_and(|o| o.key == "about-tmux");
        rune::col()
            .key("root")
            .w_full()
            .h_full()
            .padding(40.0)
            .gap(20.0)
            .child(rune::row().gap(4.0).items_center().child(text("Keep sessions").font(theme::TEXT).color(t.l1)).child(help_mark(t, "about-tmux", "keeping sessions", WORDS, open, slot)))
            .child(rune::el().key("elsewhere").size(80.0, 30.0).focusable(true).role("button").label_static("Elsewhere"))
            .children(p.tip.as_ref().map(|tip| help_card(t, tip, slot)))
    }

    fn page() -> Runtime<Page> {
        crate::fonts::declare();
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(Page::default(), view, (600.0, 400.0), Env::new(true, 1.0), text);
        rt.frame(1.0 / 60.0);
        rt
    }

    fn frames(rt: &mut Runtime<Page>, seconds: f64) {
        for _ in 0..(seconds * 60.0).ceil() as usize {
            rt.frame(1.0 / 60.0);
        }
    }

    fn key(rt: &mut Runtime<Page>, key: Key) {
        rt.key(KeyEvent { key, mods: Mods::default() });
        frames(rt, 0.1);
    }

    fn focused(rt: &Runtime<Page>) -> Option<String> {
        rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key)
    }

    fn card_says(rt: &Runtime<Page>) -> bool {
        rt.find("help-card").is_some() && rt.accessibility_tree().nodes.iter().any(|(_, n)| n.value() == Some(WORDS) || n.label() == Some(WORDS))
    }

    #[test]
    fn the_mark_is_a_named_button_in_the_tab_order_with_its_words_as_description() {
        let mut rt = page();
        let tree = rt.accessibility_tree();
        let mark = tree.nodes.iter().find(|(_, n)| n.label() == Some("About keeping sessions")).map(|(_, n)| n).expect("the ? is named");
        assert_eq!(mark.description(), Some(WORDS));
        // Tab reaches it first, then the button after it.
        key(&mut rt, Key::Tab);
        assert_eq!(focused(&rt).as_deref(), Some("about-tmux"));
        key(&mut rt, Key::Tab);
        assert_eq!(focused(&rt).as_deref(), Some("elsewhere"));
        assert!(rt.state.tip.is_none());
    }

    #[test]
    fn resting_the_pointer_opens_it_and_leaving_closes_it() {
        let mut rt = page();
        let (x, y) = rt.find("about-tmux").unwrap();
        rt.pointer_move(x, y);
        // A pass shorter than the delay opens nothing.
        frames(&mut rt, 0.1);
        assert!(rt.state.tip.is_none());
        frames(&mut rt, 0.4);
        assert_eq!(rt.state.tip.as_ref().map(|t| (t.key.as_str(), t.by)), Some(("about-tmux", Opened::Pointer)));
        assert!(card_says(&rt));
        // Under the "?".
        let (mark, _) = rt.presented("about-tmux").unwrap();
        let (card, _) = rt.presented("help-card").unwrap();
        assert!(card[1] >= mark[1] + mark[3], "{card:?} under {mark:?}");
        rt.pointer_move(500.0, 380.0);
        frames(&mut rt, 0.4);
        assert!(rt.state.tip.is_none() && rt.find("help-card").is_none());
    }

    #[test]
    fn a_click_opens_it_and_another_closes_it() {
        let mut rt = page();
        let (x, y) = rt.find("about-tmux").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        frames(&mut rt, 0.05);
        assert!(card_says(&rt));
        // The pointer resting there afterwards doesn't close it.
        frames(&mut rt, 0.5);
        assert!(rt.state.tip.is_some());
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        frames(&mut rt, 0.3);
        assert!(rt.state.tip.is_none());
        // Opened by a click, it closes on Esc too.
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        frames(&mut rt, 0.05);
        assert!(rt.state.tip.is_some());
        key(&mut rt, Key::Escape);
        assert!(rt.state.tip.is_none());
    }

    #[test]
    fn return_or_space_opens_it_and_esc_closes_it_with_focus_back_on_the_mark() {
        for opener in [Key::Enter, Key::Text(" ".into())] {
            let mut rt = page();
            key(&mut rt, Key::Tab);
            key(&mut rt, opener.clone());
            assert_eq!(rt.state.tip.as_ref().map(|t| t.by), Some(Opened::Keys), "{opener:?}");
            assert!(card_says(&rt));
            key(&mut rt, Key::Escape);
            frames(&mut rt, 0.3);
            assert!(rt.state.tip.is_none() && rt.find("help-card").is_none());
            assert_eq!(focused(&rt).as_deref(), Some("about-tmux"));
            // And again: the same key closes it.
            key(&mut rt, opener.clone());
            assert!(rt.state.tip.is_some());
            key(&mut rt, opener);
            assert!(rt.state.tip.is_none());
        }
    }

    #[test]
    fn opened_from_the_keys_a_click_outside_closes_it() {
        let mut rt = page();
        key(&mut rt, Key::Tab);
        key(&mut rt, Key::Enter);
        assert!(rt.state.tip.is_some());
        rt.pointer_down(560.0, 380.0);
        rt.pointer_up(560.0, 380.0);
        frames(&mut rt, 0.3);
        assert!(rt.state.tip.is_none());
    }
}
