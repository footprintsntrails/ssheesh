//! The text field: one line on a recessed surface, with the blue ring while it has the
//! keyboard (the design's `.field`). Rune's `text_input` does the editing; this is its look.
//!
//! ```ignore
//! field(t, input(t, &s.name, "Name").key("name").on_edit(|s: &mut Shell| &mut s.name), false)
//! ```

use rune::{text_input, El, TextEdit};

use crate::theme::{self, Theme};

/// A one-line `text_input` in the interface's text, showing `hint` while empty. The caller
/// adds its key, `.on_edit(..)` and the handlers, then wraps it in [`field`].
pub fn input(t: &Theme, edit: &TextEdit, hint: &str) -> El {
    styled(t, text_input(edit, theme::TEXT), hint)
}

/// [`input`] in the terminal's font: a path, a command.
pub fn code_input(t: &Theme, edit: &TextEdit, hint: &str) -> El {
    styled(t, text_input(edit, theme::CODE), hint)
}

fn styled(t: &Theme, input: El, hint: &str) -> El {
    input.single_line().grow(1.0).shrink().min_w(0.0).color(t.l1).placeholder(hint, t.l2).caret(t.ring, t.ring.alpha(0.35)).label(hint)
}

/// The field's surface around an [`input`]. `bad`: what it holds was refused, so its edge and
/// ring are red.
pub fn field(t: &Theme, input: El, bad: bool) -> El {
    rune::row()
        .h(theme::FIELD)
        .px(8.0)
        .gap(6.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.field())
        .border(1.0, if bad { t.red } else { t.hair })
        .focus_within_ring(theme::RING, 0.0, if bad { t.red } else { t.ring })
        .items_center()
        .child(input)
}

/// A quiet label over a field: "Address", "Port".
pub fn labelled(t: &Theme, label: &str, field: El) -> El {
    rune::col().gap(4.0).child(rune::text(label).font(theme::SMALL).color(t.l2).single_line().a11y_hidden()).child(field)
}
