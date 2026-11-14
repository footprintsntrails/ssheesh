//! The tile: a square with one icon over a name, as a browser's new tab lists sites. The main
//! menu's servers are tiles; "Add a server" is one with a dashed edge.

use rune::{text, Color, El};

use crate::icons::{icon, Icon};
use crate::kit::chip::chip;
use crate::kit::menu::key_cap;
use crate::kit::ringed_dot;
use crate::theme::{self, Theme};

/// A tile's width, and its icon square's side.
pub const TILE_WIDTH: f32 = 104.0;
pub const TILE_ICON: f32 = 48.0;

/// What a tile shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile<'a> {
    pub icon: Option<Icon>,
    pub name: &'a str,
    /// A label under the name, and whether it's the red one ("Prod"); the others are quiet.
    pub label: Option<(&'a str, bool)>,
    /// A faint wash of the server's own colour over the square.
    pub tint: Option<Color>,
    /// The key that opens it, in a cap at the tile's corner.
    pub key: Option<&'a str>,
    /// The orange "needs you" dot.
    pub waiting: bool,
    /// How many tabs are open on it; 0 shows nothing.
    pub open: usize,
    /// The keyboard's highlight is on it.
    pub focused: bool,
    /// "Add a server": a dashed edge and no fill.
    pub dashed: bool,
}

/// The tile. The caller adds its key and `.on_press_tx(..)`.
pub fn tile(t: &Theme, spec: &Tile) -> El {
    let mut square = rune::el().size(TILE_ICON, TILE_ICON).radius(12.0).items_center().justify_center();
    square = if spec.dashed { square.border(1.0, t.hair).border_dash(4.0, 3.0) } else { square.fill(spec.tint.map_or(t.hov2, |c| c.alpha(0.16))).border(1.0, t.rim) };
    if let Some(glyph) = spec.icon {
        square = square.child(icon(glyph, 22.0, if spec.dashed { t.l2 } else { t.l2on }));
    }
    if spec.waiting {
        square = square.child(ringed_dot(8.0, 2.0, t.orange_dot, t.page).key("waits").absolute(TILE_ICON - 9.0, -3.0).label_static("Needs you"));
    } else if spec.open > 0 {
        let count = spec.open.to_string();
        square = square.child(
            rune::el()
                .key("open")
                .absolute(TILE_ICON - 11.0, TILE_ICON - 11.0)
                .h(16.0)
                .min_w(16.0)
                .px(4.0)
                .radius(8.0)
                .fill(t.pop)
                .border(1.0, t.rim)
                .items_center()
                .justify_center()
                .label(format!("{count} open"))
                .child(text(count).font(theme::STATUS_VALUE).color(t.l2on).single_line().a11y_hidden()),
        );
    }
    let name = rune::row()
        .max_w(TILE_WIDTH - 8.0)
        .justify_center()
        .child(text(spec.name).key("name").font(theme::NOTE).color(if spec.dashed { t.l2 } else { t.l1 }).single_line().truncate().shrink().min_w(0.0));
    let mut e = rune::col()
        .w(TILE_WIDTH)
        .pt(14.0)
        .pb(10.0)
        .px(4.0)
        .gap(8.0)
        .radius(14.0)
        .items_center()
        .hover(t.hov)
        .press(t.down)
        .role("button")
        .label(spec.name)
        .child(square)
        .child(name);
    if let Some((label, red)) = spec.label {
        e = e.child(chip(t, label, if red { t.red } else { t.l2 }).key("env").mt(-3.0));
    }
    if let Some(key) = spec.key {
        e = e.child(key_cap(t, key, false).key("key").absolute(7.0, 7.0));
    }
    if spec.focused {
        e = e.outline(theme::RING, 0.0, t.ring);
    }
    e
}
