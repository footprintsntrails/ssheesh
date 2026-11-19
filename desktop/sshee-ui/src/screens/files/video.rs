//! A video in Files' preview, as 0.10 played one: the picture, and under it play and pause,
//! the time with a bar to seek along, and the sound (on/off, and how loud). Space plays and
//! pauses; a click on the picture does too. sshee-media plays it on a thread of its own (its
//! sound included); its pictures land on a Rune `NativeSurface`, which the GPU draws.
//!
//! The player lives as long as the preview shows the file: closing the preview, opening
//! another file or closing the tab drops it, which stops it. A tab put out of sight pauses it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rune::native_surface::{ByteFrame, ColorInfo, Format, Matrix, Transfer};
use rune::{el, row, text, Align, El, Phase, SurfaceFit, Tx};
use sshee_app::files::VideoFile;
use sshee_app::workbench::TabId;
use sshee_media::{Cursor, Options, Picture, Player, Screen, Source, Status, View};

use crate::icons::Icon;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// A video open in a Files tab's preview.
pub struct Playing {
    file: VideoFile,
    player: Player,
    surface: rune::NativeSurface,
    /// What the screen last drew of it.
    drawn: Option<View>,
}

/// The open file, read where the player asks.
struct Reading(VideoFile);

impl Source for Reading {
    fn size(&self) -> u64 {
        self.0.size()
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0 .0.read_at(at, buf)
    }
}

/// Where the player's pictures go: the surface, which wakes the window for each.
struct OnScreen {
    surface: rune::NativeSurface,
    waker: rune::Waker,
    frames: AtomicU64,
}

impl Screen for OnScreen {
    fn show(&self, p: Picture) {
        // Brightness and colour as video has them (16–235); HD pictures by BT.709's sums,
        // smaller ones by BT.601's, as players guess when a file doesn't say.
        let matrix = if p.height >= 720 { Matrix::Bt709 } else { Matrix::Bt601 };
        let frame = ByteFrame { width: p.width, height: p.height, format: Format::Nv12Video, color: ColorInfo { matrix, transfer: Transfer::Bt709 }, planes: [p.y, p.uv] };
        let id = self.frames.fetch_add(1, Ordering::Relaxed);
        let _ = self.surface.push_bytes(frame, id);
    }
    fn changed(&self) {
        (self.waker)();
    }
}

impl Playing {
    /// Starts reading `file`: its first picture shows, paused.
    pub fn open(file: &VideoFile, waker: rune::Waker) -> Playing {
        let surface = rune::NativeSurface::new();
        surface.set_waker(waker.clone());
        let screen = Arc::new(OnScreen { surface: surface.clone(), waker, frames: AtomicU64::new(0) });
        let reading = Cursor::new(Arc::new(Reading(file.clone())));
        // Tests make no sound.
        let player = Player::open(Box::new(reading), screen, Options { sound: !cfg!(test) });
        Playing { file: file.clone(), player, surface, drawn: None }
    }

    /// The player moved on since the last look (`files::tick`, each frame): its state, the
    /// second it's at, its size or sound. The page is drawn again for it.
    pub fn moved_on(&mut self) -> bool {
        let now = self.player.view();
        let seen = |v: &View| (v.status, (v.time * 4.0) as i64, v.size, v.duration.map(|d| (d * 1000.0) as i64), v.sound, v.muted, (v.volume * 100.0) as i32);
        let news = self.drawn.as_ref().is_none_or(|d| seen(d) != seen(&now));
        self.drawn = Some(now);
        news
    }

    /// It plays `file` (the same opening).
    pub fn plays(&self, file: &VideoFile) -> bool {
        self.file == *file
    }

    pub fn view(&self) -> View {
        self.player.view()
    }

    pub fn toggle(&self) {
        self.player.toggle();
    }

    pub fn pause(&self) {
        if self.player.view().status == Status::Playing {
            self.player.pause();
        }
    }

    #[cfg(test)]
    pub fn player(&self) -> &Player {
        &self.player
    }
}

/// "1:05", "12:40", "1:02:03".
pub fn clock(seconds: f64) -> String {
    let s = if seconds.is_finite() { seconds.max(0.0) as u64 } else { 0 };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The video's tab's player, to act on.
fn playing(s: &Shell, id: TabId) -> Option<&Playing> {
    s.screens.files.get(&id)?.ui.video()
}

/// The knob of a bar, the slider's, centred on the right edge of what holds it.
fn knob(t: &Theme) -> El {
    let k = theme::SLIDER_KNOB;
    el().pin(None, Some(0.0), Some(-k / 2.0), None)
        .size(k, k)
        .radius(k / 2.0)
        .fill(t.knob)
        .border(0.5, rune::Color::BLACK.alpha(0.12))
        .shadow(1.0, 2.5, rune::Color::BLACK.alpha(0.28))
        .a11y_hidden()
}

/// A bar along which a value from 0 to 1 is dragged (the time, the volume): the slider's
/// track and knob, as wide as it's given. `on_change` gets the fraction as it's dragged;
/// the arrow keys step it by `step`.
fn bar(t: &Theme, key: &'static str, name: &str, shown: String, fraction: f64, step: f64, on_change: impl Fn(&mut Shell, f64, &mut Tx) + Clone + 'static) -> El {
    let k = theme::SLIDER_KNOB;
    let line = theme::SLIDER_TRACK;
    let f = fraction.clamp(0.0, 1.0);
    let (drag, keys) = (on_change.clone(), on_change);
    el().key(key)
        .h(k)
        .mx(k / 2.0)
        .role("slider")
        .label(name)
        .a11y_value(shown)
        .range_value(f, 0.0, 1.0)
        .focusable(true)
        .child(el().absolute(0.0, (k - line) / 2.0).w_full().h(line).radius(line / 2.0).fill(t.control).a11y_hidden())
        .child(el().absolute(0.0, (k - line) / 2.0).w_frac(f as f32).h(line).radius(line / 2.0).fill(t.l2).a11y_hidden())
        .child(el().absolute(0.0, 0.0).w_frac(f as f32).h(k).a11y_hidden().child(knob(t)))
        .on_drag(move |s: &mut Shell, d, tx| {
            if d.phase == Phase::Up && d.cancelled() {
                return;
            }
            let fraction = f64::from((d.x / d.w.max(1.0)).clamp(0.0, 1.0));
            drag(s, fraction, tx);
        })
        .on_step(move |s: &mut Shell, steps, tx| keys(s, (f + f64::from(steps) * step).clamp(0.0, 1.0), tx))
}

/// The video's picture and its controls, once it has a picture (the preview shows "Loading…"
/// until then, and why when it can't be played).
pub fn view(s: &Shell, id: TabId, t: &'static Theme, p: &Playing, name: &str) -> El {
    let v = p.view();
    let [w, h] = v.size.map_or([16.0, 9.0], |[w, h]| [w as f32, h as f32]);
    let picture = el()
        .key("preview-video-picture")
        .max_w(w)
        .max_h(h)
        .aspect_fit(w / h.max(1.0))
        .native_surface(&p.surface, SurfaceFit::Fit)
        .role("button")
        .label(format!("Video {}", s.text_shown(name)))
        .on_press_tx(move |s: &mut Shell, tx| {
            if let Some(p) = playing(s, id) {
                p.toggle();
            }
            tx.instant();
        });
    let playing_now = v.status == Status::Playing;
    let (glyph, what) = if playing_now { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
    let play = crate::kit::button::toolbar_button(t, "preview-video-play", glyph, what).on_press_tx(move |s: &mut Shell, tx| {
        if let Some(p) = playing(s, id) {
            p.toggle();
        }
        tx.instant();
    });
    let duration = v.duration.unwrap_or(0.0);
    let fraction = if duration > 0.0 { v.time / duration } else { 0.0 };
    let seek = bar(t, "preview-video-seek", "Time", format!("{} of {}", clock(v.time), clock(duration)), fraction, 5.0 / duration.max(5.0), move |s: &mut Shell, f, tx| {
        if let Some(p) = playing(s, id) {
            if let Some(d) = p.view().duration {
                p.player.seek(f * d);
            }
        }
        tx.instant();
    })
    .grow(1.0)
    .min_w(40.0);
    let time = |key: &'static str, secs: f64| text(clock(secs)).key(key).font(theme::LIST_NUMBER).color(t.l2).single_line().align(Align::Center).min_w(34.0);
    let has_sound = v.sound != Some(false);
    let quiet = v.muted || v.volume <= 0.0;
    let (sound_glyph, sound_name) = match (has_sound, quiet) {
        (false, _) => (Icon::Muted, "This video has no sound"),
        (true, true) => (Icon::Muted, "Turn the sound on"),
        (true, false) => (Icon::Sound, "Mute"),
    };
    let mut mute = crate::kit::button::toolbar_button(t, "preview-video-mute", sound_glyph, sound_name);
    if has_sound {
        mute = mute.on_press_tx(move |s: &mut Shell, tx| {
            if let Some(p) = playing(s, id) {
                let v = p.view();
                if v.muted || v.volume <= 0.0 {
                    p.player.set_muted(false);
                    if v.volume <= 0.0 {
                        p.player.set_volume(1.0);
                    }
                } else {
                    p.player.set_muted(true);
                }
            }
            tx.instant();
        });
    } else {
        mute = mute.opacity(0.35).disabled(true).focusable(false);
    }
    let level = if v.muted { 0.0 } else { f64::from(v.volume) };
    let mut controls = row()
        .key("preview-video-controls")
        .w_full()
        .h(36.0)
        .px(6.0)
        .gap(6.0)
        .items_center()
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hair2)
        .child(play)
        .child(time("preview-video-time", v.time))
        .child(seek)
        .child(time("preview-video-duration", duration))
        .child(mute);
    if has_sound {
        controls = controls.child(
            bar(t, "preview-video-volume", "Volume", format!("{}%", (level * 100.0).round()), level, 0.1, move |s: &mut Shell, f, tx| {
                if let Some(p) = playing(s, id) {
                    p.player.set_volume(f as f32);
                }
                tx.instant();
            })
            .w(64.0)
            .mr(6.0),
        );
    }
    rune::col()
        .key("preview-video")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(row().w_full().grow(1.0).min_h(0.0).padding(14.0).items_center().justify_center().child(picture))
        .child(controls)
        .child(
            row()
                .key("preview-video-foot")
                .w_full()
                .h(24.0)
                .px(12.0)
                .items_center()
                .border_sides([1.0, 0.0, 0.0, 0.0], t.hair2)
                .children(v.size.map(|[w, h]| text(format!("{w} × {h}")).font(theme::LIST_NUMBER).color(t.l2).single_line())),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_a_player_shows_them() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(760.0), "12:40");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(f64::NAN), "0:00");
        assert_eq!(clock(-3.0), "0:00");
    }
}
