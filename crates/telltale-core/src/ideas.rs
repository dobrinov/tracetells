//! Idea storage and lifecycle: new → selected → in progress → released, or dismissed.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::model::{Evaluation, Idea, IdeaStatus, NewIdea};
use crate::util::now_ms;

const COLS: &str = "id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id, expected_impact,
  confidence, status, dismiss_reason, source, fingerprint, created_at, updated_at, released_at";

fn json_list(row: &SqliteRow, col: &str) -> Result<Vec<String>> {
    let raw: String = row.try_get(col)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn from_row(row: &SqliteRow) -> Result<Idea> {
    Ok(Idea {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        problem: row.try_get("problem")?,
        evidence: json_list(row, "evidence")?,
        proposal: row.try_get("proposal")?,
        acceptance_criteria: json_list(row, "acceptance_criteria")?,
        pages: json_list(row, "pages")?,
        kpi_id: row.try_get("kpi_id")?,
        expected_impact: row.try_get("expected_impact")?,
        confidence: row.try_get("confidence")?,
        status: row.try_get::<String, _>("status")?.parse()?,
        dismiss_reason: row.try_get("dismiss_reason")?,
        source: row.try_get("source")?,
        fingerprint: row.try_get("fingerprint")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        released_at: row.try_get("released_at")?,
        evaluation: None,
    })
}

type EvalRow = (i64, i64, Option<f64>, Option<f64>, i64, i64, Option<f64>, String, String, f64, i64);

async fn evaluations(pool: &SqlitePool, project_id: i64) -> Result<HashMap<i64, Evaluation>> {
    let rows: Vec<EvalRow> = sqlx::query_as(
        "SELECT e.idea_id, e.kpi_id, e.baseline, e.current, e.baseline_n, e.current_n, e.p_value,
                e.verdict, e.summary, e.window_days, e.created_at
         FROM evaluations e JOIN ideas i ON i.id = e.idea_id WHERE i.project_id = ?",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for (idea_id, kpi_id, baseline, current, baseline_n, current_n, p_value, verdict, summary, window_days, created_at) in rows {
        out.insert(
            idea_id,
            Evaluation {
                idea_id,
                kpi_id,
                baseline,
                current,
                baseline_n,
                current_n,
                p_value,
                verdict: verdict.parse()?,
                summary,
                window_days,
                created_at,
            },
        );
    }
    Ok(out)
}

pub async fn list(pool: &SqlitePool, project_id: i64, status: Option<IdeaStatus>) -> Result<Vec<Idea>> {
    let sql = match status {
        Some(_) => format!("SELECT {COLS} FROM ideas WHERE project_id = ? AND status = ? ORDER BY updated_at DESC, id DESC"),
        None => format!("SELECT {COLS} FROM ideas WHERE project_id = ? ORDER BY updated_at DESC, id DESC"),
    };
    let mut q = sqlx::query(&sql).bind(project_id);
    if let Some(s) = status {
        q = q.bind(s.as_str());
    }
    let rows = q.fetch_all(pool).await?;
    let mut evals = evaluations(pool, project_id).await?;
    rows.iter()
        .map(|r| {
            let mut idea = from_row(r)?;
            idea.evaluation = evals.remove(&idea.id);
            Ok(idea)
        })
        .collect()
}

pub async fn get(pool: &SqlitePool, project_id: i64, id: i64) -> Result<Option<Idea>> {
    let row = sqlx::query(&format!("SELECT {COLS} FROM ideas WHERE project_id = ? AND id = ?"))
        .bind(project_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut idea = from_row(&row)?;
    idea.evaluation = evaluations(pool, project_id).await?.remove(&id);
    Ok(Some(idea))
}

/// Inserts an idea unless one with the same fingerprint or title already exists
/// (in any status, so dismissed ideas stay dismissed). Returns the new id.
pub async fn insert(pool: &SqlitePool, project_id: i64, idea: &NewIdea) -> Result<Option<i64>> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM ideas WHERE project_id = ?
           AND ((fingerprint IS NOT NULL AND fingerprint = ?) OR lower(trim(title)) = lower(trim(?)))
         LIMIT 1",
    )
    .bind(project_id)
    .bind(idea.fingerprint.as_deref())
    .bind(&idea.title)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Ok(None);
    }
    let now = now_ms();
    let id = sqlx::query_scalar(
        "INSERT INTO ideas (project_id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id,
           expected_impact, confidence, status, source, fingerprint, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'new', ?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(idea.title.trim())
    .bind(idea.problem.trim())
    .bind(serde_json::to_string(&idea.evidence)?)
    .bind(idea.proposal.trim())
    .bind(serde_json::to_string(&idea.acceptance_criteria)?)
    .bind(serde_json::to_string(&idea.pages)?)
    .bind(idea.kpi_id)
    .bind(&idea.expected_impact)
    .bind(&idea.confidence)
    .bind(&idea.source)
    .bind(idea.fingerprint.as_deref())
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(Some(id))
}

pub async fn set_status(
    pool: &SqlitePool,
    project_id: i64,
    id: i64,
    status: IdeaStatus,
    reason: Option<&str>,
) -> Result<Option<Idea>> {
    let now = now_ms();
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    sqlx::query(
        "UPDATE ideas SET
           status = ?1,
           updated_at = ?2,
           dismiss_reason = CASE WHEN ?1 = 'dismissed' THEN ?3 ELSE NULL END,
           released_at = CASE WHEN ?1 = 'released' AND status <> 'released' THEN ?2 ELSE released_at END
         WHERE project_id = ?4 AND id = ?5",
    )
    .bind(status.as_str())
    .bind(now)
    .bind(reason)
    .bind(project_id)
    .bind(id)
    .execute(pool)
    .await?;
    get(pool, project_id, id).await
}

/// Links an idea to a different KPI (or none).
pub async fn set_kpi(pool: &SqlitePool, project_id: i64, id: i64, kpi_id: Option<i64>) -> Result<Option<Idea>> {
    sqlx::query("UPDATE ideas SET kpi_id = ?, updated_at = ? WHERE project_id = ? AND id = ?")
        .bind(kpi_id)
        .bind(now_ms())
        .bind(project_id)
        .bind(id)
        .execute(pool)
        .await?;
    get(pool, project_id, id).await
}
