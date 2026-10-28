//! When a website has enough new behaviour to be worth analysing. Analysis
//! happens in the website owner's coding agent, so Trace says when to run it:
//! in the dashboard, over MCP and as a one-line message for agent hooks.

use anyhow::Result;
use serde::Serialize;

use crate::Pool;
use crate::model::Project;
use crate::util::{DAY_MS, now_ms};

/// New sessions that make an analysis worth running.
pub const SESSIONS: i64 = 100;
/// Fewer are enough once this long has passed, so small sites get looked at too.
pub const SESSIONS_AFTER_A_WEEK: i64 = 30;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnalysisStatus {
    pub ready: bool,
    /// Sessions that started since the last analysis (or ever, before the first).
    pub new_sessions: i64,
    pub last_analysed_at: Option<i64>,
    /// One sentence for people and agents.
    pub message: String,
}

/// Records that the website was just analysed: by an agent saving its
/// findings (even none), or by Trace's own analysis.
pub async fn mark_analysed(pool: &Pool, site_id: i64) -> Result<()> {
    sqlx::query("UPDATE projects SET analysed_at = $1 WHERE id = $2").bind(now_ms()).bind(site_id).execute(pool).await?;
    Ok(())
}

pub async fn status(pool: &Pool, site: &Project) -> Result<AnalysisStatus> {
    let last: Option<i64> = sqlx::query_scalar("SELECT analysed_at FROM projects WHERE id = $1").bind(site.id).fetch_one(pool).await?;
    let new_sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = $1 AND started_at > $2")
        .bind(site.id)
        .bind(last.unwrap_or(0))
        .fetch_one(pool)
        .await?;
    Ok(decide(&site.name, new_sessions, last, now_ms()))
}

fn decide(name: &str, new_sessions: i64, last: Option<i64>, now: i64) -> AnalysisStatus {
    let a_week_on = last.is_some_and(|t| now - t >= 7 * DAY_MS);
    let ready = new_sessions >= SESSIONS || (a_week_on && new_sessions >= SESSIONS_AFTER_A_WEEK);
    let since = if last.is_some() { "since the last analysis" } else { "so far" };
    let message = if ready {
        format!(
            "{name} has {new_sessions} new sessions {since}: worth analysing. In Claude Code, run /trace:analyse (or /mcp__trace__analyse_website without the plugin)."
        )
    } else if new_sessions == 0 {
        format!("{name} has no new sessions {since}.")
    } else {
        let needed = if a_week_on { SESSIONS_AFTER_A_WEEK } else { SESSIONS };
        format!("{name} has {new_sessions} new sessions {since}; analysis is worth running at {needed}.")
    };
    AnalysisStatus { ready, new_sessions, last_analysed_at: last, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_at_a_hundred_or_thirty_after_a_week() {
        let now = 100 * DAY_MS;
        assert!(!decide("Shop", 99, None, now).ready);
        assert!(decide("Shop", 100, None, now).ready);
        assert!(!decide("Shop", 40, Some(now - 2 * DAY_MS), now).ready, "too soon for the small threshold");
        assert!(decide("Shop", 40, Some(now - 8 * DAY_MS), now).ready);
        let s = decide("Shop", 140, Some(now - DAY_MS), now);
        assert!(s.message.contains("140 new sessions since the last analysis") && s.message.contains("/trace:analyse"));
        assert_eq!(decide("Shop", 0, None, now).message, "Shop has no new sessions so far.");
    }

    #[tokio::test]
    async fn counts_sessions_since_the_last_analysis() {
        let target = crate::db::test_target("ready").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let add = |id: String, at: i64| {
            let pool = pool.clone();
            async move {
                sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at) VALUES ($1, $2, $3, $3)")
                    .bind(id)
                    .bind(site.id)
                    .bind(at)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        };
        for i in 0..3 {
            add(format!("before-{i}"), now_ms() - 1000).await;
        }
        assert_eq!(status(&pool, &site).await.unwrap().new_sessions, 3);
        mark_analysed(&pool, site.id).await.unwrap();
        assert_eq!(status(&pool, &site).await.unwrap().new_sessions, 0, "analysed: counting starts again");
        add("after".into(), now_ms() + 1000).await;
        let s = status(&pool, &site).await.unwrap();
        assert_eq!((s.new_sessions, s.ready), (1, false));
        assert!(s.last_analysed_at.is_some());
        crate::db::drop_test_target(&target).await;
    }
}
