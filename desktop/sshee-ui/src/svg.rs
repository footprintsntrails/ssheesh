//! SVG path data as a Rune `Path`, so an icon is drawn from the same numbers as in the design
//! (and in 0.10's own drawings) rather than retyped as curves by hand. Only path data written
//! in this repository goes through here: no SVG files, no markup, nothing from outside (Rune's
//! `svg` feature stays off).
//!
//! Every command of the path grammar is read: `M L H V C S Q T A Z`, absolute and relative,
//! with numbers run together as SVG allows (`M4.5 4.5l7 7`, `1.2-3`, `.5.5`). An arc becomes
//! cubic curves of a quarter turn at most.

use rune::Path;

/// Scales and moves a drawing's own coordinates into the element's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Fit {
    /// A drawing on a `view` × `view` grid whose corner is at (`min_x`, `min_y`), shown `size`
    /// points wide.
    pub fn square(min_x: f32, min_y: f32, view: f32, size: f32) -> Fit {
        let scale = size / view;
        Fit { scale, dx: -min_x * scale, dy: -min_y * scale }
    }

    fn at(self, p: [f32; 2]) -> [f32; 2] {
        [p[0] * self.scale + self.dx, p[1] * self.scale + self.dy]
    }
}

/// Splits path data into commands and numbers.
struct Tokens<'a> {
    rest: &'a [u8],
}

impl Tokens<'_> {
    fn skip_space(&mut self) {
        while let [b' ' | b',' | b'\n' | b'\t' | b'\r', rest @ ..] = self.rest {
            self.rest = rest;
        }
    }

    /// The command letter here, if one is.
    fn command(&mut self) -> Option<u8> {
        self.skip_space();
        match self.rest {
            [c, rest @ ..] if c.is_ascii_alphabetic() && !matches!(c, b'e' | b'E') => {
                self.rest = rest;
                Some(*c)
            }
            _ => None,
        }
    }

    /// The number here, if one is.
    fn number(&mut self) -> Option<f32> {
        self.skip_space();
        let s = self.rest;
        let mut i = 0;
        if matches!(s.first(), Some(b'-' | b'+')) {
            i += 1;
        }
        let digits = |i: &mut usize| {
            let from = *i;
            while s.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
            *i > from
        };
        let whole = digits(&mut i);
        let mut fraction = false;
        if s.get(i) == Some(&b'.') {
            i += 1;
            fraction = digits(&mut i);
        }
        if !whole && !fraction {
            return None;
        }
        if matches!(s.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(s.get(j), Some(b'-' | b'+')) {
                j += 1;
            }
            if digits(&mut j) {
                i = j;
            }
        }
        let n = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
        self.rest = &s[i..];
        Some(n)
    }

    /// An arc's flag: one digit, 0 or 1, which may sit right against the next number.
    fn flag(&mut self) -> Option<bool> {
        self.skip_space();
        match self.rest {
            [b @ (b'0' | b'1'), rest @ ..] => {
                self.rest = rest;
                Some(*b == b'1')
            }
            _ => None,
        }
    }
}

/// Path data as a `Path` in the element's points. What can't be read ends the path there: a
/// drawing written in this repository reads whole, which each icon's test checks with
/// [`reads_whole`].
pub fn path(data: &str, fit: Fit) -> Path {
    build(data, fit).0
}

/// Whether every command of `data` was read.
pub fn reads_whole(data: &str) -> bool {
    build(data, Fit { scale: 1.0, dx: 0.0, dy: 0.0 }).1
}

fn build(data: &str, fit: Fit) -> (Path, bool) {
    let mut t = Tokens { rest: data.as_bytes() };
    let mut out = Path::new();
    // The pen, the subpath's start, and the last control point (for S and T).
    let (mut pen, mut start) = ([0.0f32; 2], [0.0f32; 2]);
    let mut last_cubic: Option<[f32; 2]> = None;
    let mut last_quad: Option<[f32; 2]> = None;
    let mut command = match t.command() {
        Some(c) => c,
        None => return (out, data.trim().is_empty()),
    };
    loop {
        let relative = command.is_ascii_lowercase();
        let base = if relative { pen } else { [0.0, 0.0] };
        // One set of arguments for `command`; `None` when they run out or don't read.
        let point = |t: &mut Tokens| Some([base[0] + t.number()?, base[1] + t.number()?]);
        let done = match command.to_ascii_uppercase() {
            b'M' => point(&mut t).map(|p| {
                out = std::mem::take(&mut out).move_to(fit.at(p));
                pen = p;
                start = p;
                (None, None)
            }),
            b'L' => point(&mut t).map(|p| {
                out = std::mem::take(&mut out).line_to(fit.at(p));
                pen = p;
                (None, None)
            }),
            b'H' => t.number().map(|x| {
                pen = [base[0] + x, pen[1]];
                out = std::mem::take(&mut out).line_to(fit.at(pen));
                (None, None)
            }),
            b'V' => t.number().map(|y| {
                pen = [pen[0], base[1] + y];
                out = std::mem::take(&mut out).line_to(fit.at(pen));
                (None, None)
            }),
            b'C' => (|| {
                let (c1, c2, p) = (point(&mut t)?, point(&mut t)?, point(&mut t)?);
                out = std::mem::take(&mut out).cubic_to(fit.at(c1), fit.at(c2), fit.at(p));
                pen = p;
                Some((Some(c2), None))
            })(),
            b'S' => (|| {
                let (c2, p) = (point(&mut t)?, point(&mut t)?);
                let c1 = last_cubic.map_or(pen, |c| [2.0 * pen[0] - c[0], 2.0 * pen[1] - c[1]]);
                out = std::mem::take(&mut out).cubic_to(fit.at(c1), fit.at(c2), fit.at(p));
                pen = p;
                Some((Some(c2), None))
            })(),
            b'Q' => (|| {
                let (c, p) = (point(&mut t)?, point(&mut t)?);
                out = std::mem::take(&mut out).quad_to(fit.at(c), fit.at(p));
                pen = p;
                Some((None, Some(c)))
            })(),
            b'T' => point(&mut t).map(|p| {
                let c = last_quad.map_or(pen, |c| [2.0 * pen[0] - c[0], 2.0 * pen[1] - c[1]]);
                out = std::mem::take(&mut out).quad_to(fit.at(c), fit.at(p));
                pen = p;
                (None, Some(c))
            }),
            b'A' => (|| {
                let (rx, ry, turn) = (t.number()?, t.number()?, t.number()?);
                let (large, sweep) = (t.flag()?, t.flag()?);
                let p = point(&mut t)?;
                for [c1, c2, end] in arc(pen, [rx, ry], turn, large, sweep, p) {
                    out = std::mem::take(&mut out).cubic_to(fit.at(c1), fit.at(c2), fit.at(end));
                }
                pen = p;
                Some((None, None))
            })(),
            b'Z' => {
                out = std::mem::take(&mut out).close();
                pen = start;
                Some((None, None))
            }
            _ => None,
        };
        let Some((cubic, quad)) = done else { return (out, false) };
        last_cubic = cubic;
        last_quad = quad;
        // The next command; or the same again when more numbers follow (after a move, lines).
        match t.command() {
            Some(c) => command = c,
            None => {
                t.skip_space();
                if t.rest.is_empty() {
                    return (out, true);
                }
                if command.eq_ignore_ascii_case(&b'Z') {
                    return (out, false);
                }
                if command.eq_ignore_ascii_case(&b'M') {
                    command = if relative { b'l' } else { b'L' };
                }
            }
        }
    }
}

/// An elliptical arc from `from` to `to` as cubic curves `[control, control, end]`, by the SVG
/// specification's conversion from endpoints to a centre (appendix B.2.4).
fn arc(from: [f32; 2], radii: [f32; 2], turn_degrees: f32, large: bool, sweep: bool, to: [f32; 2]) -> Vec<[[f32; 2]; 3]> {
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    if from == to {
        return vec![];
    }
    if rx == 0.0 || ry == 0.0 {
        return vec![[from, to, to]];
    }
    let (sin, cos) = turn_degrees.to_radians().sin_cos();
    let (dx, dy) = ((from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    // Radii too small to reach are grown until they do.
    let reach = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if reach > 1.0 {
        rx *= reach.sqrt();
        ry *= reach.sqrt();
    }
    let (rx2, ry2) = (rx * rx, ry * ry);
    let under = (rx2 * ry2 - rx2 * y1 * y1 - ry2 * x1 * x1).max(0.0) / (rx2 * y1 * y1 + ry2 * x1 * x1);
    let k = under.sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let (cxp, cyp) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (from[0] + to[0]) / 2.0, sin * cxp + cos * cyp + (from[1] + to[1]) / 2.0);
    let angle = |x: f32, y: f32| y.atan2(x);
    let a0 = angle((x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut sweep_angle = angle((-x1 - cxp) / rx, (-y1 - cyp) / ry) - a0;
    if sweep && sweep_angle < 0.0 {
        sweep_angle += std::f32::consts::TAU;
    } else if !sweep && sweep_angle > 0.0 {
        sweep_angle -= std::f32::consts::TAU;
    }
    let pieces = (sweep_angle.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep_angle / pieces as f32;
    // The control points' distance for a piece of `step` radians.
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let on = |a: f32| -> ([f32; 2], [f32; 2]) {
        let (s, c) = a.sin_cos();
        // The point on the ellipse at `a`, and the direction of travel there.
        ([cx + cos * rx * c - sin * ry * s, cy + sin * rx * c + cos * ry * s], [-cos * rx * s - sin * ry * c, -sin * rx * s + cos * ry * c])
    };
    (0..pieces)
        .map(|i| {
            let (p0, d0) = on(a0 + step * i as f32);
            let (p1, d1) = on(a0 + step * (i + 1) as f32);
            let end = if i + 1 == pieces { to } else { p1 };
            [[p0[0] + handle * d0[0], p0[1] + handle * d0[1]], [p1[0] - handle * d1[0], p1[1] - handle * d1[1]], end]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn numbers_run_together_as_svg_allows() {
        let mut t = Tokens { rest: b"M4.5 4.5l7 7M1.2-3,.5.5 1e1-2E-1" };
        assert_eq!(t.command(), Some(b'M'));
        assert_eq!((t.number(), t.number()), (Some(4.5), Some(4.5)));
        assert_eq!(t.number(), None);
        assert_eq!(t.command(), Some(b'l'));
        assert_eq!((t.number(), t.number()), (Some(7.0), Some(7.0)));
        assert_eq!(t.command(), Some(b'M'));
        let rest: Vec<f32> = std::iter::from_fn(|| t.number()).collect();
        assert_eq!(rest, [1.2, -3.0, 0.5, 0.5, 10.0, -0.2]);
        // An arc's flags can sit against each other and the next number.
        let mut t = Tokens { rest: b"011.5" };
        assert_eq!((t.flag(), t.flag(), t.number()), (Some(false), Some(true), Some(1.5)));
    }

    #[test]
    fn every_command_reads_and_what_does_not_is_told() {
        for data in ["M1 2 3 4 5 6", "M1 2h3v4H1V2z", "M0 0c1 1 2 1 3 0s2-1 3 0", "M0 0q1 1 2 0t2 0", "M8 2a6 6 0 1 0 0 12A6 6 0 0 1 8 2Z", "", "  "] {
            assert!(reads_whole(data), "{data:?}");
        }
        for data in ["M1", "L", "M0 0 X 1 1", "1 2", "M0 0a1 1 0 2 0 1 1", "M0 0z1 1"] {
            assert!(!reads_whole(data), "{data:?}");
        }
    }

    #[test]
    fn a_half_circle_is_two_quarter_curves_through_the_top() {
        // From the left of a circle of radius 6 around (8, 8), clockwise on screen, to the right.
        let pieces = arc([2.0, 8.0], [6.0, 6.0], 0.0, false, true, [14.0, 8.0]);
        assert_eq!(pieces.len(), 2);
        assert!(close(pieces[0][2], [8.0, 2.0]), "{:?}", pieces[0]);
        assert!(close(pieces[1][2], [14.0, 8.0]));
        // The first control point goes straight up from the start, 0.5523 of the radius.
        assert!(close(pieces[0][0], [2.0, 8.0 - 6.0 * 0.552_284_8]), "{:?}", pieces[0]);
        // The other way round goes through the bottom.
        let pieces = arc([2.0, 8.0], [6.0, 6.0], 0.0, false, false, [14.0, 8.0]);
        assert!(close(pieces[0][2], [8.0, 14.0]), "{:?}", pieces[0]);
        // Radii too small to span the ends are grown, as SVG says: still a half circle.
        let pieces = arc([0.0, 0.0], [1.0, 1.0], 0.0, false, true, [10.0, 0.0]);
        assert!(close(pieces[0][2], [5.0, -5.0]), "{:?}", pieces[0]);
        assert!(arc([1.0, 1.0], [2.0, 2.0], 0.0, true, true, [1.0, 1.0]).is_empty());
    }

    #[test]
    fn a_drawing_is_fitted_to_its_element() {
        let fit = Fit::square(8.0, 8.0, 184.0, 23.0);
        assert!(close(fit.at([8.0, 8.0]), [0.0, 0.0]));
        assert!(close(fit.at([192.0, 100.0]), [23.0, 11.5]));
    }
}
