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
pub const TILE_ICON: f32 = 56.0;
/// The icon square's corner: an app icon's share of its side.
const SQUARE_RADIUS: f32 = 14.0;

/// What a tile shows.
#[derive(Default)]
pub struct Tile<'a> {
    pub icon: Option<Icon>,
    /// A drawing of the caller's in the icon's place, 22 points square: a server's picture, or
    /// its icon in its own colour.
    pub glyph: Option<El>,
    pub name: &'a str,
    /// A label under the name, in its colour (`kit::chip::env_color`): "Prod" red, "Staging"
    /// amber, as the sidebar, the title and a split's header draw it.
    pub label: Option<(&'a str, Color)>,
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
pub fn tile(t: &Theme, spec: Tile) -> El {
    let mut square = rune::el().size(TILE_ICON, TILE_ICON).radius(SQUARE_RADIUS).items_center().justify_center();
    // The server's own colour, washed over its square and edged in a stronger wash of it;
    // without one, the page's quiet grey.
    square = match (spec.dashed, spec.tint) {
        (true, _) => square.border(1.0, t.hair).border_dash(4.0, 3.0),
        (false, Some(c)) => square.fill(c.alpha(if t.dark { 0.2 } else { 0.14 })).border(1.0, c.alpha(0.35)),
        (false, None) => square.fill(t.hov2).border(1.0, t.rim),
    };
    if let Some(drawn) = spec.glyph {
        square = square.child(drawn);
    } else if let Some(glyph) = spec.icon {
        square = square.child(icon(glyph, 24.0, if spec.dashed { t.l2 } else { t.l2on }));
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
        .hover(t.hov2)
        .press(t.down)
        // A tap gives a little, as a tile on a phone does.
        .press_scale(0.97)
        .role("button")
        .label(spec.name)
        .child(square)
        .child(name);
    if let Some((label, color)) = spec.label {
        e = e.child(chip(t, label, color).key("env").mt(-3.0));
    }
    if let Some(key) = spec.key {
        e = e.child(key_cap(t, key, false).key("key").absolute(7.0, 7.0));
    }
    if spec.focused {
        e = e.fill(t.hov).outline(theme::RING, 0.0, t.ring);
    }
    e
}
