//! The pixel server icons (Settings → Layout → Server icons → Pixels): one filled shape each
//! on a 24-pixel grid, to match the >_< logo, by the ids of `sshee_app::labels::HOST_ICONS`
//! (and `local` for this computer). 0.10's `src/lib/pixelIcons.ts`.
//!
//! From pixelarticons by Gerrit Halfmann, https://github.com/halfmage/pixelarticons (MIT
//! licence; see desktop/THIRD-PARTY.md), as 0.10 had them; `local`, an old desktop, was drawn
//! for SSHeesh in the same manner.
//!
//! Each drawing is SVG path data of straight lines on whole pixels (`M m H h V v z`). It's
//! filled as SVG fills it (non-zero: an inner square drawn the other way round is a hole) into
//! a 24 × 24 grid once, and drawn as rectangles of whole pixels, so its edges stay square.

use std::collections::HashMap;
use std::sync::OnceLock;

use rune::{canvas, Color, El, Shape};

/// The grid's side, in pixels.
const GRID: usize = 24;

/// Every drawing, by id.
const ICONS: [(&str, &str); 31] = [
    ("server", "M6 7h4v2H6zm0 8h4v2H6zM2 5h2v14H2zm18 0h2v14h-2zM4 19h16v2H4zM4 3h16v2H4zm0 8h16v2H4z"),
    ("cloud", "M22 10h-4v2h4zm2 2h-2v6h2zm-2 6H2v2h20zM2 12H0v6h2zm2-2H2v2h2zm4-2H4v2h4zm8-4h-6v2h6zm-6 2H8v2h2zm0 4H8v2h2zm8-4h-2v2h2zM20 8h-2v4h2zm-2 4h-2v2h2z"),
    ("globe", "M6 2h12v2H6zm0 18h12v2H6zM4 4h2v2H4zm5 0h2v2H9zm0 14h2v2H9zm4 0h2v2h-2zM7 6h2v12H7zm8 0h2v12h-2zm-2-2h2v2h-2zm7 0h-2v2h2zM2 6h2v12H2zm20 0h-2v12h2zM4 18h2v2H4zm16 0h-2v2h2zM3 11h18v2H3z"),
    ("database", "M2 6h2v4H2zm0 4h2v4H2zm0 4h2v4H2zm18-8h2v4h-2zm0 4h2v4h-2zm0 4h2v4h-2zM4 4h4v2H4zm0 8h4v-2H4zm0 4h4v-2H4zm0 4h4v-2H4zM16 4h4v2h-4zm0 8h4v-2h-4zm0 4h4v-2h-4zm0 4h4v-2h-4zM8 2h8v2H8zm0 12h8v-2H8zm0 4h8v-2H8zm0 4h8v-2H8z"),
    ("box", "M10 20h4v2h-4zm0-16h4V2h-4zm0 6h4v2h-4zm4 8h4v2h-4zm0-12h4V4h-4zm0 2h4v2h-4zm4 8h4v2h-4zm0-8h4V6h-4zM6 18h4v2H6zM6 6h4V4H6zm0 2h4v2H6zm-4 8h4v2H2zm0-8h4V6H2zM2 6h2v12H2zm18 0h2v12h-2zm-8 6h2v8h-2zm-2-6h4v2h-4z"),
    ("home", "M4 20h16v2H4zm16-10h2v10h-2zM2 10h2v10H2zm2-2h2v2H4zm2-2h2v2H6zm2-2h2v2H8zm2-2h4v2h-4zm4 2h2v2h-2zm2 2h2v2h-2zm2 2h2v2h-2zM8 14h2v6H8zm2-2h4v2h-4zm4 2h2v6h-2z"),
    ("router", "M22 2h-2v2h2v12h-2v2h2v-2h2V4h-2zM2 4H0v12h2v2h2v-2H2zm0 0V2h2v2zm4 2H4v8h2zm0 0V4h2v2zm4 0h4v2h-4zm0 6H8V8h2zm4 0h-4v2H8v4H6v4h2v-4h2v-4h4v4h2v4h2v-4h-2v-4h-2zm0 0h2V8h-2zm6-6h-2V4h-2v2h2v8h2z"),
    ("drive", "M20 22H4v-2h2v-6h2v6h8v-6h2v6h2zM4 20H2V4h2zm18 0h-2V6h2zm-6-6H8v-2h8zm-4-4H6V6h6zm8-4h-2V4h2zm-2-2H4V2h14z"),
    ("cpu", "M5 3h14v2H5zm0 16h14v2H5zM3 5h2v14H3zm16 0h2v14h-2zM9 7h6v2H9zm0 8h6v2H9zM7 9h2v6H7zm8 0h2v6h-2zm-4-8h2v2h-2zm0 20h2v2h-2zM1 11h2v2H1zm20 0h2v2h-2zm0-4h2v2h-2zm0 8h2v2h-2zM1 15h2v2H1zm0-8h2v2H1zm6-6h2v2H7zm8 0h2v2h-2zm0 20h2v2h-2zm-8 0h2v2H7z"),
    ("shield", "M4 2h16v2H4zM2 4h2v10H2zm18 0h2v10h-2zM4 14h2v2H4zm2 2h2v2H6zm4 4h4v2h-4zm10-6h-2v2h2zm-2 2h-2v2h2zm-2 2h-2v2h2zm-6 0H8v2h2z"),
    ("vpn", "M5 8h14v2H5zm0 12h14v2H5zM3 10h2v10H3zm16 0h2v10h-2zM7 4h2v4H7zm2-2h6v2H9zm6 2h2v4h-2z"),
    ("rocket", "M4 13h8v6h2v2h-2v2h-2v-8H2v-4h2zm12 6h-2v-2h2zm2-2h-2v-2h2zm2-2h-2v-2h2zm-6-6h8v4h-2v-2h-8V5h-2V3h2V1h2zm-8 2H4V9h2zm2-2H6V7h2zm2-2H8V5h2z"),
    ("flask", "M8 6h8v2H8zm0-4h8v2H8zm0 6h2v2H8zm6 0h2v2h-2zM6 20h12v2H6zm-2-8h2v8H4zm14 0h2v8h-2zM6 10h2v2H6zm10 0h2v2h-2zM6 4h2v2H6zm10 0h2v2h-2z"),
    ("bot", "M5 7h14v2H5zm0 12h14v2H5zM3 9h2v10H3zm16 0h2v10h-2zM1 13h2v2H1zm20 0h2v2h-2zM11 5h2v2h-2zM7 3h4v2H7zm1 9h2v4H8zm6 0h2v4h-2z"),
    ("ai", "M11 1h2v4h-2zm0 22h2v-4h-2zM9 5h2v4H9zm0 14h2v-4H9zm4-14h2v4h-2zm0 14h2v-4h-2zM5 9h4v2H5zm14 0h-4v2h4zM1 11h4v2H1zm22 0h-4v2h4zM5 13h4v2H5zm14 0h-4v2h4zm0-12h2v6h-2zM17 3h6v2h-6zM3 17h2v2H3zm-2 2h2v2H1zm2 2h2v2H3zm2-2h2v2H5z"),
    ("terminal", "M4 2h16v2H4zm0 18h16v2H4zM2 4h2v16H2zm18 0h2v16h-2zM6 16h2v2H6zm2-2h2v2H8zm-2-2h2v2H6z"),
    ("git", "M4 14h4v2H4zm0 6h4v2H4zm-2-4h2v4H2zm6 0h2v4H8zm8-14h4v2h-4zm0 6h4v2h-4zm-2-4h2v4h-2zm6 0h2v4h-2zm-8 13h5v2h-5zm5-5h2v5h-2zM5 2h2v10H5z"),
    ("mail", "M6 8h2v2H6zm2 2h2v2H8zm10-2h-2v2h2zm-2 2h-2v2h2zm-6 2h4v2h-4zM2 6h2v12H2zm18 0h2v12h-2zM4 4h16v2H4zm0 14h16v2H4z"),
    ("chat", "M20 2H2v20h2V4h16v12H6v2H4v2h2v-2h16V2z"),
    ("shop", "M3 13h2v8H3zm2 8h14v2H5zm14-8h2v8h-2zm-9-2h4v2h-4zm4-2h4v2h-4zm4 2h4v2h-4zM6 9h4v2H6zm-4 2h4v2H2zM0 7h2v4H0zm2-2h2v2H2zm18 0h2v2h-2zm2 2h2v4h-2zM4 3h16v2H4zm6 12h4v2h-4zm-2 2h2v4H8zm6 0h2v4h-2z"),
    ("vault", "M11 18H3v-2h8zm12-3h-2v3h-4v-2h2v-3h2v-2H11V8h2v1h10zM3 16H1V8h2zm14 0h-2v-1h-2v1h-2v-3h6zm-8-2H5v-4h4zm2-6H3V6h8z"),
    ("chart", "M22 22H4v-2h18zM4 20H2V2h2zm4-6H6v-2h2zm8 0h-2v-2h2zm-6-2H8v-2h2zm4 0h-2v-2h2zm4 0h-2v-2h2zm-6-2h-2V8h2zm8 0h-2V8h2zm2-2h-2V6h2z"),
    ("backup", "M3 2h18v2H3zm0 5h18v2H3zM1 4h2v3H1zm20 0h2v3h-2zm-2 5h2v11h-2zM3 9h2v11H3zm2 11h14v2H5zm4-9h6v2H9z"),
    ("game", "M4 4h16v2H4zm0 14h16v2H4zM2 6h2v12H2zm18 0h2v12h-2zM8 9h2v6H8zM6 11h6v2H6zm8-2h2v2h-2zm2 4h2v2h-2z"),
    ("film", "M3 3h18v18H3zm2 2v2h2V5zm4 0v6h6V5zm8 0v2h2V5zm2 4h-2v2h2zm0 4h-2v2h2zm0 4h-2v2h2zm-4 2v-6H9v6zm-8 0v-2H5v2zm-2-4h2v-2H5zm0-4h2V9H5z"),
    ("music", "M4 12h4v2H4zm-2 2h2v4H2zm2 4h4v2H4zM8 6h2v12H8zm10 0h2v12h-2zm-6 8h2v4h-2zm2-2h4v2h-4zm0 6h4v2h-4zM10 4h8v2h-8z"),
    ("camera", "M4 5h4v2H4zm4-2h8v2H8zm8 2h4v2h-4zM2 7h2v12H2zm2 12h16v2H4zM20 7h2v12h-2zM10 8h4v2h-4zm0 6h4v2h-4zm-2-4h2v4H8zm6 0h2v4h-2z"),
    ("iot", "M11 19h2v2h-2zm-4-3h2v2H7zm8 0h2v2h-2zm-6-2h6v2H9zm-5-1h2v2H4zm2-2h2v2H6zm2-2h8v2H8zm-7 1h2v2H1zm20 0h2v2h-2zM3 8h2v2H3zm2-2h2v2H5zm2-2h10v2H7zm12 4h2v2h-2zm-2-2h2v2h-2zm1 7h2v2h-2zm-2-2h2v2h-2z"),
    ("desktop", "M4 2h16v2H4zm0 14h16v2H4zM2 4h2v12H2zm18 0h2v12h-2zm-9 14h2v2h-2zm-3 2h8v2H8z"),
    ("laptop", "M3 18h18v-2h2v4H1v-4h2zm2-4h14V6h2v10H3V6h2zm14-8H5V4h14z"),
    ("local", "M6 1h12v2H6zm0 8h12v2H6zM4 3h2v6H4zm14 0h2v6h-2zM4 13h16v2H4zm0 8h16v2H4zm-2-6h2v6H2zm18 0h2v6h-2zM6 17h2v2H6zm4 0h8v2h-8zm-2-6h2v2H8zm6 0h2v2h-2z"),
];

/// A filled rectangle of whole pixels: `[x, y, width, height]`.
type Block = [u8; 4];

/// Path data as a command letter and the numbers after it, in order.
fn commands(data: &str) -> Vec<(u8, Vec<i32>)> {
    let bytes = data.as_bytes();
    let mut out: Vec<(u8, Vec<i32>)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() {
            out.push((c, Vec::new()));
            i += 1;
        } else if c == b'-' || c.is_ascii_digit() {
            let from = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if let Some((_, numbers)) = out.last_mut() {
                numbers.push(data[from..i].parse().unwrap_or(0));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The outlines in a drawing's path data, each a closed polygon of whole-pixel corners.
fn outlines(data: &str) -> Vec<Vec<[i32; 2]>> {
    let mut out = Vec::new();
    let mut poly: Vec<[i32; 2]> = Vec::new();
    let (mut at, mut start) = ([0i32, 0], [0i32, 0]);
    let mut finish = |poly: &mut Vec<[i32; 2]>| {
        if poly.len() > 2 {
            out.push(std::mem::take(poly));
        }
        poly.clear();
    };
    for (command, numbers) in commands(data) {
        let rel = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            b'M' => {
                for (k, pair) in numbers.chunks(2).enumerate() {
                    let [x, y] = [pair[0], pair.get(1).copied().unwrap_or(0)];
                    at = if rel { [at[0] + x, at[1] + y] } else { [x, y] };
                    // The first pair starts an outline; more are lines from it, as in SVG.
                    if k == 0 {
                        finish(&mut poly);
                        start = at;
                    }
                    poly.push(at);
                }
            }
            b'H' => {
                for x in numbers {
                    at[0] = if rel { at[0] + x } else { x };
                    poly.push(at);
                }
            }
            b'V' => {
                for y in numbers {
                    at[1] = if rel { at[1] + y } else { y };
                    poly.push(at);
                }
            }
            b'Z' => {
                finish(&mut poly);
                at = start;
            }
            _ => {}
        }
    }
    finish(&mut poly);
    out
}

/// Which of the grid's pixels a drawing fills, row by row: the middle of each pixel, by the
/// non-zero rule (the drawings are made of up-and-down and across lines only, so only the
/// upright edges right of a pixel's middle count).
fn filled(data: &str) -> [[bool; GRID]; GRID] {
    let polys = outlines(data);
    let mut grid = [[false; GRID]; GRID];
    for (y, line) in grid.iter_mut().enumerate() {
        let py = y as f32 + 0.5;
        for (x, cell) in line.iter_mut().enumerate() {
            let px = x as f32 + 0.5;
            let mut winding = 0;
            for poly in &polys {
                for k in 0..poly.len() {
                    let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                    if a[0] != b[0] || (a[0] as f32) < px {
                        continue;
                    }
                    let (lo, hi) = (a[1].min(b[1]) as f32, a[1].max(b[1]) as f32);
                    if lo <= py && py < hi {
                        winding += if b[1] > a[1] { 1 } else { -1 };
                    }
                }
            }
            *cell = winding != 0;
        }
    }
    grid
}

/// The filled pixels as few rectangles as a row-by-row sweep makes: runs across, joined with
/// the same run in the rows under them. Fewer edges, no seams inside a bar.
fn blocks(grid: &[[bool; GRID]; GRID]) -> Vec<Block> {
    let mut open: Vec<Block> = Vec::new();
    let mut done = Vec::new();
    for (y, line) in grid.iter().enumerate() {
        let mut runs = Vec::new();
        let mut x = 0;
        while x < GRID {
            if line[x] {
                let from = x;
                while x < GRID && line[x] {
                    x += 1;
                }
                runs.push((from as u8, (x - from) as u8));
            } else {
                x += 1;
            }
        }
        let mut next = Vec::new();
        for (x, w) in runs {
            match open.iter().position(|b| b[0] == x && b[2] == w) {
                Some(k) => {
                    let mut b = open.swap_remove(k);
                    b[3] += 1;
                    next.push(b);
                }
                None => next.push([x, y as u8, w, 1]),
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done
}

fn table() -> &'static HashMap<&'static str, Vec<Block>> {
    static TABLE: OnceLock<HashMap<&'static str, Vec<Block>>> = OnceLock::new();
    TABLE.get_or_init(|| ICONS.iter().map(|(id, data)| (*id, blocks(&filled(data)))).collect())
}

/// The pixel drawing of a server's icon `size` points square in `color`, by its icon's id
/// (`local` for this computer); an id without one is drawn as `server`.
pub fn pixel_icon(id: &str, size: f32, color: Color) -> El {
    let blocks = table().get(id).or_else(|| table().get("server")).map(Vec::as_slice).unwrap_or_default();
    let k = size / GRID as f32;
    let shape = blocks.iter().fold(Shape::new(), |shape, b| {
        let [x, y, w, h] = b.map(|v| f32::from(v) * k);
        shape.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], color)
    });
    canvas(shape).size(size, size).a11y_hidden()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_server_icon_has_a_pixel_drawing() {
        for (id, _) in sshee_app::labels::HOST_ICONS {
            let blocks = table().get(id).unwrap_or_else(|| panic!("{id} has no pixel drawing"));
            assert!(!blocks.is_empty(), "{id} draws nothing");
        }
        assert!(table().contains_key("local"));
    }

    #[test]
    fn a_square_drawn_the_other_way_round_is_a_hole() {
        // Media: a frame with holes for the film's sprockets.
        let film = filled(ICONS.iter().find(|(id, _)| *id == "film").unwrap().1);
        assert!(film[3][3] && film[4][4], "the frame");
        assert!(!film[5][5] && !film[6][6], "a sprocket hole");
        // The plain server: its top bar is a whole row of pixels, its inside empty.
        let server = filled(ICONS[0].1);
        assert!((4..20).all(|x| server[3][x]));
        assert!(!server[7][12]);
    }

    #[test]
    fn rows_of_the_same_run_are_one_rectangle() {
        let mut grid = [[false; GRID]; GRID];
        for line in grid.iter_mut().take(4).skip(2) {
            line[1..5].fill(true);
        }
        grid[5][7] = true;
        let mut b = blocks(&grid);
        b.sort();
        assert_eq!(b, [[1, 2, 4, 2], [7, 5, 1, 1]]);
    }
}
