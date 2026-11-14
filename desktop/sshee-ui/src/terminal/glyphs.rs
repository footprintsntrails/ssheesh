//! Box drawing, block elements and Powerline separators drawn as shapes, not glyphs.
//!
//! With a line height of 1.22 a font's own box glyphs stop short of the cell's top and bottom,
//! which shows as gaps between rows; xterm.js draws these characters itself (its WebGL
//! renderer's `customGlyphs`, on in 0.10). This is a port of its definitions, reworked to put
//! every edge on a device pixel: lines are rectangles whose thickness and centre are whole
//! pixels, and every line that reaches a cell's edge reaches it exactly, so neighbours meet
//! without seams at fractional scales too.
//!
//! Ported from xterm.js, `addon-webgl/src/CustomGlyphs.ts`:
//!
//! Copyright (c) 2021 The xterm.js authors. All rights reserved.
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! software and associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! to whom the Software is furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all copies or
//! substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//! INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//! FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

/// One piece of a drawn character, in device pixels from the cell's top-left.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A filled rectangle; `alpha` scales the colour (the shades ░▒▓).
    Rect { x: i32, y: i32, w: i32, h: i32, alpha: f32 },
    /// A filled polygon (Powerline triangles and half circles).
    Poly(Vec<[f32; 2]>),
    /// A stroked polyline `width` pixels wide (arcs, diagonals, Powerline chevrons).
    Stroke { points: Vec<[f32; 2]>, width: f32 },
}

/// Whether `c` is drawn here rather than by the font.
pub fn is_custom(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x259F | 0xE0B0..=0xE0B7)
}

/// A line's weight on one arm of a box-drawing character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum W {
    None,
    Light,
    Heavy,
    Double,
}

/// Arms of U+2500 to U+257F, up, right, down, left: `.` none, `l` light, `h` heavy, `d`
/// double; `-` marks the dashed lines, arcs and diagonals, drawn on their own.
const ARMS: [&str; 128] = [
    ".l.l", ".h.h", "l.l.", "h.h.", "----", "----", "----", "----", "----", "----", "----", "----", ".ll.", ".hl.", ".lh.", ".hh.", //
    "..ll", "..lh", "..hl", "..hh", "ll..", "lh..", "hl..", "hh..", "l..l", "l..h", "h..l", "h..h", "lll.", "lhl.", "hll.", "llh.", //
    "hlh.", "hhl.", "lhh.", "hhh.", "l.ll", "l.lh", "h.ll", "l.hl", "h.hl", "h.lh", "l.hh", "h.hh", ".lll", ".llh", ".hll", ".hlh", //
    ".lhl", ".lhh", ".hhl", ".hhh", "ll.l", "ll.h", "lh.l", "lh.h", "hl.l", "hl.h", "hh.l", "hh.h", "llll", "lllh", "lhll", "lhlh", //
    "hlll", "llhl", "hlhl", "hllh", "hhll", "llhh", "lhhl", "hhlh", "lhhh", "hlhh", "hhhl", "hhhh", "----", "----", "----", "----", //
    ".d.d", "d.d.", ".dl.", ".ld.", ".dd.", "..ld", "..dl", "..dd", "ld..", "dl..", "dd..", "l..d", "d..l", "d..d", "ldl.", "dld.", //
    "ddd.", "l.ld", "d.dl", "d.dd", ".dld", ".ldl", ".ddd", "ld.d", "dl.l", "dd.d", "ldld", "dldl", "dddd", "----", "----", "----", //
    "----", "----", "----", "----", "...l", "l...", ".l..", "..l.", "...h", "h...", ".h..", "..h.", ".h.l", "l.h.", ".l.h", "h.l.", //
];

fn arms(c: char) -> Option<[W; 4]> {
    let i = (c as u32).checked_sub(0x2500)? as usize;
    let s = ARMS.get(i)?.as_bytes();
    if s[0] == b'-' {
        return None;
    }
    let w = |b: u8| match b {
        b'l' => W::Light,
        b'h' => W::Heavy,
        b'd' => W::Double,
        _ => W::None,
    };
    Some([w(s[0]), w(s[1]), w(s[2]), w(s[3])])
}

/// The light line's thickness in pixels for a cell `w` pixels wide: one pixel at 1x, two at 2x.
pub fn light_px(w: i32) -> i32 {
    ((w as f32 / 8.0).round() as i32).max(1)
}

/// Where the lines of one arm lie across it: `(start, end)` pixel ranges, one for a single
/// line, two (low then high) for a double.
fn across(weight: W, size: i32, t: i32) -> Vec<(i32, i32)> {
    let line = |k: i32, off: i32| {
        let a = (size - k).div_euclid(2) + off;
        (a, a + k)
    };
    match weight {
        W::None => Vec::new(),
        W::Light => vec![line(t, 0)],
        W::Heavy => vec![line(2 * t, 0)],
        W::Double => vec![line(t, -t), line(t, t)],
    }
}

const UP: usize = 0;
const RIGHT: usize = 1;
const DOWN: usize = 2;
const LEFT: usize = 3;

/// The ops for a box-drawing character with these arms, in a `w` × `h` pixel cell.
///
/// Each arm's line runs from the cell's edge toward the centre. A single line stops at the far
/// edge of the lines crossing it, so corners and joins close. Each line of a double borders a
/// quadrant: it stops where the crossing arm on its own side does (an inner corner); with no
/// arm there it runs straight on when the arm opposite continues it, else turns at the far line
/// of the arm on the other side (an outer corner).
fn box_ops(a: [W; 4], w: i32, h: i32) -> Vec<Op> {
    let t = light_px(w);
    let vert = |arm: usize| across(a[arm], w, t);
    let horiz = |arm: usize| across(a[arm], h, t);
    let mut ops = Vec::new();
    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| Op::Rect { x: x0, y: y0, w: (x1 - x0).max(0), h: (y1 - y0).max(0), alpha: 1.0 };
    // Along a vertical arm (up or down): how far its line at `range` (across, in x) reaches.
    // `toward_end`: the arm runs from the top (true for up) and ends at a larger y.
    let reach = |arm: usize, range: (i32, i32), lines: &[(i32, i32)], crossing: [usize; 2], size_along: i32, across_of: &dyn Fn(usize) -> Vec<(i32, i32)>| -> i32 {
        let from_low = arm == UP || arm == LEFT;
        let opposite = (arm + 2) % 4;
        // The far edge, in the arm's own direction, of a set of crossing lines.
        let far = |ls: &[(i32, i32)]| if from_low { ls.iter().map(|l| l.1).max() } else { ls.iter().map(|l| l.0).min() };
        let near_line = |ls: &[(i32, i32)], low_side: bool| -> Option<(i32, i32)> {
            if low_side { ls.first().copied() } else { ls.last().copied() }
        };
        let mid = |k: i32| {
            let a = (size_along - k).div_euclid(2);
            if from_low { a + k } else { a }
        };
        let own_k = range.1 - range.0;
        let all_crossing: Vec<(i32, i32)> = crossing.iter().flat_map(|&c| across_of(c)).collect();
        if lines.len() == 1 {
            let both = crossing.iter().all(|&c| a[c] != W::None);
            let double = crossing.iter().any(|&c| a[c] == W::Double);
            if a[opposite] == W::None && both && double {
                // A single stem on a double line (╤ ╧ ╟ ╢): it meets the nearer line and
                // leaves the gap between the two open.
                let near = if from_low { all_crossing.iter().min_by_key(|l| l.0) } else { all_crossing.iter().max_by_key(|l| l.1) };
                if let Some(l) = near {
                    return if from_low { l.1 } else { l.0 };
                }
            }
            // Otherwise through every crossing line (a corner, a join, a cross), or to the centre.
            return far(&all_crossing).unwrap_or_else(|| mid(own_k));
        }
        // A double's line: which side of the arm is it on?
        let low_side = range == lines[0];
        // The crossing arm on that side: for a vertical arm the low side (left) is LEFT; for a
        // horizontal arm the low side (top) is UP.
        let same = if crossing.contains(&LEFT) || crossing.contains(&RIGHT) {
            if low_side { LEFT } else { RIGHT }
        } else if low_side {
            UP
        } else {
            DOWN
        };
        let other = if same == LEFT { RIGHT } else if same == RIGHT { LEFT } else if same == UP { DOWN } else { UP };
        let same_lines = across_of(same);
        if !same_lines.is_empty() {
            // An inner corner: stop through the line of that arm nearest this arm's end.
            let l = near_line(&same_lines, from_low).expect("not empty");
            return if from_low { l.1 } else { l.0 };
        }
        if a[opposite] != W::None {
            return mid(own_k);
        }
        let other_lines = across_of(other);
        far(&other_lines).unwrap_or_else(|| mid(own_k))
    };
    for arm in [UP, DOWN] {
        let lines = vert(arm);
        for &range in &lines {
            let end = reach(arm, range, &lines, [LEFT, RIGHT], h, &horiz);
            if arm == UP {
                ops.push(rect(range.0, 0, range.1, end));
            } else {
                ops.push(rect(range.0, end, range.1, h));
            }
        }
    }
    for arm in [LEFT, RIGHT] {
        let lines = horiz(arm);
        for &range in &lines {
            let end = reach(arm, range, &lines, [UP, DOWN], w, &vert);
            if arm == LEFT {
                ops.push(rect(0, range.0, end, range.1));
            } else {
                ops.push(rect(end, range.0, w, range.1));
            }
        }
    }
    ops
}

/// Dashed lines: `n` dashes along a light or heavy line, as xterm.js spaces them.
fn dash_ops(c: char, w: i32, h: i32) -> Vec<Op> {
    let (n, heavy, vertical) = match c {
        '┄' => (3, false, false),
        '┅' => (3, true, false),
        '┆' => (3, false, true),
        '┇' => (3, true, true),
        '┈' => (4, false, false),
        '┉' => (4, true, false),
        '┊' => (4, false, true),
        '┋' => (4, true, true),
        '╌' => (2, false, false),
        '╍' => (2, true, false),
        '╎' => (2, false, true),
        '╏' => (2, true, true),
        _ => return Vec::new(),
    };
    let t = light_px(w) * if heavy { 2 } else { 1 };
    let (along, size_across) = if vertical { (h, w) } else { (w, h) };
    let a = (size_across - t).div_euclid(2);
    // Each dash takes the middle of its share, with a gap of a third of its share around it.
    let mut ops = Vec::new();
    for i in 0..n {
        let s0 = along * i / n;
        let s1 = along * (i + 1) / n;
        let gap = ((s1 - s0) as f32 / 6.0).round() as i32;
        let (d0, d1) = (s0 + gap, s1 - gap);
        ops.push(if vertical {
            Op::Rect { x: a, y: d0, w: t, h: d1 - d0, alpha: 1.0 }
        } else {
            Op::Rect { x: d0, y: a, w: d1 - d0, h: t, alpha: 1.0 }
        });
    }
    ops
}

/// A cubic Bézier from `p0` to `p3`, flattened.
fn cubic(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], out: &mut Vec<[f32; 2]>) {
    const STEPS: usize = 12;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        let f = |k: usize| u * u * u * p0[k] + 3.0 * u * u * t * p1[k] + 3.0 * u * t * t * p2[k] + t * t * t * p3[k];
        out.push([f(0), f(1)]);
    }
}

/// The rounded corners ╭╮╯╰: a quarter circle joining the cell's middle lines.
fn arc_ops(c: char, w: i32, h: i32) -> Vec<Op> {
    let t = light_px(w);
    // The light line's centre, across and along.
    let cx = (w - t).div_euclid(2) as f32 + t as f32 / 2.0;
    let cy = (h - t).div_euclid(2) as f32 + t as f32 / 2.0;
    let (wf, hf) = (w as f32, h as f32);
    let r = (wf / 2.0).min(hf / 2.0);
    let k = 0.552_284_8 * r;
    // The vertical end (at the top or bottom edge) and the horizontal end (left or right edge).
    let (vy, sy) = match c {
        '╭' | '╮' => (hf, 1.0),
        _ => (0.0, -1.0),
    };
    let (hx, sx) = match c {
        '╭' | '╰' => (wf, 1.0),
        _ => (0.0, -1.0),
    };
    let start = [cx, vy];
    let bend0 = [cx, cy + sy * r];
    let bend1 = [cx + sx * r, cy];
    let mut points = vec![start, bend0];
    cubic(bend0, [cx, cy + sy * (r - k)], [cx + sx * (r - k), cy], bend1, &mut points);
    points.push([hx, cy]);
    vec![Op::Stroke { points, width: t as f32 }]
}

fn diagonal_ops(c: char, w: i32, h: i32) -> Vec<Op> {
    let t = light_px(w) as f32;
    let (wf, hf) = (w as f32, h as f32);
    let a = Op::Stroke { points: vec![[wf, 0.0], [0.0, hf]], width: t };
    let b = Op::Stroke { points: vec![[0.0, 0.0], [wf, hf]], width: t };
    match c {
        '╱' => vec![a],
        '╲' => vec![b],
        _ => vec![a, b],
    }
}

/// U+2580 to U+259F: eighths as rectangles, the shades as the colour at 25, 50 and 75%.
fn block_ops(c: char, w: i32, h: i32) -> Vec<Op> {
    // (x, y, w, h) in eighths of the cell.
    let boxes: &[(i32, i32, i32, i32)] = match c {
        '▀' => &[(0, 0, 8, 4)],
        '▁' => &[(0, 7, 8, 1)],
        '▂' => &[(0, 6, 8, 2)],
        '▃' => &[(0, 5, 8, 3)],
        '▄' => &[(0, 4, 8, 4)],
        '▅' => &[(0, 3, 8, 5)],
        '▆' => &[(0, 2, 8, 6)],
        '▇' => &[(0, 1, 8, 7)],
        '█' => &[(0, 0, 8, 8)],
        '▉' => &[(0, 0, 7, 8)],
        '▊' => &[(0, 0, 6, 8)],
        '▋' => &[(0, 0, 5, 8)],
        '▌' => &[(0, 0, 4, 8)],
        '▍' => &[(0, 0, 3, 8)],
        '▎' => &[(0, 0, 2, 8)],
        '▏' => &[(0, 0, 1, 8)],
        '▐' => &[(4, 0, 4, 8)],
        '▔' => &[(0, 0, 8, 1)],
        '▕' => &[(7, 0, 1, 8)],
        '▖' => &[(0, 4, 4, 4)],
        '▗' => &[(4, 4, 4, 4)],
        '▘' => &[(0, 0, 4, 4)],
        '▙' => &[(0, 0, 4, 8), (4, 4, 4, 4)],
        '▚' => &[(0, 0, 4, 4), (4, 4, 4, 4)],
        '▛' => &[(0, 0, 4, 8), (4, 0, 4, 4)],
        '▜' => &[(0, 0, 8, 4), (4, 4, 4, 4)],
        '▝' => &[(4, 0, 4, 4)],
        '▞' => &[(4, 0, 4, 4), (0, 4, 4, 4)],
        '▟' => &[(4, 0, 4, 8), (0, 4, 4, 4)],
        '░' | '▒' | '▓' => {
            let alpha = match c {
                '░' => 0.25,
                '▒' => 0.5,
                _ => 0.75,
            };
            return vec![Op::Rect { x: 0, y: 0, w, h, alpha }];
        }
        _ => &[],
    };
    // Edges at whole pixels, so halves and quarters of neighbours meet.
    let ex = |e: i32| ((w * e) as f32 / 8.0).round() as i32;
    let ey = |e: i32| ((h * e) as f32 / 8.0).round() as i32;
    boxes
        .iter()
        .map(|&(x, y, bw, bh)| {
            let (x0, y0, x1, y1) = (ex(x), ey(y), ex(x + bw), ey(y + bh));
            Op::Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0, alpha: 1.0 }
        })
        .collect()
}

/// Powerline's separators, U+E0B0 to U+E0B7.
fn powerline_ops(c: char, w: i32, h: i32) -> Vec<Op> {
    let (wf, hf) = (w as f32, h as f32);
    let t = light_px(w) as f32;
    // A half circle bulging right from the left edge, in a unit square, flipped for the left ones.
    let half_circle = |flip: bool| {
        let mut points = vec![[0.0, 0.0], [0.0, 1.0]];
        cubic([0.0, 1.0], [0.552, 1.0], [1.0, 0.776], [1.0, 0.5], &mut points);
        cubic([1.0, 0.5], [1.0, 0.224], [0.552, 0.0], [0.0, 0.0], &mut points);
        points.pop();
        points.into_iter().map(|[x, y]| [if flip { (1.0 - x) * wf } else { x * wf }, y * hf]).collect::<Vec<_>>()
    };
    match c {
        '\u{e0b0}' => vec![Op::Poly(vec![[0.0, 0.0], [wf, hf / 2.0], [0.0, hf]])],
        '\u{e0b2}' => vec![Op::Poly(vec![[wf, 0.0], [0.0, hf / 2.0], [wf, hf]])],
        '\u{e0b1}' => vec![Op::Stroke { points: vec![[0.0, 0.0], [wf - t / 2.0, hf / 2.0], [0.0, hf]], width: t }],
        '\u{e0b3}' => vec![Op::Stroke { points: vec![[wf, 0.0], [t / 2.0, hf / 2.0], [wf, hf]], width: t }],
        '\u{e0b4}' => vec![Op::Poly(half_circle(false))],
        '\u{e0b6}' => vec![Op::Poly(half_circle(true))],
        '\u{e0b5}' | '\u{e0b7}' => {
            let flip = c == '\u{e0b7}';
            let mut points = vec![[0.2, 1.0]];
            cubic([0.2, 1.0], [0.422, 1.0], [0.8, 0.826], [0.78, 0.5], &mut points);
            cubic([0.78, 0.5], [0.8, 0.174], [0.422, 0.0], [0.2, 0.0], &mut points);
            let points = points.into_iter().map(|[x, y]| [if flip { (1.0 - x) * wf } else { x * wf }, y * hf]).collect();
            vec![Op::Stroke { points, width: t }]
        }
        _ => Vec::new(),
    }
}

/// How to draw `c` in a cell `w` × `h` device pixels, if it is drawn here.
pub fn ops(c: char, w: i32, h: i32) -> Vec<Op> {
    if let Some(a) = arms(c) {
        return box_ops(a, w, h);
    }
    match c as u32 {
        0x2504..=0x250B | 0x254C..=0x254F => dash_ops(c, w, h),
        0x256D..=0x2570 => arc_ops(c, w, h),
        0x2571..=0x2573 => diagonal_ops(c, w, h),
        0x2580..=0x259F => block_ops(c, w, h),
        0xE0B0..=0xE0B7 => powerline_ops(c, w, h),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cell's pixels each op covers (rectangles only), as a bitmap.
    fn paint(c: char, w: i32, h: i32) -> Vec<Vec<bool>> {
        let mut px = vec![vec![false; w as usize]; h as usize];
        for op in ops(c, w, h) {
            if let Op::Rect { x, y, w: rw, h: rh, .. } = op {
                for yy in y.max(0)..(y + rh).min(h) {
                    for xx in x.max(0)..(x + rw).min(w) {
                        px[yy as usize][xx as usize] = true;
                    }
                }
            }
        }
        px
    }

    fn column(px: &[Vec<bool>], x: usize) -> Vec<bool> {
        px.iter().map(|r| r[x]).collect()
    }

    const CELLS: [(i32, i32); 5] = [(7, 21), (9, 26), (11, 31), (13, 37), (15, 42)];

    #[test]
    fn every_character_of_the_ranges_draws_something() {
        for c in (0x2500..=0x259F).chain(0xE0B0..=0xE0B7) {
            let c = char::from_u32(c).unwrap();
            assert!(is_custom(c));
            assert!(!ops(c, 9, 21).is_empty(), "{c} U+{:04X}", c as u32);
        }
        assert!(!is_custom('a') && !is_custom('\u{e0b8}'));
    }

    #[test]
    fn vertical_lines_reach_the_top_and_bottom_and_horizontal_ones_the_sides() {
        for (w, h) in CELLS {
            for c in ['│', '┃', '║', '┼', '╋', '╬', '├', '┤', '╠', '╣'] {
                let px = paint(c, w, h);
                assert!((0..w as usize).any(|x| px[0][x]) && (0..w as usize).any(|x| px[h as usize - 1][x]), "{c} {w}x{h}");
                // The columns lit at the top are lit all the way down: no gap.
                for x in 0..w as usize {
                    if px[0][x] && c != '╬' && c != '╠' && c != '╣' {
                        assert!(column(&px, x).iter().all(|&b| b), "{c} {w}x{h} column {x}");
                    }
                }
            }
            for c in ['─', '━', '═', '┼', '┬', '┴', '╦', '╩'] {
                let px = paint(c, w, h);
                assert!((0..h as usize).any(|y| px[y][0]) && (0..h as usize).any(|y| px[y][w as usize - 1]), "{c} {w}x{h}");
            }
        }
    }

    #[test]
    fn a_vertical_line_lines_up_with_the_corners_above_and_below() {
        for (w, h) in CELLS {
            let lit = |c: char, y: usize| (0..w as usize).filter(|&x| paint(c, w, h)[y][x]).collect::<Vec<_>>();
            let bar = lit('│', 0);
            assert_eq!(lit('┌', h as usize - 1), bar, "{w}x{h}");
            assert_eq!(lit('└', 0), bar);
            assert_eq!(lit('┼', 0), bar);
            let row = |c: char, x: usize| (0..h as usize).filter(|&y| paint(c, w, h)[y][x]).collect::<Vec<_>>();
            assert_eq!(row('┌', w as usize - 1), row('─', 0));
            assert_eq!(row('┐', 0), row('─', w as usize - 1));
            // The light line is a pixel at 1x and two at 2x.
            assert_eq!(bar.len() as i32, light_px(w));
            assert_eq!(lit('┃', 0).len() as i32, 2 * light_px(w));
        }
    }

    #[test]
    fn corners_close() {
        let px = paint('┌', 9, 21);
        // The corner pixel where the two lines meet is lit, and nothing lies left of the bar
        // or above the line.
        let bar: Vec<usize> = (0..9).filter(|&x| px[20][x]).collect();
        let line: Vec<usize> = (0..21).filter(|&y| px[y][8]).collect();
        assert!(px[line[0]][bar[0]]);
        assert!((0..bar[0]).all(|x| (0..21).all(|y| !px[y][x])));
        assert!((0..line[0]).all(|y| (0..9).all(|x| !px[y][x])));
    }

    #[test]
    fn doubles_have_inner_and_outer_corners() {
        // ╔: the outer line turns at the top-left, the inner one inside it.
        let px = paint('╔', 15, 42);
        let bottom: Vec<usize> = (0..15).filter(|&x| px[41][x]).collect();
        let right: Vec<usize> = (0..42).filter(|&y| px[y][14]).collect();
        assert_eq!(bottom.len(), 4, "two lines of two pixels: {bottom:?}");
        assert_eq!(right.len(), 4, "{right:?}");
        // Above the outer horizontal line nothing is lit; left of the outer vertical line neither.
        assert!((0..right[0]).all(|y| (0..15).all(|x| !px[y][x])));
        assert!((0..bottom[0]).all(|x| (0..42).all(|y| !px[y][x])));
        // Between the two lines, inside the corner, the gap stays open.
        let gap_y = right[1] + 1;
        assert!(!px[gap_y][14]);
    }

    #[test]
    fn a_single_stem_meets_the_nearer_of_two_lines() {
        let px = paint('╤', 15, 42);
        let bar: Vec<usize> = (0..15).filter(|&x| px[41][x]).collect();
        let lines: Vec<usize> = (0..42).filter(|&y| px[y][0]).collect();
        assert_eq!(lines.len(), 4);
        // The stem's columns are dark between the two lines.
        for (y, row) in px.iter().enumerate().take(lines[2]).skip(lines[1] + 1) {
            assert!(bar.iter().all(|&x| !row[x]), "row {y}");
        }
    }

    #[test]
    fn blocks_tile_without_gaps() {
        for (w, h) in CELLS {
            let full = paint('█', w, h);
            assert!(full.iter().all(|r| r.iter().all(|&b| b)));
            let top = paint('▀', w, h);
            let bottom = paint('▄', w, h);
            for y in 0..h as usize {
                for x in 0..w as usize {
                    assert!(top[y][x] ^ bottom[y][x], "{w}x{h} at {x},{y}");
                }
            }
        }
    }
}
