//! The pane's elements: the grid of rows and the cursor over it.
//!
//! ```text
//! term   el, padding 12, focusable, keys, drag, wheel, layout
//! ├── grid(cols, cell_w, cell_h, rows).grid_caret(col, row)
//! ├── ime      transparent, absolute at the anchor's cell, `.ime_cursor()`: while the window
//! │            has the keyboard, never blinking (a flicker would turn the input method off and
//! │            on, Rune #602's trigger)
//! ├── cursor   absolute at its cell: block (a one-cell grid), bar, underline or hollow; blinks
//! │            by poll's beat; left out while composing
//! └── preedit  what the input method is composing, over the anchor's cell: underlined, the
//!              clause being converted thicker, its caret a bar
//! ```
//!
//! While "While you were away" shows, the grid is two (the rows above the line and the rows
//! from it on) with the line between them (`away.rs`), and every cell's place counts its room.
//! The find bar, the scroll bar and the pane's menu are drawn around `term` (`overlays.rs`).

use std::rc::Rc;

use rune::tok::{Family, TextStyle};
use rune::{el, grid, text, Cursor, Cx, El, GridRow, PointerButton, Span};
use sshee_term::engine::CursorShape;

use super::away::Inset;
use super::ime::Preedit;
use super::metrics::{advance_em, natural_height_em, CellMetrics};
use super::palette::{rgb, Palette};
use super::pane::{TermPane, PADDING};
use super::rows::text_style;

/// The bar cursor's width in points (0.10's `cursorWidth: 2`).
const BAR: f32 = 2.0;

/// The cell size at this scale, measured as Rune shapes the terminal's font.
fn metrics_at(pane: &TermPane, scale: f32) -> CellMetrics {
    let o = &pane.options;
    if let Some(m) = pane.metrics.get().filter(|m| m.scale == scale && m.size == o.font_size) {
        return m;
    }
    let style = TextStyle::new(o.font_size, 400.0).family(Family::Named(crate::fonts::TERMINAL_FAMILY)).features(super::rows::FEATURES);
    let measured = rune::text::advance(&"0".repeat(64), style) / 64.0;
    let advance = if measured > 0.0 { measured } else { advance_em(crate::fonts::TERMINAL_REGULAR) * o.font_size };
    let natural = natural_height_em(crate::fonts::TERMINAL_REGULAR) * o.font_size;
    let m = CellMetrics::new(o.font_size, advance, natural, o.line_height, scale, o.cell_mode);
    let refit = pane.metrics.get().is_some_and(|old| old != m);
    pane.metrics.set(Some(m));
    if refit {
        // The window moved to a display of another scale: its size in points is the same, so
        // no layout comes to fit the columns and rows to the new cell, and this view runs after
        // the frame's poll. The next poll fits them (`TermPane::poll`, `check_size`).
        pane.refit_soon();
    }
    m
}

/// The pane, for a window whose state reaches it through `get`.
pub fn pane_view<S: 'static>(pane: &TermPane, cx: &mut Cx<S>, get: fn(&mut S) -> &mut TermPane) -> El {
    pane_view_with(pane, cx, move |s| Some(get(s)), |_, _, _| false)
}

/// The pane, for a window with several: `get` finds this one in the state, or `None` once it
/// is gone (its tab closed between the frame that drew it and a handler it had queued).
/// `before_key` sees each key before the terminal does and returns true to take it: the
/// window's own shortcuts, which must work while the terminal has the keyboard.
pub fn pane_view_with<S: 'static>(
    pane: &TermPane,
    cx: &mut Cx<S>,
    get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static,
    before_key: fn(&mut S, &rune::KeyEvent, &mut rune::Tx) -> bool,
) -> El {
    let env = cx.env();
    let m = metrics_at(pane, env.scale);
    let lit = pane.cursor_lit();
    // The away line's room between two rows, if it shows.
    let inset = pane.inset(&m);
    let mut view = pane.view.borrow_mut();
    if view.palette().dark != env.system_dark {
        // A program asking for the colours (Claude Code picks light or dark by them) is told
        // the scheme it is drawn in.
        pane.model.set_answer_colors(Palette::of(view.palette().scheme, env.system_dark).answer_colors());
    }
    let rows = view.rows(m, env.system_dark, pane.focused);
    let palette = view.palette().clone();
    let size = view.size;
    let cursor = view.cursor;
    let in_view = cursor.visible && cursor.line < size.rows as usize && cursor.col < size.cols;
    let grids = match inset {
        Some(inset) => super::away::split(pane, inset, rows, size.cols, &m, in_view.then_some((cursor.col, cursor.line)), crate::theme::Theme::of(env.system_dark), get),
        None => {
            let mut g = grid(size.cols, m.cell_w, m.cell_h, rows).size(f32::from(size.cols) * m.cell_w, f32::from(size.rows) * m.cell_h);
            if in_view {
                g = g.grid_caret(cursor.col, cursor.line as u16);
            }
            vec![g]
        }
    };
    let mut term = el()
        .key("term")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(PADDING)
        .fill(palette.bg())
        .focusable(true)
        .no_focus_ring()
        .instant_changes()
        .cursor(if pane.hover_secret || pane.over.links.pointing() { Cursor::Pointer } else { Cursor::Text })
        .on_key(move |s: &mut S, ev, tx| before_key(s, ev, tx) || get(s).is_some_and(|p| p.key(ev, tx)))
        .on_physical_input(move |s: &mut S, input, tx| get(s).is_some_and(|p| p.physical(input, tx)))
        .on_drag(move |s: &mut S, d, tx| {
            if let Some(p) = get(s) {
                p.drag(d, tx);
            }
        })
        .pointer_buttons([PointerButton::Middle])
        .on_secondary_press_local(move |s: &mut S, e, tx| {
            if let Some(p) = get(s) {
                p.secondary(&e, tx);
            }
        })
        .on_pointer_move_local(move |s: &mut S, e, tx| {
            if let Some(p) = get(s) {
                p.pointer_moved(&e, tx);
            }
        })
        .on_hover_change(move |s: &mut S, over, tx| {
            // The pointer left: no link is under it.
            if over || !get(s).is_some_and(|p| p.links_left()) {
                tx.live_only();
            }
        })
        .on_wheel(move |s: &mut S, w, tx| get(s).is_some_and(|p| p.wheel(&w, tx)))
        .on_layout_tx(move |s: &mut S, [_, _, w, h], tx| {
            if let Some(p) = get(s) {
                p.laid_out(w, h, tx);
            }
        })
        .children(grids);
    // The input method's anchor: only while the window has the keyboard, and steady (#602).
    // Also while the program hides its cursor (vim, Claude Code): the input method is on only
    // while it's drawn.
    if let (true, Some((line, col))) = (pane.focused, pane.anchor) {
        let (x, y) = cell_xy(&m, inset, line, col);
        term = term.child(el().key("ime").absolute(x, y).size(m.cell_w, m.cell_h).ime_cursor().ime_option_as_alt(cfg!(target_os = "macos")));
    }
    let preedit = pane.ime.preedit().zip(pane.anchor);
    // The cursor blinks by leaving the tree for half of each beat (poll keeps the beat): an
    // element in an `instant_changes` subtree goes and comes back at once.
    if in_view && lit && preedit.is_none() {
        let (text, width, _) = view.shown_char(cursor.line, cursor.col);
        term = term.child(cursor_el(pane, &palette, &m, cursor.shape, cell_xy(&m, inset, cursor.line, cursor.col), text, width));
    }
    if let Some((p, (line, col))) = preedit {
        term = term.child(preedit_el(p, &palette, &m, cell_xy(&m, inset, line, col)));
    }
    // The overlays read the view themselves.
    drop(view);
    super::overlays::layer(term, pane, cx, get, &m)
}

/// The top-left of cell (`line`, `col`) in the terminal area's points, the away line's room
/// counted.
fn cell_xy(m: &CellMetrics, inset: Option<Inset>, line: usize, col: u16) -> (f32, f32) {
    let y = inset.map_or(PADDING + line as f32 * m.cell_h, |i| i.line_y(line, m.cell_h));
    (PADDING + f32::from(col) * m.cell_w, y)
}

/// What the input method is composing, at `(x, y)`: over the terminal's text on its own
/// background, with a thin underline, a thick one under the clause being converted, and a bar
/// at its caret. Measured as the terminal's font shapes it without the cells' tracking (CJK is
/// wider than a cell).
fn preedit_el(p: &Preedit, palette: &Palette, m: &CellMetrics, (x, y): (f32, f32)) -> El {
    let style = text_style(m).tracking(0.0);
    let fg = palette.fg();
    // (An empty line measures as a caret's width, not 0.)
    let at = |byte: usize| if byte == 0 { 0.0 } else { rune::text::advance(p.text.get(..byte).unwrap_or(&p.text), style) };
    let width = at(p.text.len()).max(m.cell_w);
    let mut e = el()
        .key("preedit")
        .absolute(x, y)
        .h(m.cell_h)
        .min_w(width)
        .fill(palette.bg())
        .child(text(p.text.as_str()).key("preedit-text").font(style).color(fg).single_line())
        .child(el().key("preedit-line").absolute(0.0, m.cell_h - 1.0).size(width, 1.0).fill(fg));
    if let Some(target) = p.target() {
        let (a, b) = (at(target.start), at(target.end));
        e = e.child(el().key("preedit-target").absolute(a, m.cell_h - 2.0).size((b - a).max(1.0), 2.0).fill(fg));
    }
    if let Some((a, b)) = p.cursor.filter(|(a, b)| a == b) {
        let _ = b;
        e = e.child(el().key("preedit-caret").absolute(at(a), 0.0).size(1.0, m.cell_h).fill(rgb(palette.cursor)));
    }
    e
}

/// The cursor over the cell at (`x`, `y`), `width` columns wide, showing `text` (the cell as
/// shown) when it is a block.
#[allow(clippy::too_many_arguments)]
fn cursor_el(pane: &TermPane, p: &Palette, m: &CellMetrics, shape: CursorShape, (x, y): (f32, f32), text: String, width: u16) -> El {
    let w = f32::from(width) * m.cell_w;
    let color = rgb(p.cursor);
    let shape = if pane.focused { shape } else { CursorShape::HollowBlock };
    let mut e = match shape {
        CursorShape::Block => {
            let style = text_style(m).tracking(0.0);
            let row = GridRow::new(" ".repeat(width as usize), vec![Span::new(0, 0)], style)
                .cell(0, width, text, style, rgb(p.cursor_accent))
                .fill(0, width, color);
            grid(width, m.cell_w, m.cell_h, vec![Rc::new(row)]).size(w, m.cell_h)
        }
        CursorShape::Bar => el().size(BAR, m.cell_h).fill(color),
        CursorShape::Underline => el().size(w, BAR).fill(color),
        CursorShape::HollowBlock => el().size(w, m.cell_h).border(1.0, color),
    };
    let y = if shape == CursorShape::Underline { y + m.cell_h - BAR } else { y };
    e = e.key("cursor").absolute(x, y);
    e
}

