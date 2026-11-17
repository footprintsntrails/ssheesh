//! A note's text as each system's notifications read it. The text comes from a terminal (what
//! a program printed, a server's name), so nothing in it may become markup: a link, an image,
//! a toast's own XML.

/// For a freedesktop server that reads `body-markup`: `&`, `<` and `>` as entities (the spec's
/// markup is a small XML subset). A server without markup shows the text as it is.
pub fn freedesktop_body(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// For a Windows toast's XML: text and attribute values alike. Control characters other than
/// tab and line breaks aren't allowed in XML at all, so they go.
pub fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One line of at most `max` characters: notification centres cut long text anyway, and a
/// terminal's line can be very long.
pub fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().nth(max).is_some() {
        out.pop();
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_stays_text() {
        assert_eq!(freedesktop_body("<a href=\"x\">a & b</a>"), "&lt;a href=\"x\"&gt;a &amp; b&lt;/a&gt;");
        assert_eq!(xml("</text><action content=\"x\"/>"), "&lt;/text&gt;&lt;action content=&quot;x&quot;/&gt;");
        assert_eq!(xml("it's\u{1b}[0m fine\nnext"), "it&apos;s[0m fine\nnext");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("0123456789", 10), "0123456789");
        assert_eq!(clip("0123456789abc", 10), "012345678…");
        assert_eq!(clip("ééééé", 3), "éé…");
    }
}
