//! A connected server's load in the sidebar (0.10's `ServerLoad.tsx`): on its icon in the
//! rail, as Settings → Layout → Server load picks (a frame that fills with CPU, upright meters
//! for CPU, memory and disk, a CPU pulse under the icon, or the number of tabs open there), and
//! as CPU and RAM meters under its name in the list. And the mark a server's icon gets while
//! its terminals connect, reconnect or couldn't connect.
//!
//! Quiet on purpose, as the Stats page's meters are: grey ink, the warning's orange only past
//! 90% of a disk or memory. Only a server with a connected terminal shows its load: its numbers
//! are fresh (`app/src/vitals.rs` looks at it every ten seconds, the one on screen every two).

use rune::{canvas, col, el, row, text, El, LineCap, Shape};
use sshee_app::model::RailStatus;
use sshee_app::workbench::vitals::{self, Vitals, FULL};
use sshee_app::workbench::Health;

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::graph::{graph, meter, Series};
use crate::kit::{progress_ring, ringed_dot};
use crate::theme::{self, Theme};

/// The looks the pulse draws: the last minute or so of the server on screen.
const PULSE: usize = 30;

/// What the sidebar shows of a server's load: CPU, memory and the fullest disk (0 to 1), its
/// warning, and its CPU so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Load {
    pub cpu: f64,
    pub memory: f64,
    pub disk: f64,
    pub warning: Option<String>,
    pub history: Vec<f64>,
}

impl Load {
    pub fn of(v: &Vitals, history: Vec<f64>) -> Load {
        Load { cpu: v.cpu.unwrap_or(0.0), memory: v.memory, disk: v.disk.as_ref().map_or(0.0, |d| d.0), warning: vitals::warning(v), history }
    }

    /// "CPU 38% · Memory 60% · Disk 44%": what a screen reader says of it.
    pub fn summary(&self) -> String {
        format!("CPU {} · Memory {} · Disk {}", vitals::percent(self.cpu), vitals::percent(self.memory), vitals::percent(self.disk))
    }
}

/// A server's load, when Settings shows it and a terminal there is connected.
pub fn load(s: &Shell, key: &str) -> Option<(RailStatus, Load)> {
    let style = s.settings.server_load();
    if matches!(style, RailStatus::Off | RailStatus::Other(_)) || s.bench.health(key) != Health::Live {
        return None;
    }
    let v = s.bench.vitals(key)?;
    let history = s.bench.history(key).map(|h| h.series(|x| x.cpu)).unwrap_or_default();
    let skip = history.len().saturating_sub(PULSE);
    Some((style, Load::of(v, history[skip..].to_vec())))
}

/// What a screen reader says of a server's state, when it's marked.
pub fn health_label(health: Health) -> Option<&'static str> {
    match health {
        Health::Connecting => Some("Connecting"),
        Health::Lost => Some("Connection lost, reconnecting"),
        Health::Failed => Some("Couldn't connect"),
        Health::Idle | Health::Live => None,
    }
}

/// The mark of a server's state, `size` points: a ring on its way round while it connects, an
/// orange dot while it reconnects, a red one when it couldn't connect. Nothing when all is well.
pub fn health_mark(t: &Theme, health: Health, size: f32) -> Option<El> {
    let label = health_label(health)?;
    let mark = match health {
        Health::Connecting => el().size(size + 4.0, size + 4.0).radius(size / 2.0 + 2.0).fill(t.frame).items_center().justify_center().child(progress_ring(size, 1.5, t.l2)),
        Health::Lost => ringed_dot(size - 1.0, 2.0, t.orange_dot, t.frame),
        _ => ringed_dot(size - 1.0, 2.0, t.red, t.frame),
    };
    // The drawing is hidden from a screen reader; what it means isn't.
    Some(el().key("health").role("img").label_static(label).child(mark.a11y_hidden()))
}

/// The rail button's rounded square, traced clockwise from the top middle: its points, inset
/// `inset` from a `size` square with corners of radius `r`.
fn frame_points(size: f32, inset: f32, r: f32) -> Vec<[f32; 2]> {
    let (a, b) = (inset, size - inset);
    let mut points = vec![[size / 2.0, a]];
    // Each corner: its centre, and the angle its quarter turn starts at.
    let corners = [([b - r, a + r], -90.0f32), ([b - r, b - r], 0.0), ([a + r, b - r], 90.0), ([a + r, a + r], 180.0)];
    for (c, start) in corners {
        for i in 0..=8 {
            let angle = (start + 90.0 * i as f32 / 8.0).to_radians();
            points.push([c[0] + r * angle.cos(), c[1] + r * angle.sin()]);
        }
    }
    points.push([size / 2.0, a]);
    points
}

/// The first `fraction` of a polyline's length.
fn part_of(points: &[[f32; 2]], fraction: f32) -> Vec<[f32; 2]> {
    let lengths: Vec<f32> = points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).collect();
    let mut left = lengths.iter().sum::<f32>() * fraction.clamp(0.0, 1.0);
    let mut out = vec![points[0]];
    for (w, len) in points.windows(2).zip(lengths) {
        if left >= len {
            out.push(w[1]);
            left -= len;
        } else {
            let k = if len > 0.0 { left / len } else { 0.0 };
            out.push([w[0][0] + (w[1][0] - w[0][0]) * k, w[0][1] + (w[1][1] - w[0][1]) * k]);
            break;
        }
    }
    out
}

/// The ink of a load: grey, the warning's orange once a disk or memory is nearly full.
fn ink(t: &Theme, warn: bool) -> rune::Color {
    if warn {
        t.orange_dot
    } else {
        t.l2on
    }
}

/// How big a server's icon is drawn on its rail button: a little smaller beside its load.
pub fn icon_size(style: Option<&RailStatus>) -> f32 {
    match style {
        Some(RailStatus::Bars) => 17.0,
        Some(RailStatus::Pulse) => 16.0,
        Some(RailStatus::Frame) => 18.0,
        _ => theme::ICON_RAIL,
    }
}

/// Where the icon sits on its button beside the load, from the middle.
pub fn icon_shift(style: Option<&RailStatus>) -> (f32, f32) {
    match style {
        Some(RailStatus::Bars) => (-4.0, 0.0),
        Some(RailStatus::Pulse) => (0.0, -4.0),
        _ => (0.0, 0.0),
    }
}

/// The load drawn on a rail button `size` points square, over its icon. `tabs`: how many tabs
/// are open there; `waiting`: one of them waits for you (the count turns orange).
pub fn on_icon(t: &Theme, style: &RailStatus, load: &Load, size: f32, tabs: usize, waiting: bool) -> Option<El> {
    let warn = load.warning.is_some();
    let e = match style {
        RailStatus::Off | RailStatus::Other(_) => return None,
        RailStatus::Frame => {
            let outline = frame_points(size, 1.0, theme::RADIUS - 1.0);
            let filled = part_of(&outline, (load.cpu as f32).max(0.01));
            let track = if warn { t.orange_dot.alpha(0.3) } else { t.hair2 };
            let shape = Shape::new().stroke_with(outline, false, 2.0, LineCap::Round, track).stroke_with(filled, false, 2.0, LineCap::Round, ink(t, warn));
            canvas(shape).size(size, size).absolute(0.0, 0.0)
        }
        RailStatus::Bars => {
            let bar = |v: f64| {
                let full = v >= FULL;
                col().w(3.0).h(16.0).radius(1.5).fill(t.control).clip_corners([1.5; 4]).justify_end().child(el().w_full().h(16.0 * (v.clamp(0.0, 1.0) as f32).max(0.12)).radius(1.5).fill(ink(t, full)))
            };
            row().gap(1.5).absolute(size - 13.0, (size - 16.0) / 2.0).child(bar(load.cpu)).child(bar(load.memory)).child(bar(load.disk))
        }
        RailStatus::Pulse => {
            // Scaled to the recent peak, so a calm server still shows a heartbeat.
            let peak = load.history.iter().copied().fold(0.2, f64::max);
            let line = graph(t, &[Series { values: &load.history, ink: ink(t, warn), wash: false }], PULSE, (peak * 1.3).min(1.0), 7.0);
            el().w(22.0).absolute((size - 22.0) / 2.0, size - 11.0).child(line)
        }
        RailStatus::Count => {
            let (fill, words) = if waiting { (t.orange_dot, t.on_color) } else if warn { (t.control, t.orange) } else { (t.control, t.l1) };
            el().absolute(size - 13.0, size - 13.0)
                .h(15.0)
                .min_w(15.0)
                .px(4.0)
                .radius(7.5)
                .fill(fill)
                .border(1.5, t.frame)
                .items_center()
                .justify_center()
                .child(text(tabs.min(99).to_string()).font(theme::SMALL).color(words).single_line())
        }
    };
    Some(e.key("load").label(load.summary()).role("img"))
}

/// CPU and RAM under a server's name in the list, and its warning when it has one.
pub fn under_name(t: &Theme, load: &Load, indent: f32) -> El {
    let pair = |label: &'static str, v: f64| {
        row().grow(1.0).min_w(0.0).gap(5.0).items_center().child(text(label).font(theme::SMALL).color(t.l2).single_line()).child(el().grow(1.0).min_w(0.0).child(meter(t, v, v >= FULL, 3.0)))
    };
    let mut e = col().key("load-line").w_full().pl(indent).pr(8.0).pb(6.0).gap(4.0).label(load.summary()).child(row().w_full().gap(9.0).items_center().child(pair("CPU", load.cpu)).child(pair("RAM", load.memory)));
    if let Some(warning) = &load.warning {
        e = e.child(row().gap(5.0).items_center().child(icon(Icon::Warning, 11.0, t.orange)).child(text(warning).font(theme::SMALL).color(t.l1).single_line().truncate().shrink().min_w(0.0)));
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_is_traced_from_the_top_middle_and_cut_at_its_share() {
        let outline = frame_points(32.0, 1.0, 7.0);
        assert_eq!(outline[0], [16.0, 1.0]);
        assert_eq!(*outline.last().unwrap(), [16.0, 1.0]);
        // Every point on the square's edge or its rounded corners.
        assert!(outline.iter().all(|p| (1.0..=31.0).contains(&(p[0].round())) && (1.0..=31.0).contains(&(p[1].round()))));
        // A quarter of the way: down the right side, about where its middle is.
        let quarter = part_of(&outline, 0.25);
        let end = quarter.last().unwrap();
        assert!((end[0] - 31.0).abs() < 0.01 && (end[1] - 16.0).abs() < 1.0, "{end:?}");
        assert_eq!(part_of(&outline, 1.0).len(), outline.len());
        assert_eq!(part_of(&outline, 0.0).len(), 2);
    }

    #[test]
    fn a_load_reads_its_numbers_and_its_warning() {
        let v = Vitals { cpu: Some(0.38), memory: 0.93, disk: Some((0.44, None)), ..Vitals::default() };
        let load = Load::of(&v, vec![0.1, 0.38]);
        assert_eq!(load.summary(), "CPU 38% · Memory 93% · Disk 44%");
        assert_eq!(load.warning.as_deref(), Some("Memory 93% used"));
        // No second look yet: no CPU, nothing made up.
        assert_eq!(Load::of(&Vitals::default(), vec![]).cpu, 0.0);
    }
}
