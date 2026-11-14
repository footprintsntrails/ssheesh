//! The status bar's items: a quiet label and its value; a value past its threshold turns
//! bold with a warning sign; Tor's round-trip time at the right while Tor is on.

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// "CPU 38%". With `warn`, the whole item is bold primary text after an orange warning sign.
pub fn status_item(t: &Theme, label: &str, value: &str, warn: bool) -> El {
    if warn {
        return rune::row()
            .gap(5.0)
            .items_center()
            .child(icon(Icon::Warning, 12.0, t.orange))
            .child(text(format!("{label} {value}")).font(theme::STATUS_WARN).color(t.l1).single_line());
    }
    rune::row()
        .gap(5.0)
        .items_center()
        .child(text(label).font(theme::STATUS).color(t.l2).single_line())
        .child(text(value).font(theme::STATUS_VALUE).color(t.l2on).single_line())
}

/// "Tor 84 ms" after the green onion; without a time yet, just "Tor".
pub fn status_tor(t: &Theme, latency_ms: Option<u32>) -> El {
    let mut row = rune::row().key("tor-time").gap(6.0).items_center().child(icon(Icon::Tor, 13.0, t.green)).child(text("Tor").font(theme::STATUS).color(t.l2).single_line());
    if let Some(ms) = latency_ms {
        row = row.child(text(format!("{ms} ms")).font(theme::STATUS_VALUE).color(t.l2on).single_line());
    }
    row
}
