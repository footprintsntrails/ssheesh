//! The tray: its menu, its tooltip, its icon, and what a pick means. The icon itself is drawn
//! by whoever hosts it ([`TrayHost`]): Rune's tray on Windows and Linux (`sshee-ui`'s
//! `shell::desktop`), the menu bar item in [`crate::macos`]. Both give back the `id` of the item
//! picked, and [`route`] turns it into an [`Event`].
//!
//! ```text
//! Hide SSHeesh Preview          (Show … while the window is hidden)
//! ─────────────
//! web-7731 needs you            one line per tab that waits, its server's name as the window
//! db-0412: Terminal 2 needs you shows it (dotted while streaming)
//! ─────────────
//! Quit SSHeesh Preview
//! ```

use crate::Event;

/// A tab that needs you, as the tray lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub tab: u64,
    /// Its server's name as the window shows it: an alias while streaming.
    pub server: String,
    /// The tab's own name ("Terminal 2", "Claude Code").
    pub what: String,
}

/// One line of the tray's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// `id` comes back when it's picked.
    Action { id: String, label: String, enabled: bool },
    Separator,
}

impl Item {
    pub fn action(id: impl Into<String>, label: impl Into<String>) -> Item {
        Item::Action { id: id.into(), label: label.into(), enabled: true }
    }

    fn note(label: impl Into<String>) -> Item {
        Item::Action { id: String::new(), label: label.into(), enabled: false }
    }
}

/// Draws the tray and tells it its menu. A pick is reported to whoever made it, as the
/// item's id, for [`route`].
pub trait TrayHost {
    fn set_menu(&self, items: Vec<Item>);
    fn set_tooltip(&self, tooltip: String);
}

/// The most tabs named in the menu; the rest are counted.
pub const LISTED: usize = 8;

/// The menu, for the window `visible` or not and the tabs that wait.
pub fn menu(app: &str, visible: bool, waiting: &[Waiting]) -> Vec<Item> {
    let mut items = vec![if visible { Item::action("hide", format!("Hide {app}")) } else { Item::action("show", format!("Show {app}")) }];
    if !waiting.is_empty() {
        items.push(Item::Separator);
        for w in waiting.iter().take(LISTED) {
            // A server with one waiting tab is named alone; with more, each tab is.
            let alone = waiting.iter().filter(|o| o.server == w.server).count() == 1;
            let label = if alone { format!("{} needs you", w.server) } else { format!("{}: {} needs you", w.server, w.what) };
            items.push(Item::action(format!("tab:{}", w.tab), label));
        }
        if waiting.len() > LISTED {
            items.push(Item::note(format!("{} more need you", waiting.len() - LISTED)));
        }
    }
    items.push(Item::Separator);
    items.push(Item::action("quit", format!("Quit {app}")));
    items
}

/// What the icon says under the pointer.
pub fn tooltip(app: &str, waiting: &[Waiting]) -> String {
    match waiting {
        [] => app.to_owned(),
        [one] => format!("{app}: {} needs you", one.server),
        many => format!("{app}: {} terminals need you", many.len()),
    }
}

/// What picking the item `id` asks of the window. `token`: the activation token the tray host
/// handed over, if any.
pub fn route(id: &str, token: Option<String>) -> Option<Event> {
    match id {
        "show" => Some(Event::Show { token }),
        "hide" => Some(Event::Hide),
        "quit" => Some(Event::Quit),
        _ => {
            let n = id.strip_prefix("tab:")?;
            n.bytes().all(|b| b.is_ascii_digit()).then(|| n.parse().ok()).flatten().map(|tab| Event::OpenTab { tab, token })
        }
    }
}

/// The app's icon at 32 points, for a tray that draws pixels (Windows, Linux).
pub const ICON_PNG: &[u8] = include_bytes!("../../../src-tauri/icons/32x32.png");

/// A PNG's pixels as straight RGBA rows: `(width, height, bytes)`.
pub fn rgba(png_bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.as_chunks::<2>().0.iter().flat_map(|[g, a]| [*g, *g, *g, *a]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, pixels))
}

/// The logo's face (`>_<`) alone, black on clear, `width × height` pixels: the macOS menu bar
/// wants a template image it tints itself, not the coloured icon. Drawn from the logo's own
/// geometry (app-icon.svg's 200-unit grid), four by four samples a pixel.
pub fn template(width: u32, height: u32) -> Vec<u8> {
    // The face's box on the logo's grid, with room for the strokes.
    let (x0, y0, x1, y1) = (16.0_f32, 70.0_f32, 184.0_f32, 166.0_f32);
    let scale = ((x1 - x0) / width as f32).max((y1 - y0) / height as f32);
    let (dx, dy) = (x0 + ((x1 - x0) - scale * width as f32) / 2.0, y0 + ((y1 - y0) - scale * height as f32) / 2.0);
    let near = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (vx, vy, wx, wy) = (b.0 - a.0, b.1 - a.1, p.0 - a.0, p.1 - a.1);
        let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        let (ex, ey) = (wx - t * vx, wy - t * vy);
        (ex * ex + ey * ey).sqrt()
    };
    let inside = |p: (f32, f32)| {
        let stroke = 10.0;
        let left = near(p, (32.0, 86.0), (61.0, 112.0)).min(near(p, (61.0, 112.0), (32.0, 138.0)));
        let right = near(p, (168.0, 86.0), (139.0, 112.0)).min(near(p, (139.0, 112.0), (168.0, 138.0)));
        let mouth = (76.0..=124.0).contains(&p.0) && (140.0..=160.0).contains(&p.1);
        left <= stroke || right <= stroke || mouth
    };
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = (dx + (x as f32 + (sx as f32 + 0.5) / 4.0) * scale, dy + (y as f32 + (sy as f32 + 0.5) / 4.0) * scale);
                    hits += inside(p) as u32;
                }
            }
            out.extend([0, 0, 0, (hits * 255 / 16) as u8]);
        }
    }
    out
}

/// RGBA rows as a PNG file's bytes (the menu bar item's image).
pub fn png_of(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(rgba);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(tab: u64, server: &str, what: &str) -> Waiting {
        Waiting { tab, server: server.into(), what: what.into() }
    }

    fn labels(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                Item::Action { label, enabled: true, .. } => label.clone(),
                Item::Action { label, enabled: false, .. } => format!("({label})"),
                Item::Separator => "---".into(),
            })
            .collect()
    }

    #[test]
    fn nothing_waits_show_or_hide_and_quit() {
        assert_eq!(labels(&menu("SSHeesh Preview", true, &[])), ["Hide SSHeesh Preview", "---", "Quit SSHeesh Preview"]);
        assert_eq!(labels(&menu("SSHeesh Preview", false, &[])), ["Show SSHeesh Preview", "---", "Quit SSHeesh Preview"]);
        assert_eq!(tooltip("SSHeesh Preview", &[]), "SSHeesh Preview");
    }

    #[test]
    fn waiting_tabs_are_named_by_server_and_by_tab_when_a_server_has_more() {
        let waiting = [w(3, "web-7731", "Claude Code"), w(5, "db-0412", "Terminal"), w(6, "db-0412", "Terminal 2")];
        assert_eq!(
            labels(&menu("SSHeesh Preview", true, &waiting)),
            ["Hide SSHeesh Preview", "---", "web-7731 needs you", "db-0412: Terminal needs you", "db-0412: Terminal 2 needs you", "---", "Quit SSHeesh Preview"]
        );
        assert_eq!(tooltip("SSHeesh Preview", &waiting[..1]), "SSHeesh Preview: web-7731 needs you");
        assert_eq!(tooltip("SSHeesh Preview", &waiting), "SSHeesh Preview: 3 terminals need you");
    }

    #[test]
    fn a_long_list_is_cut_and_counted() {
        let waiting: Vec<Waiting> = (0..11).map(|i| w(i, &format!("Server {i}"), "Terminal")).collect();
        let items = menu("SSHeesh Preview", true, &waiting);
        assert_eq!(items.iter().filter(|i| matches!(i, Item::Action { id, .. } if id.starts_with("tab:"))).count(), LISTED);
        assert!(labels(&items).contains(&"(3 more need you)".to_owned()));
    }

    #[test]
    fn every_item_of_the_menu_routes_to_its_event() {
        let menu = menu("SSHeesh Preview", false, &[w(42, "web-7731", "Terminal")]);
        let routed: Vec<Option<Event>> = menu.iter().filter_map(|i| if let Item::Action { id, .. } = i { Some(route(id, None)) } else { None }).collect();
        assert_eq!(routed, [Some(Event::Show { token: None }), Some(Event::OpenTab { tab: 42, token: None }), Some(Event::Quit)]);
        assert_eq!(route("hide", None), Some(Event::Hide));
        assert_eq!(route("tab:7", Some("tok".into())), Some(Event::OpenTab { tab: 7, token: Some("tok".into()) }));
        for odd in ["", "tab:", "tab:-1", "tab:+1", "tab:1x", "Tab:1", "open"] {
            assert_eq!(route(odd, None), None, "{odd}");
        }
    }

    #[test]
    fn the_icon_reads_and_the_template_is_the_face_alone() {
        let (w, h, px) = rgba(ICON_PNG).unwrap();
        assert_eq!((w, h, px.len()), (32, 32, 32 * 32 * 4));
        let t = template(64, 36);
        assert_eq!(t.len(), 64 * 36 * 4);
        // Black everywhere, see-through where there's no stroke: the corners are clear, the
        // middle of the mouth is solid.
        assert!(t.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]));
        assert_eq!(t[3], 0);
        let alpha = |x: u32, y: u32| t[((y * 64 + x) * 4 + 3) as usize];
        assert_eq!(alpha(32, 32), 255, "the mouth");
        assert_eq!(alpha(32, 10), 0, "between the eyes");
        let png = png_of(64, 36, &t);
        assert_eq!(rgba(&png).unwrap(), (64, 36, t));
    }
}
