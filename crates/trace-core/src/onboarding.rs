//! The setup checklist for a website. Every step but the privacy review is
//! derived from data Trace already has, so nothing can drift out of sync.

use anyhow::Result;
use serde::Serialize;
use crate::Pool;

use crate::model::Project;
use crate::util::now_ms;

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub key: &'static str,
    pub done: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checklist {
    pub steps: Vec<Step>,
    /// Required steps done, and how many there are.
    pub done: usize,
    pub total: usize,
    pub complete: bool,
}

pub async fn checklist(pool: &Pool, site: &Project) -> Result<Checklist> {
    let (last_event_at, reviewed_at, analysed_at): (Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT last_event_at, privacy_reviewed_at, analysed_at FROM projects WHERE id = $1")
            .bind(site.id)
            .fetch_one(pool)
            .await?;
    let rules: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM privacy_rules WHERE project_id = $1").bind(site.id).fetch_one(pool).await?;
    let kpis: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM kpis WHERE project_id = $1 AND archived_at IS NULL").bind(site.id).fetch_one(pool).await?;
    let used_tokens: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_tokens WHERE account_id = $1 AND revoked_at IS NULL AND last_used_at IS NOT NULL",
    )
    .bind(site.account_id)
    .fetch_one(pool)
    .await?;
    let people: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memberships WHERE account_id = $1").bind(site.account_id).fetch_one(pool).await?;
    let invites: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM invites WHERE account_id = $1 AND accepted_at IS NULL AND expires_at > $2")
            .bind(site.account_id)
            .bind(now_ms())
            .fetch_one(pool)
            .await?;

    let insights: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM insights WHERE project_id = $1").bind(site.id).fetch_one(pool).await?;

    // Three outcomes, each checked against what Trace sees: data arrives, goals
    // are defined, a first analysis ran. The rest helps but never blocks.
    let step = |key, done, optional| Step { key, done, optional };
    let steps = vec![
        step("data", last_event_at.is_some(), false),
        step("goals", kpis > 0, false),
        step("insight", analysed_at.is_some() || insights > 0, false),
        step("agent", used_tokens > 0, true),
        step("privacy", reviewed_at.is_some() || rules > 0, true),
        step("team", people > 1 || invites > 0, true),
    ];
    let required: Vec<&Step> = steps.iter().filter(|s| !s.optional).collect();
    let done = required.iter().filter(|s| s.done).count();
    let total = required.len();
    Ok(Checklist { steps, done, total, complete: done == total })
}

/// Records that someone looked at what this website protects.
pub async fn confirm_privacy_review(pool: &Pool, site_id: i64, by: &str) -> Result<()> {
    sqlx::query("UPDATE projects SET privacy_reviewed_at = $1, privacy_reviewed_by = $2 WHERE id = $3")
        .bind(now_ms())
        .bind(by)
        .bind(site_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn done(c: &Checklist, key: &str) -> bool {
        c.steps.iter().any(|s| s.key == key && s.done)
    }

    fn keys_done(c: &Checklist) -> Vec<&'static str> {
        c.steps.iter().filter(|s| s.done).map(|s| s.key).collect()
    }

    #[tokio::test]
    async fn steps_follow_what_trace_observes() {
        let path = crate::db::test_target("onboarding").await;
        let pool = db::connect(&path).await.unwrap();
        let (account, site) = db::create_account_with_site(&pool, "Acme", None).await.unwrap();
        let docs = db::create_site(&pool, account.id, "Docs").await.unwrap();

        let fresh = checklist(&pool, &site).await.unwrap();
        assert!(keys_done(&fresh).is_empty());
        assert_eq!((fresh.done, fresh.total, fresh.complete), (0, 3, false));

        sqlx::query("UPDATE projects SET last_event_at = 1 WHERE id = $1").bind(site.id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO kpis (project_id, key, name, definition, direction, created_at) VALUES ($1, 'k', 'K', '{}', 'increase', 1)")
            .bind(site.id)
            .execute(&pool)
            .await
            .unwrap();
        crate::auth::create_user(&pool, account.id, "a@acme.example", "", "a long enough password", crate::auth::Role::Owner).await.unwrap();
        sqlx::query("INSERT INTO api_tokens (account_id, name, token_hash, prefix, created_at) VALUES ($1, 't', 'h', 'p', 1)")
            .bind(account.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(!done(&checklist(&pool, &site).await.unwrap(), "agent"), "an unused token doesn't count");
        sqlx::query("UPDATE api_tokens SET last_used_at = 2").execute(&pool).await.unwrap();
        assert_eq!(keys_done(&checklist(&pool, &site).await.unwrap()), ["data", "goals", "agent"]);

        crate::readiness::mark_analysed(&pool, site.id, "Claude Code").await.unwrap();
        let solo = checklist(&pool, &site).await.unwrap();
        assert!(solo.complete, "the agent, privacy and team steps are optional");
        assert!(!done(&solo, "team") && !done(&solo, "privacy"));

        confirm_privacy_review(&pool, site.id, "a@acme.example").await.unwrap();
        assert!(done(&checklist(&pool, &site).await.unwrap(), "privacy"));
        assert!(!done(&checklist(&pool, &docs).await.unwrap(), "privacy"), "the review belongs to one website");
        crate::privacy::add_rule(&pool, docs.id, crate::privacy::RuleKind::DropProp, "customer_ref", None, "", "test").await.unwrap();
        assert!(done(&checklist(&pool, &docs).await.unwrap(), "privacy"), "a custom rule counts as reviewed");
        crate::db::drop_test_target(&path).await;
    }
}
