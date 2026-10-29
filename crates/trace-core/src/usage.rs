//! Monthly usage per account, counted against plan limits.

use anyhow::Result;
use serde::Serialize;
use crate::Pool;

use crate::util::now_ms;

/// `YYYY-MM` (UTC) for a timestamp in epoch milliseconds.
pub fn month_of(ms: i64) -> String {
    // Howard Hinnant's days-to-civil algorithm.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

pub fn this_month() -> String {
    month_of(now_ms())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Usage {
    pub month: String,
    pub events: i64,
    pub events_dropped: i64,
}

pub async fn get(pool: &Pool, account_id: i64, month: &str) -> Result<Usage> {
    let row: Option<(i64, i64)> =
        sqlx::query_as("SELECT events, events_dropped FROM usage WHERE account_id = $1 AND month = $2")
            .bind(account_id)
            .bind(month)
            .fetch_optional(pool)
            .await?;
    let (events, events_dropped) = row.unwrap_or_default();
    Ok(Usage { month: month.to_string(), events, events_dropped })
}

pub async fn add_events(pool: &Pool, account_id: i64, kept: i64, dropped: i64) -> Result<()> {
    if kept == 0 && dropped == 0 {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO usage (account_id, month, events, events_dropped) VALUES ($1, $2, $3, $4)
         ON CONFLICT(account_id, month) DO UPDATE SET events = usage.events + excluded.events, events_dropped = usage.events_dropped + excluded.events_dropped",
    )
    .bind(account_id)
    .bind(this_month())
    .bind(kept)
    .bind(dropped)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(1_798_675_200_000), "2026-12"); // 2026-12-31T00:00:00Z
        assert_eq!(month_of(1_798_761_599_999), "2026-12"); // last millisecond of 2026
        assert_eq!(month_of(1_798_761_600_000), "2027-01");
        assert_eq!(month_of(951_782_400_000), "2000-02"); // 2000-02-29
    }
}
