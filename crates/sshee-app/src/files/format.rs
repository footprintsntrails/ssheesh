//! How sizes, speeds and dates read in Files. A port of `bytes`, `rate`, `percent` and
//! `modified` from 0.10's src/lib/format.ts, with the clock passed in.

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// 1536 → "1.5 KB". One decimal under 10, none above.
pub fn bytes(n: u64) -> String {
    amount(n as f64)
}

/// A speed: "1.5 MB/s".
pub fn rate(bytes_per_sec: f64) -> String {
    format!("{}/s", amount(bytes_per_sec))
}

/// A share of the whole: 0.4 → "40%". Out of range reads as the nearest end.
pub fn percent(fraction: f64) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round())
}

fn amount(n: f64) -> String {
    if !n.is_finite() || n <= 0.0 {
        return "0 B".into();
    }
    let mut i = 0;
    while i < UNITS.len() - 1 && n >= 1024f64.powi(i as i32 + 1) {
        i += 1;
    }
    // "1020 KB" reads worse than "1.0 MB".
    if n / 1024f64.powi(i as i32) >= 1000.0 && i < UNITS.len() - 1 {
        i += 1;
    }
    let v = n / 1024f64.powi(i as i32);
    if v < 10.0 && i > 0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{} {}", v.round(), UNITS[i])
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The date of a day counted from 1970-01-01: year, month (1 to 12), day. Howard Hinnant's
/// `civil_from_days`, by hand so this crate needs no date library for one list column.
fn civil(days: i64) -> (i64, usize, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as usize, day)
}

/// Finder-style dates: "Just now", "12 min ago", "Today 14:02", "Yesterday 09:15", "Oct 12",
/// "Oct 12, 2023"; "—" when the machine gave no time.
///
/// `unix` is the file's time and `now` the moment the listing was taken, both in Unix seconds;
/// `utc_offset` is how far this computer was from UTC then, in seconds east. Days are this
/// computer's days. One offset serves every date: a file from before the clocks changed may
/// show a time an hour off, and the wrong day only if it was changed within that hour of
/// midnight.
pub fn modified_text(unix: i64, now: i64, utc_offset: i32) -> String {
    if unix == 0 {
        return "—".into();
    }
    let ago = now - unix;
    // A clock a little ahead on the server: its newest files are "Just now" too.
    if ago < 60 {
        return "Just now".into();
    }
    if ago < 3_600 {
        return format!("{} min ago", ago / 60);
    }
    let local = unix + i64::from(utc_offset);
    let (day, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let today = (now + i64::from(utc_offset)).div_euclid(86_400);
    let time = format!("{:02}:{:02}", secs / 3_600, secs % 3_600 / 60);
    if day == today {
        return format!("Today {time}");
    }
    if day == today - 1 {
        return format!("Yesterday {time}");
    }
    let (year, month, date) = civil(day);
    if year == civil(today).0 {
        format!("{} {date}", MONTHS[month - 1])
    } else {
        format!("{} {date}, {year}", MONTHS[month - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_in_0_10() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1), "1 B");
        assert_eq!(bytes(999), "999 B");
        // From 1000 up the next unit reads better, as in 0.10.
        assert_eq!(bytes(1023), "1.0 KB");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(10 * 1024), "10 KB");
        assert_eq!(bytes(999 * 1024), "999 KB");
        // "1020 KB" reads worse than "1.0 MB".
        assert_eq!(bytes(1020 * 1024), "1.0 MB");
        assert_eq!(bytes(1_048_576), "1.0 MB");
        assert_eq!(bytes(5 * 1_073_741_824 + 300 * 1_048_576), "5.3 GB");
        assert_eq!(bytes(u64::MAX), "16384 PB");
        assert_eq!(rate(315.0 * 1024.0), "315 KB/s");
        assert_eq!(rate(0.0), "0 B/s");
        assert_eq!(rate(f64::NAN), "0 B/s");
        assert_eq!((percent(0.404), percent(1.7), percent(-1.0)), ("40%".into(), "100%".into(), "0%".into()));
    }

    /// 2024-10-12 14:02:00 UTC.
    const NOON: i64 = 1_728_741_720;

    #[test]
    fn days_since_1970_are_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(NOON.div_euclid(86_400)), (2024, 10, 12));
        // A leap day, the day after it, and the turn of a century that isn't one.
        assert_eq!(civil(19_782), (2024, 2, 29));
        assert_eq!(civil(19_783), (2024, 3, 1));
        assert_eq!(civil(47_540), (2100, 2, 28));
        assert_eq!(civil(47_541), (2100, 3, 1));
    }

    #[test]
    fn dates_read_as_in_finder() {
        let at = |unix: i64| modified_text(unix, NOON, 0);
        assert_eq!(at(0), "—");
        assert_eq!(at(NOON - 5), "Just now");
        assert_eq!(at(NOON + 30), "Just now");
        assert_eq!(at(NOON - 60), "1 min ago");
        assert_eq!(at(NOON - 12 * 60 - 59), "12 min ago");
        assert_eq!(at(NOON - 3_600), "Today 13:02");
        assert_eq!(at(NOON - 14 * 3_600), "Today 00:02");
        assert_eq!(at(NOON - 14 * 3_600 - 180), "Yesterday 23:59");
        assert_eq!(at(NOON - 86_400 - 5 * 3_600 + 13 * 60), "Yesterday 09:15");
        assert_eq!(at(NOON - 2 * 86_400), "Oct 10");
        assert_eq!(at(NOON - 300 * 86_400), "Dec 17, 2023");
        assert_eq!(at(1_697_112_000), "Oct 12, 2023");
    }

    #[test]
    fn days_are_this_computers_days() {
        // 23:30 UTC the day before is 02:30 today three hours east, and still yesterday in UTC.
        let late = NOON - 14 * 3_600 - 32 * 60;
        assert_eq!(modified_text(late, NOON, 0), "Yesterday 23:30");
        assert_eq!(modified_text(late, NOON, 3 * 3_600), "Today 02:30");
        // West of UTC the year can still be the old one.
        let new_year = 1_735_689_600 + 3_600; // 2025-01-01 01:00 UTC
        assert_eq!(modified_text(new_year - 2 * 86_400, new_year, -5 * 3_600), "Dec 29");
        assert_eq!(modified_text(new_year - 2 * 86_400, new_year + 86_400 * 2, 0), "Dec 30, 2024");
    }
}
