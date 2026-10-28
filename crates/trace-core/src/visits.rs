//! Cookieless visits: recognising a visit without storing anything on the
//! visitor's device.
//!
//! By default the collector keeps no id in the browser, so there is nothing to
//! ask consent for. The server tells visits apart instead: a visitor is a hash
//! of the website, the visitor's address and their browser, salted with a
//! random value that changes every day (UTC). Salts older than yesterday are
//! deleted, so once a day is over its hashes can't be linked to an address or
//! to the next day's visits. The address itself is never stored.
//!
//! A session is the same visitor sending within [`SESSION_IDLE_MS`] of their
//! last event, like the collector's own rule. Yesterday's salt is kept so a
//! session that runs past midnight carries on rather than splitting in two.
//!
//! What this costs: visitors can't be followed from one day to the next, so a
//! returning visitor counts again each day, and two people on one network
//! with the same browser version look like one visitor.

use anyhow::Result;
use crate::Pool;
use crate::auth::{random_token, sha256_hex};
use crate::util::DAY_MS;

/// No activity for this long ends a session; matches the collector.
pub const SESSION_IDLE_MS: i64 = 30 * 60 * 1000;
/// Visitor hashes start with this, so they can't be mistaken for a collector's id.
const PREFIX: &str = "v_";

/// The day (UTC) a moment falls on, e.g. "2026-10-28".
fn day(at_ms: i64) -> String {
    let days = at_ms.div_euclid(DAY_MS);
    let date = civil_date(days);
    format!("{:04}-{:02}-{:02}", date.0, date.1, date.2)
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Today's salt, created on first use, and yesterday's if it still exists.
/// Older salts are deleted, which is what makes old hashes unlinkable.
async fn salts(pool: &Pool, now: i64) -> Result<(String, Option<String>)> {
    let today = day(now);
    let yesterday = day(now - DAY_MS);
    sqlx::query("INSERT INTO daily_salts (day, salt) VALUES ($1, $2) ON CONFLICT (day) DO NOTHING")
        .bind(&today)
        .bind(random_token(32))
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM daily_salts WHERE day < $1").bind(&yesterday).execute(pool).await?;
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT day, salt FROM daily_salts WHERE day IN ($1, $2)").bind(&today).bind(&yesterday).fetch_all(pool).await?;
    let pick = |d: &str| rows.iter().find(|(day, _)| day == d).map(|(_, s)| s.clone());
    Ok((pick(&today).unwrap_or_default(), pick(&yesterday)))
}

fn visitor_hash(salt: &str, project_id: i64, address: &str, user_agent: &str) -> String {
    // Separators that can't occur in the parts, so ("a", "bc") and ("ab", "c") differ.
    let digest = sha256_hex(&format!("{salt}\u{1f}{project_id}\u{1f}{address}\u{1f}{user_agent}"));
    format!("{PREFIX}{}", &digest[..32])
}

/// Where a visitor falls between 0 and 1, for sampling: stable for the day.
pub fn sample_position(visitor: &str) -> f64 {
    let hex = visitor.strip_prefix(PREFIX).unwrap_or(visitor);
    let n = u64::from_str_radix(hex.get(..13).unwrap_or("0"), 16).unwrap_or(0);
    n as f64 / (1u64 << 52) as f64
}

/// A cookieless visit: the session to record events under, and the visitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub session_id: String,
    pub visitor_id: String,
}

/// The visit a batch without a session id belongs to: the visitor's active
/// session if they sent anything in the last 30 minutes (today or, across
/// midnight, yesterday), otherwise a new one.
pub async fn visit(pool: &Pool, project_id: i64, address: &str, user_agent: &str, now: i64) -> Result<Visit> {
    let (today, yesterday) = salts(pool, now).await?;
    let visitor = visitor_hash(&today, project_id, address, user_agent);
    let earlier = yesterday.map(|s| visitor_hash(&s, project_id, address, user_agent)).unwrap_or_else(|| visitor.clone());
    let active: Option<(String, String)> = sqlx::query_as(
        "SELECT id, visitor_id FROM sessions
         WHERE project_id = $1 AND visitor_id IN ($2, $3) AND last_seen_at > $4
         ORDER BY last_seen_at DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(&visitor)
    .bind(&earlier)
    .bind(now - SESSION_IDLE_MS)
    .fetch_optional(pool)
    .await?;
    Ok(match active {
        Some((session_id, visitor_id)) => Visit { session_id, visitor_id },
        None => Visit { session_id: format!("c_{}", random_token(16)), visitor_id: visitor },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    #[test]
    fn days_are_utc_dates() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_793_232_000_000), "2026-10-29");
        assert_eq!(day(1_793_232_000_000 - 1), "2026-10-28");
        assert_eq!(day(951_782_400_000), "2000-02-29", "leap day");
    }

    #[test]
    fn sampling_positions_spread_and_stay_put() {
        let positions: Vec<f64> = (0..400).map(|i| sample_position(&visitor_hash("salt", 1, &format!("10.0.0.{i}"), "UA"))).collect();
        assert!(positions.iter().all(|p| (0.0..1.0).contains(p)));
        let below = positions.iter().filter(|p| **p < 0.25).count();
        assert!((60..140).contains(&below), "about a quarter: {below}");
        assert_eq!(sample_position(&visitor_hash("salt", 1, "a", "b")), sample_position(&visitor_hash("salt", 1, "a", "b")));
    }

    async fn record(pool: &Pool, project: i64, v: &Visit, at: i64) {
        sqlx::query(
            "INSERT INTO sessions (id, project_id, visitor_id, started_at, last_seen_at) VALUES ($1, $2, $3, $4, $4)
             ON CONFLICT(id) DO UPDATE SET last_seen_at = excluded.last_seen_at",
        )
        .bind(&v.session_id)
        .bind(project)
        .bind(&v.visitor_id)
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn visits_without_anything_on_the_device() {
        let path = crate::db::test_target("visits").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let account = crate::db::create_account(&pool, "Acme", None).await.unwrap().id;
        let shop = crate::db::create_site(&pool, account, "Shop").await.unwrap().id;
        let docs = crate::db::create_site(&pool, account, "Docs").await.unwrap().id;
        // Noon UTC, so the midnight case below is deliberate.
        let noon = now_ms().div_euclid(DAY_MS) * DAY_MS + DAY_MS / 2;

        let first = visit(&pool, shop, "203.0.113.7", "Firefox 140", noon).await.unwrap();
        assert!(first.visitor_id.starts_with("v_") && !first.visitor_id.contains("203.0.113.7"));
        record(&pool, shop, &first, noon).await;

        // Next page, ten minutes later: the same session.
        let again = visit(&pool, shop, "203.0.113.7", "Firefox 140", noon + 10 * 60_000).await.unwrap();
        assert_eq!(again, first);
        record(&pool, shop, &again, noon + 10 * 60_000).await;
        // Another browser, another address or another website: someone else.
        assert_ne!(visit(&pool, shop, "203.0.113.7", "Safari 19", noon).await.unwrap().visitor_id, first.visitor_id);
        assert_ne!(visit(&pool, shop, "198.51.100.2", "Firefox 140", noon).await.unwrap().visitor_id, first.visitor_id);
        assert_ne!(visit(&pool, docs, "203.0.113.7", "Firefox 140", noon).await.unwrap().visitor_id, first.visitor_id);

        // After 30 quiet minutes, a new session for the same visitor.
        let later = visit(&pool, shop, "203.0.113.7", "Firefox 140", noon + 41 * 60_000).await.unwrap();
        assert_ne!(later.session_id, first.session_id);
        assert_eq!(later.visitor_id, first.visitor_id);

        // A session running past midnight carries on; the next day's visitor is a new hash.
        let late = noon + DAY_MS / 2 - 5 * 60_000;
        let evening = visit(&pool, shop, "198.51.100.9", "Chrome 150", late).await.unwrap();
        record(&pool, shop, &evening, late).await;
        let after_midnight = visit(&pool, shop, "198.51.100.9", "Chrome 150", late + 10 * 60_000).await.unwrap();
        assert_eq!(after_midnight.session_id, evening.session_id);
        let next_day = visit(&pool, shop, "198.51.100.9", "Chrome 150", late + 3 * 60 * 60_000).await.unwrap();
        assert_ne!(next_day.visitor_id, evening.visitor_id, "unlinkable from one day to the next");

        // Two days on, the old salts are gone.
        visit(&pool, shop, "a", "b", noon + 2 * DAY_MS).await.unwrap();
        let days: Vec<String> = sqlx::query_scalar("SELECT day FROM daily_salts ORDER BY day").fetch_all(&pool).await.unwrap();
        assert_eq!(days, [day(noon + DAY_MS), day(noon + 2 * DAY_MS)]);
        crate::db::drop_test_target(&path).await;
    }
}
