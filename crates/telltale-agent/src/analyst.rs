use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
use telltale_core::model::{Idea, Kpi, NewIdea};
use telltale_core::util::now_ms;
use telltale_core::{digest, ideas, kpi};

use crate::claude::{self, ClaudeConfig};
use crate::heuristics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Claude when an API key is configured, heuristics otherwise.
    Auto,
    Claude,
    Heuristics,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub engine: Engine,
    pub days: u32,
    pub max_ideas: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { engine: Engine::Auto, days: 14, max_ideas: 5 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub run_id: i64,
    pub engine: String,
    pub model: Option<String>,
    pub created: Vec<i64>,
    /// Ideas the analyst proposed that already existed.
    pub duplicates: usize,
    /// Set when Claude failed and the run fell back to heuristics.
    pub warning: Option<String>,
}

const SYSTEM_PROMPT: &str = "\
You are Telltale's product analyst. You get an aggregated digest of how real users behaved in a web product: \
pages with views and exit rates, navigation transitions, rage clicks, dead clicks, JavaScript errors, form starts and submits, \
custom events and the team's KPIs with current and previous values. Your job is to propose specific product changes that \
will move those KPIs.

How to work:
- Ground every idea in numbers from the digest and cite them in `evidence`. Don't invent behaviour you can't see.
- Put the KPIs first. Prefer ideas that plausibly move a KPI, especially one that is off target or getting worse. \
Set `kpi_id` to the KPI the idea is meant to move, or null if none fits.
- Be concrete. Name the page, the element (use the selector and its text), what to change and why it should help. \
A developer or coding agent will implement `proposal` without seeing the digest.
- `acceptance_criteria` are checks a developer can verify in the product, not KPI targets.
- `pages` lists the routes involved, exactly as they appear in the digest.
- Don't repeat an existing idea, even with different wording. Treat dismissed ideas and their reasons as feedback on what \
this team doesn't want, and avoid proposing similar ones.
- Fewer, stronger ideas beat many weak ones. If the data doesn't support any idea, return an empty list.";

#[derive(Deserialize)]
struct AnalystOutput {
    ideas: Vec<AnalystIdea>,
}

#[derive(Deserialize)]
struct AnalystIdea {
    title: String,
    problem: String,
    evidence: Vec<String>,
    proposal: String,
    acceptance_criteria: Vec<String>,
    pages: Vec<String>,
    kpi_id: Option<i64>,
    expected_impact: String,
    confidence: String,
}

fn output_schema() -> serde_json::Value {
    let strings = json!({"type": "array", "items": {"type": "string"}});
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ideas"],
        "properties": {
            "ideas": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "problem", "evidence", "proposal", "acceptance_criteria", "pages",
                                 "kpi_id", "expected_impact", "confidence"],
                    "properties": {
                        "title": {"type": "string", "description": "Imperative, under 80 characters"},
                        "problem": {"type": "string", "description": "What users struggle with, in 1-3 sentences"},
                        "evidence": strings.clone(),
                        "proposal": {"type": "string"},
                        "acceptance_criteria": strings.clone(),
                        "pages": strings,
                        "kpi_id": {"anyOf": [{"type": "integer"}, {"type": "null"}]},
                        "expected_impact": {"type": "string", "description": "Which metric should move and roughly how much"},
                        "confidence": {"type": "string", "enum": ["low", "medium", "high"]}
                    }
                }
            }
        }
    })
}

fn user_message(digest: &digest::Digest, existing: &[Idea], max_ideas: usize) -> Result<String> {
    let existing: Vec<_> = existing
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "title": i.title,
                "status": i.status,
                "dismiss_reason": i.dismiss_reason,
                "evaluation": i.evaluation.as_ref().map(|e| &e.summary),
            })
        })
        .collect();
    Ok(format!(
        "<digest>\n{}\n</digest>\n\n<existing_ideas>\n{}\n</existing_ideas>\n\n\
         Propose up to {max_ideas} new ideas. Rates in the digest are fractions between 0 and 1. \
         Times are epoch milliseconds.",
        serde_json::to_string_pretty(digest)?,
        serde_json::to_string_pretty(&existing)?,
    ))
}

async fn ask_claude(
    cfg: &ClaudeConfig,
    digest: &digest::Digest,
    existing: &[Idea],
    kpis: &[Kpi],
    max_ideas: usize,
) -> Result<(Vec<NewIdea>, String)> {
    let out: claude::Structured<AnalystOutput> =
        claude::structured(cfg, SYSTEM_PROMPT, &user_message(digest, existing, max_ideas)?, output_schema()).await?;
    let ideas = out
        .value
        .ideas
        .into_iter()
        .take(max_ideas)
        .map(|i| NewIdea {
            title: i.title,
            problem: i.problem,
            evidence: i.evidence,
            proposal: i.proposal,
            acceptance_criteria: i.acceptance_criteria,
            pages: i.pages,
            kpi_id: i.kpi_id.filter(|id| kpis.iter().any(|k| k.id == *id)),
            expected_impact: i.expected_impact,
            confidence: i.confidence,
            source: "claude".into(),
            fingerprint: None,
        })
        .collect();
    Ok((ideas, out.model))
}

/// Runs one analysis pass and stores the new ideas.
pub async fn analyze(
    pool: &SqlitePool,
    project_id: i64,
    claude_cfg: Option<&ClaudeConfig>,
    opts: &Options,
) -> Result<RunReport> {
    let engine_cfg = match (opts.engine, claude_cfg) {
        (Engine::Heuristics, _) | (Engine::Auto, None) => None,
        (_, Some(cfg)) => Some(cfg),
        (Engine::Claude, None) => anyhow::bail!("the claude engine needs ANTHROPIC_API_KEY to be set"),
    };
    let started_at = now_ms();
    let run_id: i64 = sqlx::query_scalar(
        "INSERT INTO analysis_runs (project_id, engine, model, started_at) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(if engine_cfg.is_some() { "claude" } else { "heuristics" })
    .bind(engine_cfg.map(|c| c.model.as_str()))
    .bind(started_at)
    .fetch_one(pool)
    .await?;

    let result = run(pool, project_id, engine_cfg, opts).await;
    let (engine, model, proposed, warning, error) = match result {
        Ok((engine, model, proposed, warning)) => (engine, model, proposed, warning, None),
        Err(e) => (String::new(), None, Vec::new(), None, Some(format!("{e:#}"))),
    };

    let mut created = Vec::new();
    let mut duplicates = 0;
    for idea in &proposed {
        match ideas::insert(pool, project_id, idea).await? {
            Some(id) => created.push(id),
            None => duplicates += 1,
        }
    }
    sqlx::query("UPDATE analysis_runs SET finished_at = ?, ideas_created = ?, error = ?, engine = COALESCE(NULLIF(?, ''), engine), model = ? WHERE id = ?")
        .bind(now_ms())
        .bind(created.len() as i64)
        .bind(error.as_deref().or(warning.as_deref()))
        .bind(&engine)
        .bind(model.as_deref())
        .bind(run_id)
        .execute(pool)
        .await?;
    if let Some(e) = error {
        anyhow::bail!(e);
    }
    Ok(RunReport { run_id, engine, model, created, duplicates, warning })
}

async fn run(
    pool: &SqlitePool,
    project_id: i64,
    claude_cfg: Option<&ClaudeConfig>,
    opts: &Options,
) -> Result<(String, Option<String>, Vec<NewIdea>, Option<String>)> {
    let digest = digest::build(pool, project_id, opts.days).await?;
    let kpis = kpi::list(pool, project_id).await?;
    if digest.totals.sessions == 0 {
        return Ok(("none".into(), None, Vec::new(), Some("No sessions in the window yet. Install the collector first.".into())));
    }
    let heuristic = || {
        let mut found = heuristics::find(&digest, &kpis);
        found.truncate(opts.max_ideas);
        found
    };
    let Some(cfg) = claude_cfg else {
        return Ok(("heuristics".into(), None, heuristic(), None));
    };
    let existing = ideas::list(pool, project_id, None).await?;
    match ask_claude(cfg, &digest, &existing, &kpis, opts.max_ideas).await {
        Ok((ideas, model)) => Ok(("claude".into(), Some(model), ideas, None)),
        Err(e) if opts.engine == Engine::Auto => {
            tracing::warn!(error = %format!("{e:#}"), "Claude analysis failed; using heuristics");
            Ok(("heuristics".into(), None, heuristic(), Some(format!("Claude failed, used heuristics instead: {e:#}"))))
        }
        Err(e) => Err(e),
    }
}

