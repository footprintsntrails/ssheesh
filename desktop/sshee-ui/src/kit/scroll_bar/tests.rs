use super::*;

/// A viewport `viewport` points across and tall, scrolled to `offset` of `max`.
fn at(offset: [f32; 2], max: [f32; 2], viewport: [f32; 2]) -> ScrollPos {
    let mut p = ScrollPos::default();
    p.offset = offset;
    p.max = max;
    p.viewport = viewport;
    p.drawn = viewport;
    p
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn the_thumb_is_the_share_of_the_content_in_view_at_the_place_scrolled_to() {
    // 400 tall over 1600 of content: a quarter of the track, which is the height less its ends.
    let v = at([0.0, 0.0], [0.0, 1200.0], [300.0, 400.0]);
    let (len, shows) = track_len(v, Axis::Y);
    assert!(shows);
    assert_eq!(len, 400.0 - 2.0 * INSET);
    assert_eq!(thumb_along(v, len, Axis::Y), (0.0, len / 4.0));
    // Half way down: half the travel.
    let v = at([0.0, 600.0], [0.0, 1200.0], [300.0, 400.0]);
    let (top, h) = thumb_along(v, len, Axis::Y);
    assert!(close(top, (len - h) / 2.0), "{top}");
    // At the end: against the track's end.
    let v = at([0.0, 1200.0], [0.0, 1200.0], [300.0, 400.0]);
    let (top, h) = thumb_along(v, len, Axis::Y);
    assert!(close(top + h, len));
}

#[test]
fn a_long_list_keeps_a_thumb_long_enough_to_grab() {
    // A virtual list of 100,000 rows of 30 points: its content is estimated whole, so the thumb
    // is the shortest one, and still where the list is.
    let rows = 100_000.0 * 30.0;
    let v = at([0.0, rows / 2.0], [0.0, rows - 500.0], [600.0, 500.0]);
    let (len, _) = track_len(v, Axis::Y);
    let (top, h) = thumb_along(v, len, Axis::Y);
    assert_eq!(h, MIN_THUMB);
    assert!(close(top, (len - MIN_THUMB) * (rows / 2.0) / (rows - 500.0)));
    // A viewport too short for even that shows no bar.
    let v = at([0.0, 0.0], [0.0, 900.0], [600.0, MIN_THUMB]);
    assert!(!track_len(v, Axis::Y).1);
}

#[test]
fn content_that_fits_shows_no_bar_on_that_axis() {
    let v = at([0.0, 0.0], [0.0, 0.0], [300.0, 400.0]);
    assert!(!track_len(v, Axis::Y).1);
    assert!(!track_len(v, Axis::X).1);
    // A vertical list: a vertical bar only.
    let v = at([0.0, 0.0], [0.0, 10.0], [300.0, 400.0]);
    assert!(track_len(v, Axis::Y).1);
    assert!(!track_len(v, Axis::X).1);
}

#[test]
fn a_wide_text_has_a_bar_along_its_bottom_and_the_corner_is_left_to_it() {
    // Text 3 times as wide as the view, and twice as tall.
    let v = at([600.0, 0.0], [1200.0, 400.0], [600.0, 400.0]);
    let (across, shows) = track_len(v, Axis::X);
    assert!(shows);
    assert_eq!(across, 600.0 - 2.0 * INSET);
    let (left, w) = thumb_along(v, across, Axis::X);
    assert!(close(w, across / 3.0));
    assert!(close(left, (across - w) / 2.0));
    // The vertical bar stops short of the horizontal one's strip.
    let (down, _) = track_len(v, Axis::Y);
    assert_eq!(down, 400.0 - 2.0 * INSET - STRIP);
    // Where each bar sits: down the right edge, along the bottom.
    assert_eq!(track_rect(v, Axis::Y, down), [600.0 - STRIP, INSET, STRIP, down]);
    assert_eq!(track_rect(v, Axis::X, across), [INSET, 400.0 - STRIP, across, STRIP]);
    // The thumb against the edge, at rest and wider under the pointer.
    assert_eq!(thumb_rect(Axis::Y, THUMB, 40.0), [THUMB_HOT - THUMB, 0.0, THUMB, 40.0]);
    assert_eq!(thumb_rect(Axis::X, THUMB_HOT, 40.0), [0.0, 0.0, 40.0, THUMB_HOT]);
    assert_eq!(slot_rect(Axis::Y, 10.0, 40.0)[0] + THUMB_HOT + EDGE, STRIP);
}

#[test]
fn dragging_the_thumb_to_a_place_scrolls_there_and_no_further() {
    let v = at([0.0, 0.0], [0.0, 1200.0], [300.0, 400.0]);
    let (len, _) = track_len(v, Axis::Y);
    let (_, h) = thumb_along(v, len, Axis::Y);
    let travel = len - h;
    assert!(close(offset_along(v, len, travel / 2.0, Axis::Y), 600.0));
    assert_eq!(offset_along(v, len, -50.0, Axis::Y), 0.0);
    assert_eq!(offset_along(v, len, travel + 50.0, Axis::Y), 1200.0);
    // And back: the offset puts the thumb where it was dragged.
    let to = at([0.0, offset_along(v, len, 77.0, Axis::Y)], [0.0, 1200.0], [300.0, 400.0]);
    assert!(close(thumb_along(to, len, Axis::Y).0, 77.0));
    // Sideways the same.
    let v = at([0.0, 0.0], [900.0, 0.0], [300.0, 400.0]);
    let (len, _) = track_len(v, Axis::X);
    let (_, w) = thumb_along(v, len, Axis::X);
    assert!(close(offset_along(v, len, len - w, Axis::X), 900.0));
}

#[test]
fn a_grip_lets_go_when_its_release_was_lost() {
    let g = Grip::new(Press::Thumb { grab: 3.0 }, [0.0, 0.0]);
    // The area hears a move before the drag does: normal.
    let g = g.pointer([1.0, 0.0]).expect("one move the drag hasn't heard yet");
    let g = g.moved([1.0, 0.0]);
    assert_eq!(g.pointer([1.0, 0.0]), Some(g), "the drag heard it");
    // Two moves the drag never hears: it's gone.
    let g = g.pointer([2.0, 0.0]).unwrap();
    assert_eq!(g.pointer([3.0, 0.0]), None);
}

#[test]
fn an_overlay_bar_shows_while_scrolled_or_held_then_fades() {
    use visibility::{step, BarState, Effect, Event, Follow, Opacity};
    let follow = |scrolled, revealed| Follow {
        shows: true,
        overlay: true,
        scrolled,
        instant: false,
        resized: false,
        lengthened: false,
        extent_changed: false,
        pressed: false,
        revealed,
        now: 1.0,
        hold: HOLD,
    };
    let opacity = |e: rune::machine::Effects<Effect>| e.into_iter().find_map(|e| if let Effect::Opacity(o) = e { Some(o) } else { None });
    let mut s = BarState::Uninit;
    assert_eq!(opacity(step(&mut s, Event::Follow(follow(false, false)))), Some(Opacity::Snap(0.0)), "hidden at rest");
    assert_eq!(opacity(step(&mut s, Event::Follow(follow(true, false)))), Some(Opacity::Pulse { hold: HOLD }), "a scroll shows it");
    assert_eq!(opacity(step(&mut s, Event::Hold)), Some(Opacity::Set(1.0)), "the pointer over the area holds it");
    let let_go = Event::LetGo { pressed: false, revealed: false, now: 2.0, hold: HOLD };
    assert_eq!(opacity(step(&mut s, let_go)), Some(Opacity::Pulse { hold: HOLD }), "and it fades a second after");
    // Always shown when the system says so.
    let mut s = BarState::Uninit;
    let always = Follow { overlay: false, ..follow(false, false) };
    assert_eq!(opacity(step(&mut s, Event::Follow(always))), Some(Opacity::Snap(1.0)));
}

/// A window with one long list in it, as the app's are: 300 rows of 30 points, laid out only
/// where they show (`virtual_rows`), in a viewport 400 points tall.
mod window {
    use rune::{col, row, text, Cx, El, Env, GrowthSpec, Runtime, ScrollSpec};

    use super::super::{ScrollBars, EDGE, INSET, MIN_THUMB};
    use crate::theme::Theme;

    struct Long;

    const ROWS: usize = 300;
    const ROW: f32 = 30.0;

    fn view(_: &Long, _: &mut Cx<Long>) -> El {
        let t = Theme::of(true);
        let list = col()
            .key("long")
            .w(500.0)
            .h(400.0)
            .scroll(ScrollSpec { stick_bottom: false })
            .child(col().key("long-rows").w_full().growth(GrowthSpec::default()).virtual_rows(ROWS, ROW, |_: &Long, i| format!("row-{i}").into(), move |_: &Long, i| {
                row().h(ROW).px(10.0).child(text(format!("Row {i}")).font(crate::theme::TEXT).color(t.l1).single_line())
            }))
            .scroll_bars(t);
        col().key("root").w_full().h_full().fill(t.page).child(list)
    }

    fn open() -> Runtime<Long> {
        crate::fonts::declare();
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(Long, view, (800.0, 600.0), Env::new(true, 1.0), text);
        settle(&mut rt);
        rt
    }

    fn settle<S: 'static>(rt: &mut Runtime<S>) {
        for _ in 0..6 {
            rt.frame(1.0 / 60.0);
        }
        for _ in 0..8 {
            rt.frame(0.25);
        }
    }

    /// The first row on screen.
    fn first_row(rt: &Runtime<Long>) -> usize {
        let top = rt.presented("long").unwrap().0[1];
        (0..ROWS).find(|i| rt.presented(&format!("row-{i}")).is_some_and(|(r, _)| r[1] + r[3] > top + 0.5)).unwrap()
    }

    fn thumb<S: 'static>(rt: &Runtime<S>) -> ([f32; 4], f32) {
        rt.presented("thumb").expect("no thumb")
    }

    #[test]
    fn a_long_list_shows_a_thumb_under_the_pointer_and_dragging_it_scrolls() {
        let mut rt = open();
        let [x, y, w, h] = rt.presented("long").unwrap().0;
        assert_eq!(first_row(&rt), 0);
        // At rest, out of the way: nothing drawn, nothing to press.
        assert!(thumb(&rt).1 < 0.01, "the bar shows at rest");
        // The pointer over the list brings it up: a thumb as long as the share in view, against
        // the right edge, at the top.
        rt.pointer_move(x + w / 2.0, y + h / 2.0);
        settle(&mut rt);
        let (r, opacity) = thumb(&rt);
        assert!(opacity > 0.99, "no bar under the pointer: {opacity}");
        let track = h - 2.0 * INSET;
        let content = ROWS as f32 * ROW;
        assert!((r[3] - (track * h / content).max(MIN_THUMB)).abs() < 1.0, "thumb {r:?}");
        assert!((r[0] + r[2] - (x + w - EDGE)).abs() < 0.5, "the thumb isn't against the right edge: {r:?}");
        assert!((r[1] - (y + INSET)).abs() < 0.5, "not at the top: {r:?}");
        // Under the pointer the thumb widens, and keeps its length and place.
        let (gx, gy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        rt.pointer_move(gx, gy);
        settle(&mut rt);
        let hot = thumb(&rt).0;
        assert_eq!(hot[2], super::super::THUMB_HOT, "the thumb didn't widen: {hot:?}");
        assert!((hot[3] - r[3]).abs() < 0.5 && (hot[1] - r[1]).abs() < 0.5, "the thumb changed length under the pointer: {hot:?}, was {r:?}");
        let r = hot;
        // Drag it half way down the track: the list is half way down.
        rt.pointer_down(gx, gy);
        rt.frame(1.0 / 60.0);
        let travel = track - r[3];
        for step in 1..=10 {
            rt.pointer_move(gx, gy + travel / 2.0 * step as f32 / 10.0);
            rt.frame(1.0 / 60.0);
        }
        rt.pointer_up(gx, gy + travel / 2.0);
        settle(&mut rt);
        let half = ((content - h) / 2.0 / ROW) as usize;
        let first = first_row(&rt);
        assert!(first.abs_diff(half) <= 1, "dragged half way, the list shows row {first}, not {half}");
        // The thumb went with it.
        let r2 = thumb(&rt).0;
        assert!((r2[1] - (r[1] + travel / 2.0)).abs() < 1.5, "the thumb is at {r2:?}");
        // A press on the track under the thumb pages down.
        let below = y + h - INSET - 2.0;
        rt.pointer_move(gx, below);
        rt.frame(1.0 / 60.0);
        rt.pointer_down(gx, below);
        rt.frame(1.0 / 60.0);
        rt.pointer_up(gx, below);
        settle(&mut rt);
        let paged = first_row(&rt);
        assert!(paged > first + 5, "a press below the thumb didn't page: row {paged}");
        // The pointer gone, the bar fades.
        rt.pointer_move(x + w + 100.0, y + h / 2.0);
        settle(&mut rt);
        assert!(thumb(&rt).1 < 0.01, "the bar stays after the pointer left");
    }

    #[test]
    fn a_list_that_fits_has_no_bar() {
        struct Short;
        fn view(_: &Short, _: &mut Cx<Short>) -> El {
            let t = Theme::of(false);
            let list = col().key("short").w(500.0).h(400.0).scroll(ScrollSpec { stick_bottom: false }).child(text("One line").font(crate::theme::TEXT).color(t.l1)).scroll_bars(t);
            col().w_full().h_full().child(list)
        }
        crate::fonts::declare();
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(Short, view, (800.0, 600.0), Env::new(false, 1.0), text);
        settle(&mut rt);
        let [x, y, ..] = rt.presented("short").unwrap().0;
        rt.pointer_move(x + 250.0, y + 200.0);
        settle(&mut rt);
        let (r, o) = thumb(&rt);
        assert_eq!(r[2] * r[3] * o, 0.0, "a bar on a list that fits");
        assert!(!rt.elements().iter().any(|e| e.role == "scrollbar"), "a scroll bar for assistive technology on a list that fits");
    }
}
