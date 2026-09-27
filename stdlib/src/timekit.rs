//! Time & date builtins for Rak (`time_now`, `time_fmt`, `time_parse`, ...).
//!
//! Pure UTC civil-date arithmetic (Howard Hinnant's days-from-civil
//! algorithms) — no external datetime crate.

/// Seconds since the Unix epoch (UTC).
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch (UTC).
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Broken-down UTC civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: i64, // 1-12
    pub day: i64,   // 1-31
    pub hour: i64,  // 0-23
    pub minute: i64,
    pub second: i64,
    /// 0 = Sunday .. 6 = Saturday.
    pub weekday: i64,
    /// 1-based day of year.
    pub yday: i64,
}

/// days from 1970-01-01 for a civil y/m/d (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// civil (y, m, d) from days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Is `y` a leap year?
fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Epoch seconds -> broken-down UTC time.
pub fn tm_from_unix(ts: i64) -> Civil {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 was a Thursday; shift so Sunday = 0.
    let weekday = (days.rem_euclid(7) + 4).rem_euclid(7);
    let jan1 = days_from_civil(year, 1, 1);
    Civil {
        year,
        month,
        day,
        hour: secs / 3600,
        minute: (secs % 3600) / 60,
        second: secs % 60,
        weekday,
        yday: days - jan1 + 1,
    }
}

/// Broken-down UTC time -> epoch seconds.
pub fn unix_from_civil(c: &Civil) -> i64 {
    days_from_civil(c.year, c.month, c.day) * 86_400 + c.hour * 3600 + c.minute * 60 + c.second
}

/// strftime-lite: %Y %m %d %H %M %S %y %e %T %% — unknown tokens pass through.
pub fn fmt(ts: i64, fmt: &str) -> String {
    let c = tm_from_unix(ts);
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '%' {
            match it.next() {
                Some('Y') => out.push_str(&format!("{:04}", c.year)),
                Some('y') => out.push_str(&format!("{:02}", c.year.rem_euclid(100))),
                Some('m') => out.push_str(&format!("{:02}", c.month)),
                Some('d') => out.push_str(&format!("{:02}", c.day)),
                Some('e') => out.push_str(&format!("{:2}", c.day)),
                Some('H') => out.push_str(&format!("{:02}", c.hour)),
                Some('M') => out.push_str(&format!("{:02}", c.minute)),
                Some('S') => out.push_str(&format!("{:02}", c.second)),
                Some('T') => out.push_str(&format!("{:02}:{:02}:{:02}", c.hour, c.minute, c.second)),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Parse "YYYY-MM-DD", "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DDTHH:MM:SS[Z]"
/// (UTC) into epoch seconds. Anything else is an error naming the problem.
pub fn parse(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let bad = |why: &str| Err::<i64, String>(format!("time_parse: {} in '{}'", why, s));
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t.trim_end_matches('Z'))),
        None => match s.split_once(' ') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        },
    };
    let mut dp = date.split('-');
    let year = dp.next().and_then(|v| v.parse::<i64>().ok()).ok_or("expected YYYY")?;
    let month = dp.next().and_then(|v| v.parse::<i64>().ok()).ok_or("expected MM")?;
    let day = dp.next().and_then(|v| v.parse::<i64>().ok()).ok_or("expected DD")?;
    if dp.next().is_some() || !(1..=12).contains(&month) {
        return bad("invalid month");
    }
    let dim = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap(year) => 29,
        _ => 28,
    };
    if !(1..=dim).contains(&day) {
        return bad("day out of range for month");
    }
    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    if let Some(t) = time {
        let mut tp = t.split(':');
        hour = tp.next().and_then(|v| v.parse::<i64>().ok()).ok_or("expected HH")?;
        minute = tp.next().and_then(|v| v.parse::<i64>().ok()).ok_or("expected MM")?;
        second = tp.next().map(|v| v.parse::<i64>().unwrap_or(0)).unwrap_or(0);
        if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
            return bad("time out of range");
        }
    }
    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_thursday_1970() {
        let c = tm_from_unix(0);
        assert_eq!((c.year, c.month, c.day), (1970, 1, 1));
        assert_eq!(c.weekday, 4); // Thursday
        assert_eq!(c.yday, 1);
        assert_eq!(unix_from_civil(&c), 0);
    }

    #[test]
    fn known_instants() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(tm_from_unix(1_700_000_000).year, 2023);
        let c = tm_from_unix(1_700_000_000);
        assert_eq!((c.month, c.day, c.hour, c.minute, c.second), (11, 14, 22, 13, 20));
        // leap day: 2024-02-29
        let c = tm_from_unix(1_709_164_800);
        assert_eq!((c.month, c.day), (2, 29));
    }

    #[test]
    fn fmt_tokens() {
        assert_eq!(fmt(1_700_000_000, "%Y-%m-%d %H:%M:%S"), "2023-11-14 22:13:20");
        assert_eq!(fmt(1_700_000_000, "%y"), "23");
        assert_eq!(fmt(0, "%T"), "00:00:00");
        assert_eq!(fmt(0, "%%"), "%");
        assert_eq!(fmt(0, "%q"), "%q"); // unknown passes through
    }

    #[test]
    fn parse_roundtrip() {
        let ts = 1_700_000_000;
        let s = fmt(ts, "%Y-%m-%d %H:%M:%S");
        assert_eq!(parse(&s).unwrap(), ts);
        assert_eq!(parse("2023-11-14T22:13:20Z").unwrap(), ts);
        assert_eq!(parse("2023-11-14").unwrap(), parse("2023-11-14 00:00:00").unwrap());
        assert!(parse("2023-13-01").is_err());
        assert!(parse("2023-02-30").is_err());
        assert!(parse("not a date").is_err());
    }
}
