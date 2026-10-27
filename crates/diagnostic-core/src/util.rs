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

pub fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}
