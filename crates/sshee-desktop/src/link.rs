//! The link a Windows toast opens when it's clicked: `ssheesh-preview://notification/42`, 42
//! being the tab. Windows starts the program with it, and the running one receives it
//! (Rune's `App::on_open_url`). It comes from outside the app (any program, a web page, can
//! open such a link), so it's read strictly: this one shape, a tab number and nothing else.
//! The worst a crafted link does is bring the window forward.

/// The link for `tab`'s notification.
pub fn for_tab(scheme: &str, tab: u64) -> String {
    format!("{scheme}://notification/{tab}")
}

/// The tab a notification's link names, or `None` for anything else.
pub fn tab_of(scheme: &str, link: &str) -> Option<u64> {
    let (given, rest) = link.trim().split_once("://")?;
    if !given.eq_ignore_ascii_case(scheme) {
        return None;
    }
    let number = rest.strip_prefix("notification/")?;
    let number = number.strip_suffix('/').unwrap_or(number);
    if number.is_empty() || number.len() > 20 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "ssheesh-preview";

    #[test]
    fn a_tab_goes_there_and_back() {
        assert_eq!(for_tab(S, 42), "ssheesh-preview://notification/42");
        assert_eq!(tab_of(S, &for_tab(S, 42)), Some(42));
        assert_eq!(tab_of(S, &for_tab(S, u64::MAX)), Some(u64::MAX));
        // Windows and browsers may add a slash, or shout the scheme.
        assert_eq!(tab_of(S, "ssheesh-preview://notification/42/"), Some(42));
        assert_eq!(tab_of(S, "SSHEESH-PREVIEW://notification/42"), Some(42));
    }

    #[test]
    fn anything_else_is_refused() {
        for link in [
            "",
            "ssheesh-preview://",
            "ssheesh-preview://notification/",
            "ssheesh-preview://notification/-1",
            "ssheesh-preview://notification/4 2",
            "ssheesh-preview://notification/42?x=1",
            "ssheesh-preview://notification/42/../1",
            "ssheesh-preview://notification/99999999999999999999",
            "ssheesh-preview://notification/000000000000000000001",
            "ssheesh-preview://settings/42",
            "ssheesh://notification/42",
            "https://notification/42",
            "notification/42",
        ] {
            assert_eq!(tab_of(S, link), None, "{link}");
        }
    }
}
