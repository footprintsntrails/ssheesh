//! A text field (the design's `.field`): one line in a soft box, ringed in the focus colour
//! while it has the keyboard. The typing itself is Rune's (`rune::text_input`): selection,
//! the clipboard, input methods.

use rune::{text_input, El, TextEdit};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// What a field shows around its text.
#[derive(Clone, Copy, Debug, Default)]
pub struct Field<'a> {
    /// What it is for, said when it is empty and to a screen reader.
    pub placeholder: &'a str,
    /// An icon before the text (the magnifier of a search field).
    pub lead: Option<Icon>,
    /// Its width in points; `None` fills the room it is given.
    pub width: Option<f32>,
}

/// The field over `edit`, a `TextEdit` in the window's state that `access` finds again.
/// `dress` adds to the field itself what the caller needs: its `.key(..)`, and
/// `.on_input_change(..)`, `.on_input_key(..)` or `.on_submit(..)` to hear of typing.
pub fn text_field<S: 'static>(
    t: &Theme,
    spec: Field,
    edit: &TextEdit,
    access: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static,
    dress: impl FnOnce(El) -> El,
) -> El {
    let input = text_input(edit, theme::TEXT)
        .single_line()
        .color(t.l1)
        .placeholder(spec.placeholder, t.l2)
        .caret(t.ring, t.ring.alpha(0.3))
        .label(spec.placeholder)
        .grow(1.0)
        .min_w(0.0)
        .on_edit(access);
    let mut field = rune::row()
        .h(theme::FIELD)
        .px(8.0)
        .gap(6.0)
        .radius(theme::RADIUS_ROW)
        .fill(if t.dark { t.code } else { t.page })
        .border(1.0, t.hair)
        .items_center()
        .cursor(rune::Cursor::Text)
        .focus_within_ring(theme::RING, 0.0, t.ring);
    field = match spec.width {
        Some(width) => field.w(width),
        None => field.w_full(),
    };
    if let Some(glyph) = spec.lead {
        field = field.child(icon(glyph, 14.0, t.l2));
    }
    field.child(dress(input))
}
