//! Raw behaviour is only kept as long as analysis, evaluation and each
//! account's plan allow.

use anyhow::Result;
use crate::Pool;

use crate::util::{DAY_MS, now_ms};

/// Deletes raw events and sessions older than each account's retention: the
/// shorter of its plan's limit and the server-wide `server_days` (0 = no
/// server-wide limit). Insights, KPIs and evaluations are aggregates and stay.
/// Returns the number of events deleted.
pub async fn purge(pool: &Pool, server_days: u32) -> Result<u64> {
    let mut deleted = 0;
    for account in crate::db::accounts(pool).await? {
        let plan_days = crate::plans::for_account(pool, account.id).await?.effective.retention_days;
        let server = (server_days > 0).then_some(server_days as i64);
        let days = match (plan_days, server) {
            (Some(p), Some(s)) => p.min(s),
            (Some(d), None) | (None, Some(d)) => d,
            (None, None) => continue,
        };
        let cutoff = now_ms() - days * DAY_MS;
        for site in crate::db::sites(pool, account.id).await? {
            deleted += sqlx::query("DELETE FROM events WHERE project_id = $1 AND ts < $2")
                .bind(site.id)
                .bind(cutoff)
                .execute(pool)
                .await?
                .rows_affected();
            sqlx::query("DELETE FROM sessions WHERE project_id = $1 AND last_seen_at < $2").bind(site.id).bind(cutoff).execute(pool).await?;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plan_retention_and_server_cap() {
        let path = crate::db::test_target("retention").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, free) = crate::db::create_account_with_site(&pool, "Free", Some("free")).await.unwrap(); // 30 days
        let (_, open) = crate::db::create_account_with_site(&pool, "Open", None).await.unwrap(); // unlimited
        for site in [free.id, open.id] {
            for days in [1, 31, 120] {
                sqlx::query("INSERT INTO events (project_id, session_id, ts, type) VALUES ($1, 's', $2, 'pageview')")
                    .bind(site)
                    .bind(now_ms() - days * DAY_MS)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        let count = |site: i64| {
            let pool = pool.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events WHERE project_id = $1").bind(site).fetch_one(&pool).await.unwrap() }
        };
        purge(&pool, 90).await.unwrap();
        assert_eq!(count(free.id).await, 1, "a 30-day account loses 31-day-old events");
        assert_eq!(count(open.id).await, 2, "an unlimited account still follows the server's 90 days");
        purge(&pool, 0).await.unwrap();
        assert_eq!(count(open.id).await, 2, "no limits at all when the server sets none");
        crate::db::drop_test_target(&path).await;
    }
}
