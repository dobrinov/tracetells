//! Raw behaviour is only kept as long as analysis and evaluation need it.

use anyhow::Result;
use sqlx::SqlitePool;

use crate::util::{DAY_MS, now_ms};

/// Deletes events and sessions older than `days`. Ideas, KPIs and
/// evaluations are kept: they hold aggregates, not raw behaviour.
pub async fn purge(pool: &SqlitePool, project_id: i64, days: u32) -> Result<u64> {
    let cutoff = now_ms() - days as i64 * DAY_MS;
    let events = sqlx::query("DELETE FROM events WHERE project_id = ? AND ts < ?")
        .bind(project_id)
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected();
    sqlx::query("DELETE FROM sessions WHERE project_id = ? AND last_seen_at < ?")
        .bind(project_id)
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(events)
}
