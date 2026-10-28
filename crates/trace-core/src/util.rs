use std::time::{SystemTime, UNIX_EPOCH};

pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Truncates to at most `max` characters without splitting a UTF-8 code point.
pub fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// Shortens to at most `max` characters, cutting at a word boundary and adding an ellipsis.
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut = truncate(s, max.saturating_sub(1));
    let cut = cut.rfind(' ').filter(|i| *i > max / 2).map(|i| &cut[..i]).unwrap_or(&cut);
    format!("{}…", cut.trim_end_matches([' ', ',', '.', ':', '(']))
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Epoch milliseconds as a UTC date, e.g. `2026-10-28`.
pub fn date(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(DAY_MS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// A moment given as epoch milliseconds or ISO 8601: `2026-10-28`,
/// `2026-10-28T14:05:00Z`, `2026-10-28 14:05:00+02:00` (seconds and the
/// fraction optional; no offset means UTC).
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms);
    }
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<i64>().ok());
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if s.get(4..5) != Some("-") || s.get(7..8) != Some("-") || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, mo as u32, d as u32) * DAY_MS;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(ms);
    }
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    let n = |r: std::ops::Range<usize>| rest.get(r).filter(|p| p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<i64>().ok());
    let (h, mi) = (n(0..2)?, n(3..5)?);
    if rest.get(2..3) != Some(":") || h > 23 || mi > 59 {
        return None;
    }
    let mut i = 5;
    let mut sec = 0;
    if rest.get(5..6) == Some(":") {
        sec = n(6..8).filter(|s| *s <= 60)?;
        i = 8;
        if rest.get(8..9) == Some(".") {
            i = 9 + rest[9..].bytes().take_while(u8::is_ascii_digit).count();
        }
    }
    ms += ((h * 60 + mi) * 60 + sec) * 1000;
    match &rest[i..] {
        "" | "Z" | "z" => Some(ms),
        off => {
            let sign = match off.get(0..1)? { "+" => 1, "-" => -1, _ => return None };
            let o = |r: std::ops::Range<usize>| off.get(r).filter(|p| p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<i64>().ok());
            let (oh, om) = (o(1..3)?, if off.len() == 6 && off.get(3..4) == Some(":") { o(4..6)? } else if off.len() == 5 { o(3..5)? } else { return None });
            Some(ms - sign * (oh * 60 + om) * 60_000)
        }
    }
}

pub fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_times() {
        assert_eq!(parse_time("2026-10-28"), Some(1_793_145_600_000));
        assert_eq!(date(1_793_145_600_000), "2026-10-28");
        assert_eq!(parse_time("2026-10-28T14:05:00Z"), Some(1_793_145_600_000 + (14 * 60 + 5) * 60_000));
        assert_eq!(parse_time("2026-10-28 16:05:00.250+02:00"), parse_time("2026-10-28T14:05:00Z"));
        assert_eq!(parse_time("2026-10-28T14:05"), parse_time("2026-10-28T14:05:00Z"));
        assert_eq!(parse_time("1793145600000"), Some(1_793_145_600_000));
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400_000), "2000-02-29");
        for bad in ["yesterday", "2026-13-01", "2026-10-28T25:00Z", "2026-10-28T14:05+2"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }
}
