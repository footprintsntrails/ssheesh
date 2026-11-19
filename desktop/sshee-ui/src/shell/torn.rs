//! The card under the pointer while a tab is dragged past the window's edge (`tabs.rs`): the
//! window can't draw outside itself, so the card is a Rune overlay of the window's own, over
//! other windows, letting every click through. It shows the tab's icon, name and machine, and
//! that letting go opens a window. Where the system hides where the pointer is (Wayland: no
//! `Drag::screen_point`) there is no card; letting go still opens the window.

use std::time::Duration;

use rune::{row, text, App, Cx, El, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec};
use sshee_app::workbench::TabId;

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// The card's size, in points.
const SIZE: (f32, f32) = (240.0, 40.0);

/// How far from the pointer the card's corner is: beside it, not under it.
const BESIDE: (f32, f32) = (14.0, 10.0);

/// What the card shows.
#[derive(Clone, Default)]
pub struct Card {
    icon: Option<Icon>,
    name: String,
    /// The window's themes, dark and light: the card follows the system as the window does.
    looks: Option<(&'static Theme, &'static Theme)>,
}

/// The card's overlay and where it is.
#[derive(Default)]
pub(crate) struct Torn {
    overlay: Option<rune::OverlayHandle<Card>>,
    /// The tab whose card is shown, and the window's scale when it was (desktop pixels per
    /// point).
    shown: Option<(TabId, f64)>,
}

/// The card's overlay, for `app`'s window: hidden until a tab goes past the edge, its
/// surface given back a minute after the card was last shown.
pub(crate) fn overlay(app: &mut App<Shell>) -> rune::OverlayHandle<Card> {
    let options = OverlayOptions { placement: Some(OverlayPlacement::Rect { x: 0.0, y: 0.0, width: SIZE.0, height: SIZE.1 }), input: OverlayInput::None, ..OverlayOptions::default() };
    app.overlay(OverlaySpec::new("SSHeesh tab", SIZE, card).options(options).release_gpu_after(Duration::from_secs(60)), Card::default())
}

fn card(c: &Card, cx: &mut Cx<Card>) -> El {
    let Some((dark, light)) = c.looks else { return rune::el() };
    let t = if cx.env().system_dark { dark } else { light };
    let mut e = row().key("torn-tab").w(SIZE.0).h(SIZE.1).px(12.0).gap(8.0).items_center().radius(theme::RADIUS).fill(t.pop).border(0.5, t.rim);
    if let Some(glyph) = c.icon {
        e = e.child(icon(glyph, theme::ICON, t.l1));
    }
    e.child(text(&c.name).font(theme::TAB_NAME).color(t.l1).single_line().truncate_fade().shrink())
}

impl Shell {
    /// The window's card overlay, once Rune made it (`shell::window_app`).
    pub(crate) fn set_torn_overlay(&mut self, overlay: rune::OverlayHandle<Card>) {
        self.torn.overlay = Some(overlay);
    }

    /// Tab `id` is dragged past the window's edge with the pointer at `screen` (desktop
    /// pixels): its card follows the pointer. `None`: back inside, let go, or nowhere known:
    /// the card goes.
    pub(crate) fn torn_card(&mut self, id: TabId, screen: Option<(i32, i32)>) {
        let Some(overlay) = self.torn.overlay.clone() else { return };
        let Some(screen) = screen else {
            if self.torn.shown.take().is_some() {
                overlay.hide();
            }
            return;
        };
        let scale = match self.torn.shown {
            Some((shown, scale)) if shown == id => scale,
            _ => {
                let Some(scale) = self.window.as_ref().and_then(|w| w.placement()).map(|p| p.scale) else { return };
                let Some(tab) = self.bench.tab(id) else { return };
                let server = self.bench.server(&tab.server).map(|sv| self.server_shown(&tab.server, &sv.name).into_owned()).unwrap_or_default();
                let label = self.text_shown(&sshee_app::agents::tab_label(tab)).into_owned();
                let name = if server.is_empty() { label } else { format!("{label} · {server}") };
                let (icon, looks) = (super::tabs::tab_icon(tab), (self.theme(true), self.theme(false)));
                overlay.update(move |c, _| *c = Card { icon: Some(icon), name, looks: Some(looks) });
                overlay.show();
                self.torn.shown = Some((id, scale));
                scale
            }
        };
        let (x, y) = card_at(screen, scale);
        overlay.set_placement(Some(OverlayPlacement::Rect { x, y, width: SIZE.0, height: SIZE.1 }));
    }
}

/// The card's top-left, in the points overlays are placed in, for the pointer at `screen`
/// (desktop pixels) on a display of `scale`: beside the pointer.
pub(crate) fn card_at(screen: (i32, i32), scale: f64) -> (f32, f32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    ((f64::from(screen.0) / scale) as f32 + BESIDE.0, (f64::from(screen.1) / scale) as f32 + BESIDE.1)
}
