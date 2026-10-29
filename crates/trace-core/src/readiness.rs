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
    /// What to do: "analyse" (worth it now), "wait" (some new behaviour, not
    /// enough yet) or "nothing_new".
    pub next: &'static str,
    /// Sessions that started since the last analysis (or ever, before the first).
    pub new_sessions: i64,
    /// Events recorded since then: what the next analysis will read that the last one didn't.
    pub new_events: i64,
    /// New sessions that make an analysis worth running now (fewer once a week has passed).
    pub needed: i64,
    /// When the last analysis ran: the behaviour it read ends there.
    pub last_analysed_at: Option<i64>,
    /// Who ran it, e.g. "Claude Code" (a coding agent over MCP) or "Trace".
    pub analysed_by: Option<String>,
    /// Whether anyone in the account has connected a coding agent, which is where analysis usually runs.
    pub agent_connected: bool,
    /// One sentence for people and agents.
    pub message: String,
}

/// Records that the website was just analysed, and by whom: an agent saving
/// its findings (even none), or Trace's own analysis.
pub async fn mark_analysed(pool: &Pool, site_id: i64, by: &str) -> Result<()> {
    sqlx::query("UPDATE projects SET analysed_at = $1, analysed_by = $2 WHERE id = $3").bind(now_ms()).bind(by).bind(site_id).execute(pool).await?;
    Ok(())
}

/// Who an MCP caller is, for "analysed by": the app its token is named after
/// ("Claude Code" from `... via token "Claude Code (signed in 2026-10-28)"`).
pub fn agent_name(actor: &str) -> String {
    actor
        .split_once("via token \"")
        .map(|(_, rest)| rest.split([' ', '"']).filter(|w| !w.is_empty()).take_while(|w| !w.starts_with('(')).collect::<Vec<_>>().join(" "))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "a coding agent".into())
}

pub async fn status(pool: &Pool, site: &Project) -> Result<AnalysisStatus> {
    let (last, by): (Option<i64>, Option<String>) =
        sqlx::query_as("SELECT analysed_at, analysed_by FROM projects WHERE id = $1").bind(site.id).fetch_one(pool).await?;
    let since = last.unwrap_or(0);
    let new_sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = $1 AND started_at > $2")
        .bind(site.id)
        .bind(since)
        .fetch_one(pool)
        .await?;
    let new_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE project_id = $1 AND ts > $2").bind(site.id).bind(since).fetch_one(pool).await?;
    let tokens: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_tokens WHERE account_id = $1 AND revoked_at IS NULL AND last_used_at IS NOT NULL")
        .bind(site.account_id)
        .fetch_one(pool)
        .await?;
    let mut s = decide(&site.name, new_sessions, last, now_ms());
    s.new_events = new_events;
    s.analysed_by = by;
    s.agent_connected = tokens > 0;
    Ok(s)
}

fn decide(name: &str, new_sessions: i64, last: Option<i64>, now: i64) -> AnalysisStatus {
    let a_week_on = last.is_some_and(|t| now - t >= 7 * DAY_MS);
    let needed = if a_week_on { SESSIONS_AFTER_A_WEEK } else { SESSIONS };
    let ready = new_sessions >= needed;
    let since = match last {
        Some(t) => {
            let days = (now - t) / DAY_MS;
            let age = match days {
                0 => "today".to_string(),
                1 => "yesterday".to_string(),
                d => format!("{d} days ago"),
            };
            format!("since the last analysis ({age})")
        }
        None => "so far".to_string(),
    };
    let message = if ready {
        format!(
            "{name} has {new_sessions} new sessions {since}: worth analysing. In Claude Code, run /trace:analyse (or /mcp__trace__analyse_website without the plugin)."
        )
    } else if new_sessions == 0 {
        format!("{name} has no new sessions {since}.")
    } else {
        format!("{name} has {new_sessions} new sessions {since}; analysis is worth running at {needed}.")
    };
    let next = if ready { "analyse" } else if new_sessions == 0 { "nothing_new" } else { "wait" };
    AnalysisStatus { ready, next, new_sessions, new_events: 0, needed, last_analysed_at: last, analysed_by: None, agent_connected: false, message }
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
        assert_eq!((decide("Shop", 40, Some(now - 2 * DAY_MS), now).next, decide("Shop", 40, Some(now - 2 * DAY_MS), now).needed), ("wait", 100));
        assert_eq!(decide("Shop", 40, Some(now - 8 * DAY_MS), now).needed, 30);
    }

    #[test]
    fn names_the_agent_from_its_token() {
        assert_eq!(agent_name(r#"ana@acme.example via token "Claude Code (signed in 2026-10-28)""#), "Claude Code");
        assert_eq!(agent_name(r#"ana@acme.example via token "Cursor""#), "Cursor");
        assert_eq!(agent_name("TRACE_ADMIN_TOKEN"), "a coding agent");
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
        mark_analysed(&pool, site.id, "Claude Code").await.unwrap();
        assert_eq!(status(&pool, &site).await.unwrap().new_sessions, 0, "analysed: counting starts again");
        add("after".into(), now_ms() + 1000).await;
        let s = status(&pool, &site).await.unwrap();
        assert_eq!((s.new_sessions, s.ready), (1, false));
        assert!(s.last_analysed_at.is_some());
        assert_eq!((s.analysed_by.as_deref(), s.next, s.agent_connected), (Some("Claude Code"), "wait", false));
        crate::db::drop_test_target(&target).await;
    }
}
