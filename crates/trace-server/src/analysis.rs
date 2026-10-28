//! Running analysis within the account's plan: Claude-powered runs count
//! against the monthly limit, heuristic runs never do.

use anyhow::{Result, bail};
use trace_agent::{ClaudeConfig, Engine, Options, RunReport};
use trace_core::model::Project;
use trace_core::{Pool, plans, usage};

pub async fn run(pool: &Pool, site: &Project, claude: Option<&ClaudeConfig>, opts: &Options) -> Result<RunReport> {
    let wants_claude = claude.is_some() && opts.engine != Engine::Heuristics;
    if !wants_claude {
        return trace_agent::analyze(pool, site.id, claude, opts).await;
    }
    let limit = plans::for_account(pool, site.account_id).await?.effective.monthly_ai_runs;
    if !usage::take_ai_run(pool, site.account_id, limit).await? {
        let message = format!(
            "this account has used its {} Claude-powered analyses for this month",
            limit.unwrap_or_default()
        );
        if opts.engine == Engine::Claude {
            bail!("{message}; run again with the heuristics engine, or ask for more");
        }
        let mut report = trace_agent::analyze(pool, site.id, None, &Options { engine: Engine::Heuristics, ..opts.clone() }).await?;
        report.warning = Some(format!("{message}, so the built-in heuristics were used."));
        return Ok(report);
    }
    let report = trace_agent::analyze(pool, site.id, claude, opts).await;
    // Only runs that actually used Claude count.
    if !matches!(&report, Ok(r) if r.engine == "claude") {
        usage::refund_ai_run(pool, site.account_id).await?;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use trace_core::db;

    fn fake_claude() -> ClaudeConfig {
        // Points at a closed port: a Claude attempt fails fast, and auto falls back.
        ClaudeConfig { api_key: "x".into(), model: "claude-opus-5-5".into(), base_url: "http://127.0.0.1:9".into(), effort: "low".into() }
    }

    async fn site_with_sessions() -> (Pool, Project, String) {
        let path = trace_core::db::test_target("analysis").await;
        let pool = db::connect(&path).await.unwrap();
        let (_, site) = db::create_account_with_site(&pool, "Free", Some("free")).await.unwrap();
        sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at) VALUES ('s', $1, $2, $3)")
            .bind(site.id)
            .bind(trace_core::util::now_ms())
            .bind(trace_core::util::now_ms())
            .execute(&pool)
            .await
            .unwrap();
        (pool, site, path)
    }

    #[tokio::test]
    async fn claude_limit_falls_back_refuses_and_never_limits_heuristics() {
        let (pool, site, path) = site_with_sessions().await;
        let claude = fake_claude();
        // Use up the free plan's 5 runs.
        sqlx::query("INSERT INTO usage (account_id, month, ai_runs) VALUES ($1, $2, 5)")
            .bind(site.account_id)
            .bind(usage::this_month())
            .execute(&pool)
            .await
            .unwrap();

        let auto = run(&pool, &site, Some(&claude), &Options::default()).await.unwrap();
        assert_eq!(auto.engine, "heuristics");
        assert!(auto.warning.unwrap().contains("Claude-powered analyses"));

        let explicit = run(&pool, &site, Some(&claude), &Options { engine: Engine::Claude, ..Options::default() }).await;
        assert!(explicit.unwrap_err().to_string().contains("used its 5"));

        for _ in 0..10 {
            let h = run(&pool, &site, Some(&claude), &Options { engine: Engine::Heuristics, ..Options::default() }).await.unwrap();
            assert_eq!(h.engine, "heuristics");
        }
        assert_eq!(usage::get(&pool, site.account_id, &usage::this_month()).await.unwrap().ai_runs, 5, "heuristics never count");
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn failed_claude_runs_are_not_counted() {
        let (pool, site, path) = site_with_sessions().await;
        let report = run(&pool, &site, Some(&fake_claude()), &Options::default()).await.unwrap();
        assert_eq!(report.engine, "heuristics", "the unreachable API falls back");
        assert_eq!(usage::get(&pool, site.account_id, &usage::this_month()).await.unwrap().ai_runs, 0);
        trace_core::db::drop_test_target(&path).await;
    }
}
