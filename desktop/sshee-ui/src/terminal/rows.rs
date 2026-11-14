//! The terminal's rows as Rune `GridRow`s.
//!
//! Every row is built from what the presentation pass may show (`PresentedRow`), never from the
//! engine's text alone: a hidden secret is dots here, so it is never in a row's line, a pinned
//! cell, the glyph atlas, the GPU or the accessibility tree. Colours, styles and widths come
//! from the engine's cells by column.
//!
//! A row's line holds exactly one character per column, so the grid's columns stay put
//! whatever the font does: characters the bundled font lacks, wide ones and ones with
//! combining marks are spaces in the line and drawn as pinned cells; box drawing is a space and
//! drawn as shapes (`glyphs`).
//!
//! Built rows are kept by a hash of everything they depend on, so a row that only moved (the
//! screen scrolled) comes back as the same `Rc` and Rune keeps its shaped layout. Only rows
//! that can come back that way are kept: the screen's and the screen's before it. A kept row
//! holds its shaped layout and drawn instances (about 21 KB at 133 columns), so keeping every
//! row a flood ever built would hold tens of megabytes per pane past Rune's own bound on stale
//! layouts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use rune::tok::{Family, TextStyle};
use rune::{Color, GridRow, LineCap, Shape, Span};
use sshee_term::engine::{row_texts, CellFlags, Cursor, Modes, Row, Size};
use sshee_term::model::{EdgeRows, Frame};
use sshee_term::present::{Edges, FindMarks, Look, PresentedRow, Presenter, Role, RowText, Screen};
use sshee_term::scroll::ScrollView;
use sshee_term::select::SelectionSpan;

use super::coverage::Coverage;
use super::find::FindMark;
use super::glyphs::{self, Op};
use super::metrics::CellMetrics;
use super::palette::{mix_srgb, Palette};

/// The dot a hidden character shows as (the presentation pass's).
pub const DOT: char = '•';

/// How far a hidden secret's pill is tinted with the accent.
pub const PILL_ALPHA: f32 = 0.16;

/// How far a find match's fill goes from the terminal's background to its text colour. The
/// match the find bar is on is drawn the other way round (the text's colour behind, the
/// background's in front). Neither is blue (that's the selection) and both keep the text well
/// above 4.5:1 in either appearance (`find_marks_keep_the_text_readable`).
pub const FOUND_MIX: f32 = 0.22;

/// A find match's fill and its text's colour: `current` for the one the bar is on.
pub fn found_colors(p: &Palette, current: bool) -> (Color, Color) {
    if current {
        (p.fg(), p.bg())
    } else {
        (mix_srgb(p.bg(), p.fg(), FOUND_MIX), p.fg())
    }
}

/// No ligatures or contextual alternates: `--` and `->` stay two characters, each on its cell
/// (Rune turns them off only for its own mono slot, and this is a named family).
pub const FEATURES: &str = "'calt' 0, 'liga' 0, 'clig' 0, 'dlig' 0";

/// The terminal's text style for these cells.
pub fn text_style(m: &CellMetrics) -> TextStyle {
    TextStyle::new(m.size, 400.0).family(Family::Named(crate::fonts::TERMINAL_FAMILY)).features(FEATURES).lh(m.cell_h).tracking(m.tracking)
}

/// A run of the line with one look, by byte offsets into the line.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanPlan {
    pub start: usize,
    pub end: usize,
    /// `None`: the line's own colour (the scheme's text colour).
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<Color>,
    /// A hidden secret's pill behind the run.
    pub pill: Option<Color>,
}

/// A character drawn on its own at its column.
#[derive(Clone, Debug, PartialEq)]
pub struct CellPlan {
    pub col: u16,
    pub width: u16,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
}

/// One row, worked out before it becomes a `GridRow` (what the tests read).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowPlan {
    pub text: String,
    pub spans: Vec<SpanPlan>,
    pub cells: Vec<CellPlan>,
    /// Background runs, then the selection: `(start, end, colour)` in columns.
    pub fills: Vec<(u16, u16, Color)>,
    /// Shapes over the text, in device pixels from the row's left edge: box drawing, double
    /// underlines, strikethrough.
    pub ops: Vec<(i32, Op, Color)>,
    pub wraps: bool,
}

/// What a row's look depends on besides its cells.
pub struct RowLook<'a> {
    pub palette: &'a Palette,
    pub metrics: &'a CellMetrics,
    /// The selection's colour (active or not).
    pub selection: Color,
}

#[derive(Clone, Copy, PartialEq)]
struct RunKey {
    color: Option<Color>,
    bold: bool,
    italic: bool,
    underline: Option<Color>,
    secret: bool,
}

/// A link's underline on a row: its columns, and whether it is dotted (a file's path).
pub type Underline = (Range<u16>, bool);

/// A link's cells by view line: a long link runs over several rows.
pub type LinkCells = Vec<(usize, Range<u16>)>;

/// Works out row `row` as `shown` (its presented text) says, with the selection over `sel` and
/// the link under the pointer underlined over `under`.
pub fn plan_row(row: &Row, shown: &PresentedRow, sel: Option<Range<u16>>, under: Option<&Underline>, look: &RowLook, coverage: &mut Coverage) -> RowPlan {
    let cols = row.cells.len();
    let p = look.palette;
    let (cw, ch) = (look.metrics.cell_w_px as i32, look.metrics.cell_h_px as i32);
    // The presented characters of each column, as byte ranges of `shown.text`.
    let mut at: Vec<Option<(usize, usize)>> = vec![None; cols];
    for ((i, c), &col) in shown.text.char_indices().zip(&shown.cols) {
        if let Some(slot) = at.get_mut(col as usize) {
            let end = i + c.len_utf8();
            *slot = Some(slot.map_or((i, end), |(s, _)| (s, end)));
        }
    }
    let mut hidden = vec![false; cols];
    // Find matches by column: `Some(true)` for the one the find bar is on.
    let mut found: Vec<Option<bool>> = vec![None; cols];
    for span in &shown.spans {
        if matches!(span.role, Role::Secret { .. } | Role::Masked) {
            for c in span.cols.clone() {
                if let Some(h) = hidden.get_mut(c as usize) {
                    *h = true;
                }
            }
        }
        if let Role::Found { current } = span.role {
            for c in span.cols.clone() {
                if let Some(f) = found.get_mut(c as usize) {
                    *f = Some(f.unwrap_or(false) || current);
                }
            }
        }
    }
    let mut plan = RowPlan { wraps: row.wraps, ..RowPlan::default() };
    let mut run: Option<(usize, RunKey)> = None;
    let mut covered = 0usize;
    for (c, cell) in row.cells.iter().enumerate() {
        let flags = cell.flags;
        let (fg, bg) = if flags.contains(CellFlags::INVERSE) { (cell.bg, cell.fg) } else { (cell.fg, cell.bg) };
        let mut fgc = p.color(fg);
        let mut bgc = p.color(bg);
        if flags.contains(CellFlags::DIM) {
            fgc = mix_srgb(fgc, bgc, 0.5);
        }
        if let Some(current) = found[c] {
            (bgc, fgc) = found_colors(p, current);
        }
        if bgc != p.bg() {
            match plan.fills.last_mut() {
                Some(f) if f.1 == c as u16 && f.2 == bgc => f.1 += 1,
                _ => plan.fills.push((c as u16, c as u16 + 1, bgc)),
            }
        }
        if covered > c {
            continue;
        }
        let width = (cell.width as usize).clamp(1, 2).min(cols - c);
        let secret = hidden[c];
        let text = at[c].map_or("", |(s, e)| &shown.text[s..e]);
        let first = text.chars().next().unwrap_or(' ');
        let linked = under.filter(|(cols, _)| cols.contains(&(c as u16))).map(|(_, dotted)| *dotted);
        let underline = flags.intersects(CellFlags::UNDERLINE | CellFlags::UNDERCURL | CellFlags::DOTTED_UNDERLINE | CellFlags::DASHED_UNDERLINE)
            || linked == Some(false);
        let key = RunKey {
            // The default colour is written out too: a row has no colour of its own, and text
            // without one would take the window's default instead of the scheme's.
            color: Some(if secret { p.accent } else { fgc }),
            bold: flags.contains(CellFlags::BOLD),
            italic: flags.contains(CellFlags::ITALIC),
            underline: (underline && !secret).then_some(fgc),
            secret,
        };
        if run.as_ref().is_none_or(|r| r.1 != key) {
            close_run(&mut plan, run.take());
            run = Some((plan.text.len(), key));
        }
        let x = c as i32 * cw;
        if cell.width == 0 {
            // A blank left where a wide character didn't fit: holds its column.
            plan.text.push(' ');
        } else if flags.contains(CellFlags::HIDDEN) && !secret {
            (0..width).for_each(|_| plan.text.push(' '));
        } else if secret {
            plan.text.push(DOT);
            (1..width).for_each(|_| plan.text.push(' '));
        } else if glyphs::is_custom(first) && text.chars().nth(1).is_none() {
            plan.text.push(' ');
            plan.ops.extend(glyphs::ops(first, cw, ch).into_iter().map(|op| (x, op, fgc)));
        } else if width == 2 || text.chars().nth(1).is_some() || !coverage.has(first) {
            (0..width).for_each(|_| plan.text.push(' '));
            plan.cells.push(CellPlan {
                col: c as u16,
                width: width as u16,
                text: if text.is_empty() { " ".into() } else { text.to_string() },
                bold: key.bold,
                italic: key.italic,
                color: fgc,
            });
        } else {
            plan.text.push(first);
        }
        // Lines the line's spans don't draw.
        let t = glyphs::light_px(cw);
        if linked == Some(true) && !secret {
            // A path's dotted underline: a dot and two gaps, in step along the whole row.
            let period = 3 * t;
            let first = (period - x.rem_euclid(period)) % period;
            for dot in (first..cw * width as i32).step_by(period as usize) {
                plan.ops.push((x, Op::Rect { x: dot, y: ch - 2 * t, w: t, h: t, alpha: 1.0 }, fgc));
            }
        }
        if flags.contains(CellFlags::DOUBLE_UNDERLINE) && !secret {
            let y = ch - 3 * t;
            for yy in [y, y + 2 * t] {
                plan.ops.push((x, Op::Rect { x: 0, y: yy, w: cw * width as i32, h: t, alpha: 1.0 }, fgc));
            }
        }
        if flags.contains(CellFlags::STRIKEOUT) && !secret {
            let y = (ch - t).div_euclid(2);
            plan.ops.push((x, Op::Rect { x: 0, y, w: cw * width as i32, h: t, alpha: 1.0 }, fgc));
        }
        covered = c + width;
    }
    close_run(&mut plan, run);
    if let Some(sel) = sel.filter(|s| s.end > s.start) {
        plan.fills.push((sel.start, sel.end.min(cols as u16), look.selection));
    }
    plan
}

fn close_run(plan: &mut RowPlan, run: Option<(usize, RunKey)>) {
    let Some((start, key)) = run else { return };
    let end = plan.text.len();
    if end == start || (key.color.is_none() && !key.bold && !key.italic && key.underline.is_none() && !key.secret) {
        return;
    }
    plan.spans.push(SpanPlan {
        start,
        end,
        color: key.color,
        bold: key.bold,
        italic: key.italic,
        underline: key.underline,
        pill: key.secret.then(|| key.color.unwrap_or(Color::WHITE).alpha(PILL_ALPHA)),
    });
}

impl RowPlan {
    /// The Rune row, in the terminal's style, at these metrics.
    pub fn into_grid_row(self, m: &CellMetrics) -> GridRow {
        let style = text_style(m);
        let spans = self
            .spans
            .iter()
            .map(|s| {
                let mut span = Span::new(s.start, s.end);
                if let Some(c) = s.color {
                    span = span.color(c);
                }
                if s.bold {
                    span = span.weight(700.0);
                }
                if s.italic {
                    span = span.italic();
                }
                if let Some(c) = s.underline {
                    span = span.underline(c);
                }
                if let Some(c) = s.pill {
                    span = span.bg(c, 0.0);
                }
                span
            })
            .collect();
        let mut row = GridRow::new(self.text, spans, style);
        for c in self.cells {
            let mut cell_style = style.tracking(0.0);
            if c.bold {
                cell_style.weight = 700.0;
            }
            if c.italic {
                cell_style = cell_style.italic();
            }
            row = row.cell(c.col, c.width, c.text, cell_style, c.color);
        }
        for (a, b, color) in self.fills {
            row = row.fill(a, b, color);
        }
        if !self.ops.is_empty() {
            let s = m.scale;
            let mut shape = Shape::new();
            for (x0, op, color) in self.ops {
                let pt = |x: f32, y: f32| [(x0 as f32 + x) / s, y / s];
                shape = match op {
                    Op::Rect { x, y, w, h, alpha } => {
                        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
                        shape.fill(vec![pt(x, y), pt(x + w, y), pt(x + w, y + h), pt(x, y + h)], color.alpha(color.a * alpha))
                    }
                    Op::Poly(points) => shape.fill(points.into_iter().map(|[x, y]| pt(x, y)).collect::<Vec<_>>(), color),
                    Op::Stroke { points, width } => {
                        let cap = if points.len() > 3 { LineCap::Round } else { LineCap::Butt };
                        shape.stroke_with(points.into_iter().map(|[x, y]| pt(x, y)).collect::<Vec<_>>(), false, width / s, cap, color)
                    }
                };
            }
            row = row.shape(shape);
        }
        row.wraps(self.wraps)
    }
}

/// How many rows a view run built and how many it handed back unchanged (for the meter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowStats {
    pub built: u64,
    pub reused: u64,
}

/// What a line was built from last time: the same again keeps its `Rc` without hashing.
#[derive(Clone, PartialEq)]
struct LineInputs {
    row: u64,
    shown: *const PresentedRow,
    sel: Option<Range<u16>>,
    under: Option<Underline>,
}

/// The pane's view of its terminal: the rows of the last frame, what the presentation pass
/// made of them, and the `GridRow`s built from that. Updated by poll (frames) and handlers
/// (reveal), read by the view, all on the UI thread.
pub struct PaneView {
    pub size: Size,
    mirror: Vec<Row>,
    hashes: Vec<u64>,
    edges: EdgeRows,
    presenter: Presenter,
    shown: Vec<Arc<PresentedRow>>,
    stale: bool,
    built: Vec<Rc<GridRow>>,
    inputs: Vec<Option<LineInputs>>,
    /// Each line's key (see `rows`), beside `built`.
    keys: Vec<u64>,
    /// The rows a scroll can bring back: the screen's before the last change, by key.
    recent: HashMap<u64, Rc<GridRow>>,
    epoch: u64,
    look: Option<(CellMetrics, bool, bool)>,
    palette: Palette,
    coverage: Coverage,
    pub cursor: Cursor,
    pub modes: Modes,
    pub display_offset: usize,
    pub selection: Option<SelectionSpan>,
    pub stats: RowStats,
    /// What the scroll bar shows, as of the last frame.
    pub scroll: ScrollView,
    /// What the find bar looks for and the match it is on (`set_find`).
    find: Option<FindMark>,
    /// The link under the pointer: its cells by view line, and whether its underline is dotted.
    underline: Option<(LinkCells, bool)>,
}

fn hash_of(v: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl PaneView {
    pub fn new(size: Size, look: Look) -> Self {
        PaneView {
            size,
            mirror: vec![Row::blank(size.cols); size.rows as usize],
            hashes: vec![0; size.rows as usize],
            edges: EdgeRows::default(),
            presenter: Presenter::new(look),
            shown: Vec::new(),
            stale: true,
            built: Vec::new(),
            inputs: Vec::new(),
            keys: Vec::new(),
            recent: HashMap::new(),
            epoch: 0,
            look: None,
            palette: Palette::new(true),
            coverage: Coverage::new(crate::fonts::TERMINAL_REGULAR),
            cursor: Cursor::default(),
            modes: Modes::default(),
            display_offset: 0,
            selection: None,
            stats: RowStats::default(),
            scroll: ScrollView::default(),
            find: None,
            underline: None,
        }
    }

    /// Takes in a frame from the model: its changed rows, and everything around them.
    pub fn intake(&mut self, frame: Frame) {
        let rows = frame.size.rows as usize;
        if frame.size != self.size || self.mirror.len() != rows {
            self.size = frame.size;
            self.mirror = vec![Row::blank(frame.size.cols); rows];
            self.hashes = vec![0; rows];
        }
        for r in frame.rows {
            if r.line < rows {
                self.hashes[r.line] = hash_of(&r.row);
                self.mirror[r.line] = r.row;
            }
        }
        self.edges = frame.edges;
        self.cursor = frame.cursor;
        self.modes = frame.modes;
        self.display_offset = frame.display_offset;
        self.selection = frame.selection;
        self.scroll = frame.scroll;
        self.stale = true;
    }

    /// What the find bar looks for, to mark on screen (`None`: nothing).
    pub fn set_find(&mut self, find: Option<FindMark>) {
        if self.find != find {
            self.find = find;
            self.stale = true;
        }
    }

    /// The link under the pointer, to underline: its cells by view line, dotted for a path.
    pub fn set_underline(&mut self, underline: Option<(LinkCells, bool)>) {
        self.underline = underline;
    }

    /// New settings for the presentation pass (streamer mode): what is on screen is worked
    /// out again under them, at once.
    pub fn set_look(&mut self, look: Look) {
        self.presenter.set_look(look);
        self.stale = true;
    }

    /// The view line the find bar's match starts on, when it is in view: its place is kept
    /// from the history's bottom, and the view's own is in the scroll position.
    fn current_match(&self) -> Option<(usize, usize)> {
        let (from_bottom, start) = self.find.as_ref()?.current?;
        let v = self.scroll;
        let bottom = v.total.saturating_sub(v.top + v.rows);
        let line = (bottom + self.mirror.len()).checked_sub(from_bottom + 1)?;
        (line < self.mirror.len()).then_some((line, start))
    }

    /// The rows the presentation pass reads: the edges' and the view's.
    fn texts(&self) -> (Vec<RowText>, Vec<RowText>, Vec<RowText>) {
        let above = row_texts(&self.edges.above, false);
        let view = row_texts(&self.mirror, self.edges.above.last().is_some_and(|r| r.wraps));
        let below = row_texts(&self.edges.below, self.mirror.last().is_some_and(|r| r.wraps));
        (above, view, below)
    }

    fn screen<'a>(&self, texts: &'a (Vec<RowText>, Vec<RowText>, Vec<RowText>)) -> Screen<'a> {
        Screen {
            rows: &texts.1,
            cols: self.size.cols,
            edges: Edges { above: &texts.0, below: &texts.2, more_above: self.edges.more_above, more_below: self.edges.more_below },
            key_open_above: self.edges.key_open_above,
            own_screen: &[],
            find: None,
        }
    }

    /// The presented rows, worked out again when a frame or a reveal changed them.
    fn present(&mut self) {
        if !self.stale && self.shown.len() == self.mirror.len() {
            return;
        }
        let texts = self.texts();
        let mut screen = self.screen(&texts);
        // (The query is copied out: the pass borrows the presenter, and with it the view.)
        let find = self.find.as_ref().map(|f| f.query.clone()).zip(Some(self.current_match()));
        screen.find = find.as_ref().map(|(query, current)| FindMarks { query, current: *current });
        self.shown = self.presenter.present(&screen);
        self.stale = false;
    }

    /// The presented row at view line `line`, if any (for hit tests).
    pub fn shown(&mut self, line: usize) -> Option<Arc<PresentedRow>> {
        self.present();
        self.shown.get(line).cloned()
    }

    /// Whether a click at (`line`, `col`) lands on a hidden secret a click may show.
    pub fn revealable_at(&mut self, line: usize, col: u16) -> bool {
        self.shown(line).is_some_and(|r| r.span_at(col, |role| matches!(role, Role::Secret { revealable: true })).is_some())
    }

    /// Shows the secret at (`line`, `col`) from now on, if a click may; true when it did.
    pub fn reveal(&mut self, line: usize, col: u16) -> bool {
        let texts = self.texts();
        let screen = self.screen(&texts);
        let shown = self.presenter.reveal(&screen, line, col);
        if shown {
            self.stale = true;
        }
        shown
    }

    /// What copying goes through: the presentation pass's dots, never the secret.
    pub fn look(&self) -> &Look {
        self.presenter.look()
    }

    pub fn key_open_above(&self) -> bool {
        self.edges.key_open_above
    }

    /// The character at a view cell as shown (a dot where hidden, a space where the program
    /// concealed it with SGR 8, as `plan_row` draws them), for the block cursor.
    pub fn shown_char(&mut self, line: usize, col: u16) -> (String, u16, bool) {
        let cell = self.mirror.get(line).and_then(|r| r.cells.get(col as usize));
        let width = cell.map_or(1, |c| c.width.max(1) as u16);
        let concealed = cell.is_some_and(|c| c.flags.contains(CellFlags::HIDDEN));
        let Some(row) = self.shown(line) else { return (" ".into(), 1, false) };
        let hidden = row.span_at(col, |r| matches!(r, Role::Secret { .. } | Role::Masked)).is_some();
        if concealed && !hidden {
            return (" ".into(), width, false);
        }
        let text: String = row.text.chars().zip(&row.cols).filter(|(_, &c)| c == col).map(|(ch, _)| ch).collect();
        let text = if text.is_empty() { " ".to_string() } else { text };
        let custom = text.chars().next().is_some_and(glyphs::is_custom);
        (if custom { " ".into() } else { text }, width, hidden)
    }

    /// Characters met so far that the terminal's font lacks (each one's first meeting goes to
    /// a fallback font).
    pub fn fallbacks(&self) -> u64 {
        self.coverage.fallbacks
    }

    /// The palette rows are built with.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Draws in `scheme` from the next rows on.
    pub fn set_scheme(&mut self, scheme: super::palette::Scheme) {
        if self.palette.scheme != scheme {
            self.palette = Palette::of(scheme, self.palette.dark);
            self.look = None;
        }
    }

    /// The rows to draw, at these metrics and in this scheme; `focused` picks the selection's
    /// colour.
    pub fn rows(&mut self, metrics: CellMetrics, dark: bool, focused: bool) -> Vec<Rc<GridRow>> {
        if self.look != Some((metrics, dark, focused)) {
            if self.palette.dark != dark {
                self.palette = Palette::of(self.palette.scheme, dark);
            }
            self.look = Some((metrics, dark, focused));
            self.epoch += 1;
            self.recent.clear();
            self.inputs.clear();
            self.keys.clear();
        }
        self.present();
        let n = self.mirror.len();
        self.built.resize_with(n, || Rc::new(GridRow::new("", Vec::new(), text_style(&metrics))));
        self.inputs.resize(n, None);
        self.keys.resize(n, 0);
        // The screen as it was before this run, while it's being replaced: a row that moved
        // comes from here.
        let mut before: Option<HashMap<u64, Rc<GridRow>>> = None;
        let look = RowLook {
            palette: &self.palette,
            metrics: &metrics,
            selection: if focused { self.palette.selection } else { self.palette.selection_inactive },
        };
        let cols = self.size.cols;
        for line in 0..n {
            let shown = &self.shown[line];
            let sel = self.selection.and_then(|s| s.columns_on(line as i32 - self.display_offset as i32, cols));
            let under = self.underline.as_ref().and_then(|(parts, dotted)| parts.iter().find(|(l, _)| *l == line).map(|(_, cols)| (cols.clone(), *dotted)));
            let inputs = LineInputs { row: self.hashes[line], shown: Arc::as_ptr(shown), sel: sel.clone(), under: under.clone() };
            if self.inputs[line].as_ref() == Some(&inputs) {
                continue;
            }
            let key = hash_of(&(self.epoch, self.hashes[line], &**shown, &sel, &under));
            let before = before.get_or_insert_with(|| {
                let mut screen = HashMap::with_capacity(n);
                for (line, (k, row)) in self.keys.iter().zip(&self.built).enumerate() {
                    if self.inputs.get(line).is_some_and(Option::is_some) {
                        screen.insert(*k, row.clone());
                    }
                }
                screen
            });
            let row = match before.get(&key).or_else(|| self.recent.get(&key)) {
                Some(row) => {
                    self.stats.reused += 1;
                    row.clone()
                }
                None => {
                    self.stats.built += 1;
                    let plan = plan_row(&self.mirror[line], shown, sel, under.as_ref(), &look, &mut self.coverage);
                    Rc::new(plan.into_grid_row(&metrics))
                }
            };
            self.built[line] = row;
            self.keys[line] = key;
            self.inputs[line] = Some(inputs);
        }
        // Something changed: what was on screen before this run is what the next can bring back
        // (a scroll by the wheel there and back again), besides what is on screen now.
        if let Some(before) = before {
            self.recent = before;
        }
        self.built.clone()
    }

    /// Built rows kept for reuse beyond those on screen (for tests and the meter).
    pub fn kept_rows(&self) -> usize {
        self.recent.len()
    }
}

/// The pane's view, shared by poll and handlers (which write) and the view (which builds).
pub type SharedView = Rc<RefCell<PaneView>>;

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_term::engine::{Cell, Color as TermColor};
    use sshee_term::present::RowText;

    fn metrics() -> CellMetrics {
        CellMetrics::new(13.0, 7.8, 17.16, 1.22, 1.0, super::super::metrics::CellMode::Floor)
    }

    fn present(rows: &[Row]) -> Vec<Arc<PresentedRow>> {
        let texts: Vec<RowText> = row_texts(rows, false);
        let mut p = Presenter::new(Look { hide_secrets: true, ..Look::default() });
        p.present(&Screen::new(&texts, rows[0].cells.len() as u16))
    }

    fn plan(row: &Row, sel: Option<Range<u16>>) -> RowPlan {
        let palette = Palette::new(true);
        let m = metrics();
        let look = RowLook { palette: &palette, metrics: &m, selection: palette.selection };
        let shown = present(std::slice::from_ref(row));
        plan_row(row, &shown[0], sel, None, &look, &mut Coverage::new(crate::fonts::TERMINAL_REGULAR))
    }

    /// A made-up token of GitHub's shape, built at run time so none sits in the source.
    fn token() -> String {
        let body: String = (0..36).map(|i| (b'a' + (i * 7 % 26) as u8) as char).collect();
        format!("{}{}{}", "gh", "p_", body)
    }

    #[test]
    fn plain_text_is_the_line_one_character_per_column() {
        let p = plan(&Row::from_text("web-7731:~$ ls", 20), None);
        assert_eq!(p.text, "web-7731:~$ ls      ");
        // One run in the scheme's own text colour: without it the row would take the window's.
        let fg = Palette::new(true).fg();
        assert_eq!(p.spans.len(), 1, "{:?}", p.spans);
        assert_eq!((p.spans[0].start, p.spans[0].end, p.spans[0].color, p.spans[0].bold), (0, 20, Some(fg), false));
        assert!(p.cells.is_empty() && p.fills.is_empty() && p.ops.is_empty());
    }

    #[test]
    fn colours_bold_and_backgrounds_become_spans_and_fills() {
        let mut row = Row::from_text("ab cd", 6);
        row.cells[0].fg = TermColor::Indexed(1);
        row.cells[1].fg = TermColor::Indexed(1);
        row.cells[1].flags |= CellFlags::BOLD;
        row.cells[3].bg = TermColor::Indexed(4);
        row.cells[4].bg = TermColor::Indexed(4);
        row.cells[4].flags |= CellFlags::UNDERLINE;
        let p = plan(&row, Some(1..3));
        let red = Palette::new(true).color(TermColor::Indexed(1));
        let blue = Palette::new(true).color(TermColor::Indexed(4));
        assert_eq!(p.text, "ab cd ");
        let fg = Palette::new(true).fg();
        assert_eq!(p.spans.len(), 5, "{:?}", p.spans);
        assert_eq!((p.spans[0].start, p.spans[0].end, p.spans[0].color, p.spans[0].bold), (0, 1, Some(red), false));
        assert_eq!((p.spans[1].start, p.spans[1].end, p.spans[1].bold), (1, 2, true));
        assert_eq!((p.spans[2].start, p.spans[2].end, p.spans[2].color), (2, 4, Some(fg)));
        assert_eq!((p.spans[3].start, p.spans[3].end, p.spans[3].underline.is_some()), (4, 5, true));
        assert_eq!((p.spans[4].start, p.spans[4].end, p.spans[4].color), (5, 6, Some(fg)));
        assert_eq!(p.fills[0], (3, 5, blue));
        // The selection goes last, over the backgrounds.
        assert_eq!(p.fills[1], (1, 3, Palette::new(true).selection));
    }

    #[test]
    fn inverse_swaps_the_defaults() {
        let mut row = Row::from_text("x", 2);
        row.cells[0].flags |= CellFlags::INVERSE;
        let p = plan(&row, None);
        let palette = Palette::new(true);
        assert_eq!(p.fills, [(0, 1, palette.fg())]);
        assert_eq!(p.spans[0].color, Some(palette.bg()));
    }

    #[test]
    fn wide_marked_and_missing_characters_are_pinned_cells() {
        let p = plan(&Row::from_text("a漢e\u{301}😀b", 10), None);
        // a, 漢 (two spaces), e+mark (a space), 😀 (two spaces), b.
        assert_eq!(p.text, "a   " .to_string() + "  b   ");
        let cells: Vec<(u16, u16, &str)> = p.cells.iter().map(|c| (c.col, c.width, c.text.as_str())).collect();
        assert_eq!(cells, [(1, 2, "漢"), (3, 1, "e\u{301}"), (4, 2, "😀")]);
        assert_eq!(p.text.chars().count(), 10);
    }

    #[test]
    fn box_drawing_is_shapes_and_spaces() {
        let p = plan(&Row::from_text("┌─┐ ok", 8), None);
        assert_eq!(p.text, "    ok  ");
        assert!(p.ops.iter().any(|(x, _, _)| *x == 0) && p.ops.iter().any(|(x, _, _)| *x == 14));
        assert!(p.cells.is_empty());
    }

    #[test]
    fn a_hidden_secret_is_dots_in_the_line_and_never_its_characters() {
        let secret = token();
        let row = Row::from_text(&format!("GH_TOKEN={secret}"), 60);
        let p = plan(&row, None);
        assert!(!p.text.contains(&secret[..4]), "{}", p.text);
        for k in 0..secret.len() - 4 {
            assert!(!p.text.contains(&secret[k..k + 4]));
        }
        assert!(p.text.starts_with("GH_TOKEN=•••"));
        assert_eq!(p.text.chars().count(), 60);
        let pill = p.spans.iter().find(|s| s.pill.is_some()).expect("a pill");
        assert_eq!(p.text[pill.start..pill.end].chars().count(), secret.len());
        assert!(p.text[pill.start..pill.end].chars().all(|c| c == DOT));
        assert!(p.cells.iter().all(|c| !secret.contains(&c.text)));
    }

    #[test]
    fn rows_that_only_moved_come_back_as_the_same_rc() {
        let mut view = PaneView::new(Size::new(10, 3), Look { hide_secrets: true, ..Look::default() });
        let m = metrics();
        let blank = Row::blank(10);
        let frame = |rows: [&str; 3]| Frame {
            size: Size::new(10, 3),
            rows: rows.iter().enumerate().map(|(line, t)| sshee_term::model::RowSnapshot { line, row: Row::from_text(t, 10) }).collect(),
            full: true,
            cursor: Cursor::default(),
            modes: Modes::default(),
            display_offset: 0,
            history: 0,
            selection: None,
            scroll: Default::default(),
            last_output: None,
            edges: EdgeRows::default(),
        };
        let _ = blank;
        view.intake(frame(["one", "two", "three"]));
        let first = view.rows(m, true, true);
        assert_eq!(view.stats, RowStats { built: 3, reused: 0 });
        // The same inputs again: nothing is built or looked up.
        let again = view.rows(m, true, true);
        assert!(first.iter().zip(&again).all(|(a, b)| Rc::ptr_eq(a, b)));
        assert_eq!(view.stats, RowStats { built: 3, reused: 0 });
        // Scrolled by one: two rows moved, one is new.
        view.intake(frame(["two", "three", "four"]));
        let moved = view.rows(m, true, true);
        assert!(Rc::ptr_eq(&moved[0], &first[1]) && Rc::ptr_eq(&moved[1], &first[2]));
        assert_eq!(view.stats, RowStats { built: 4, reused: 2 });
        // Another scale: everything is built again.
        let m2 = CellMetrics::new(13.0, 7.8, 17.16, 1.22, 2.0, super::super::metrics::CellMode::Floor);
        view.rows(m2, true, true);
        assert_eq!(view.stats.built, 7);
        let _: Cell = Cell::default();
    }

    fn frame_of(rows: &[String]) -> Frame {
        let size = Size::new(10, rows.len() as u16);
        Frame {
            size,
            rows: rows.iter().enumerate().map(|(line, t)| sshee_term::model::RowSnapshot { line, row: Row::from_text(t, 10) }).collect(),
            full: true,
            cursor: Cursor::default(),
            modes: Modes::default(),
            display_offset: 0,
            history: 0,
            selection: None,
            scroll: Default::default(),
            last_output: None,
            edges: EdgeRows::default(),
        }
    }

    /// A flood of new lines builds rows that never come back: only the screen's and the one
    /// before it are kept, not every row built (each holds its shaped layout).
    #[test]
    fn a_flood_keeps_two_screens_of_rows() {
        let mut view = PaneView::new(Size::new(10, 4), Look { hide_secrets: true, ..Look::default() });
        let m = metrics();
        for n in 0..2000 {
            let rows: Vec<String> = (0..4).map(|k| format!("line {}", n * 4 + k)).collect();
            view.intake(frame_of(&rows));
            view.rows(m, true, true);
            assert!(view.kept_rows() <= 4, "{} rows kept", view.kept_rows());
        }
        assert_eq!(view.stats.built, 8000);
        // A scroll there and back (the wheel, one line up and down) still finds the rows it moved.
        let a: Vec<String> = (0..4).map(|k| format!("x{k}")).collect();
        let b: Vec<String> = (0..4).map(|k| format!("x{}", k + 1)).collect();
        view.intake(frame_of(&a));
        let first = view.rows(m, true, true);
        view.intake(frame_of(&b));
        view.rows(m, true, true);
        view.intake(frame_of(&a));
        let back = view.rows(m, true, true);
        assert!(first.iter().zip(&back).all(|(x, y)| Rc::ptr_eq(x, y)));
    }

    #[test]
    fn a_click_shows_a_secret_and_only_then() {
        let secret = token();
        let mut view = PaneView::new(Size::new(60, 1), Look { hide_secrets: true, ..Look::default() });
        view.intake(Frame {
            size: Size::new(60, 1),
            rows: vec![sshee_term::model::RowSnapshot { line: 0, row: Row::from_text(&format!("export GH_TOKEN={secret}"), 60) }],
            full: true,
            cursor: Cursor::default(),
            modes: Modes::default(),
            display_offset: 0,
            history: 0,
            selection: None,
            scroll: Default::default(),
            last_output: None,
            edges: EdgeRows::default(),
        });
        let m = metrics();
        view.rows(m, true, true);
        assert!(!view.revealable_at(0, 3));
        assert!(view.revealable_at(0, 20));
        assert!(!view.shown(0).unwrap().text.contains(&secret));
        assert!(view.reveal(0, 20));
        assert!(view.shown(0).unwrap().text.contains(&secret));
    }

    /// Relative luminance and contrast as WCAG counts them (a `Color` holds linear values).
    fn contrast(a: Color, b: Color) -> f32 {
        let l = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let (hi, lo) = if l(a) > l(b) { (l(a), l(b)) } else { (l(b), l(a)) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn find_marks_keep_the_text_readable() {
        use super::super::palette::Scheme;
        for scheme in [Scheme::Classic, Scheme::Fresh] {
            for dark in [true, false] {
                let p = Palette::of(scheme, dark);
                let (quiet, quiet_ink) = found_colors(&p, false);
                let (strong, strong_ink) = found_colors(&p, true);
                assert!(contrast(quiet, quiet_ink) >= 4.5, "{scheme:?} dark={dark}: a match's text is {:.1}:1", contrast(quiet, quiet_ink));
                assert!(contrast(strong, strong_ink) >= 4.5, "{scheme:?} dark={dark}: the current match's text is {:.1}:1", contrast(strong, strong_ink));
                // The current match stands out from the others more than they do from the page.
                assert!(contrast(strong, p.bg()) > contrast(quiet, p.bg()) * 2.0);
                assert!(contrast(quiet, p.bg()) > 1.15, "{scheme:?} dark={dark}: a match can't be told from the page");
                // Neither is the selection's blue.
                assert!(quiet != p.selection && strong != p.selection);
            }
        }
    }

    #[test]
    fn find_matches_are_fills_with_their_own_ink_and_never_show_a_secret() {
        use sshee_term::present::FindMarks;
        let secret = token();
        let rows = [Row::from_text("red needle, blue needle", 30), Row::from_text(&format!("KEY={secret}"), 60)];
        let palette = Palette::new(true);
        let m = metrics();
        let look = RowLook { palette: &palette, metrics: &m, selection: palette.selection };
        let present = |row: &Row, query: &str, current: Option<(usize, usize)>| {
            let texts = row_texts(std::slice::from_ref(row), false);
            let mut screen = Screen::new(&texts, row.cells.len() as u16);
            screen.find = Some(FindMarks { query, current });
            Presenter::new(Look { hide_secrets: true, ..Look::default() }).present(&screen)
        };
        let mut coverage = Coverage::new(crate::fonts::TERMINAL_REGULAR);
        // Two matches, the second the one the bar is on.
        let shown = present(&rows[0], "needle", Some((0, 17)));
        let p = plan_row(&rows[0], &shown[0], None, None, &look, &mut coverage);
        let (quiet, quiet_ink) = found_colors(&palette, false);
        let (strong, strong_ink) = found_colors(&palette, true);
        assert_eq!(p.fills, [(4, 10, quiet), (17, 23, strong)]);
        let ink = |col: usize| p.spans.iter().find(|s| (s.start..s.end).contains(&col)).and_then(|s| s.color);
        assert_eq!((ink(5), ink(18), ink(0)), (Some(quiet_ink), Some(strong_ink), Some(palette.fg())));
        // The text itself is as it was: a mark changes colours only.
        assert_eq!(p.text.trim_end(), "red needle, blue needle");
        // Looking for a piece of a hidden secret marks nothing: the pass searches the dots.
        let shown = present(&rows[1], &secret[8..16], None);
        let p = plan_row(&rows[1], &shown[0], None, None, &look, &mut coverage);
        assert!(p.fills.is_empty(), "{:?}", p.fills);
        assert!(!p.text.contains(&secret[8..16]));
    }

    #[test]
    fn the_link_under_the_pointer_is_underlined_and_a_paths_underline_is_dotted() {
        let row = Row::from_text("see https://example.com now", 30);
        let web = plan(&row, None);
        assert!(web.spans.iter().all(|s| s.underline.is_none()) && web.ops.is_empty(), "a link is underlined before the pointer is on it");
        let palette = Palette::new(true);
        let m = metrics();
        let look = RowLook { palette: &palette, metrics: &m, selection: palette.selection };
        let shown = present(std::slice::from_ref(&row));
        let mut coverage = Coverage::new(crate::fonts::TERMINAL_REGULAR);
        // A web link: a line under its cells and no others.
        let p = plan_row(&row, &shown[0], None, Some(&(4..23, false)), &look, &mut coverage);
        let lined: Vec<(usize, usize)> = p.spans.iter().filter(|s| s.underline.is_some()).map(|s| (s.start, s.end)).collect();
        assert_eq!(lined, [(4, 23)]);
        assert!(p.ops.is_empty());
        // A path: dots instead, a pixel on and two off, all within its cells.
        let p = plan_row(&row, &shown[0], None, Some(&(4..23, true)), &look, &mut coverage);
        assert!(p.spans.iter().all(|s| s.underline.is_none()));
        let cw = m.cell_w_px as i32;
        let dots: Vec<i32> = p.ops.iter().map(|(x, op, _)| match op {
            Op::Rect { x: dx, w, h, .. } => {
                assert_eq!((*w, *h), (1, 1));
                x + dx
            }
            _ => panic!("a dot is a rectangle"),
        }).collect();
        assert!(dots.len() > 30 && dots.windows(2).all(|d| d[1] - d[0] == 3), "{dots:?}");
        assert!(dots[0] >= 4 * cw && *dots.last().unwrap() < 23 * cw);
    }
}
