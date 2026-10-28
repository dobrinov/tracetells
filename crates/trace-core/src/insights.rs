//! Insight storage and lifecycle: new → selected → in progress → released, or dismissed.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::Row;
use crate::Pool;

use crate::model::{Evaluation, Insight, InsightStatus, NewInsight};
use crate::util::now_ms;

const COLS: &str = "id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id, expected_impact,
  confidence, status, dismiss_reason, source, fingerprint, created_at, updated_at, released_at";

fn json_list(row: &AnyRow, col: &str) -> Result<Vec<String>> {
    let raw: String = row.try_get(col)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn from_row(row: &AnyRow) -> Result<Insight> {
    Ok(Insight {
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

async fn evaluations(pool: &Pool, project_id: i64) -> Result<HashMap<i64, Evaluation>> {
    let rows: Vec<EvalRow> = sqlx::query_as(
        "SELECT e.insight_id, e.kpi_id, e.baseline, e.current, e.baseline_n, e.current_n, e.p_value,
                e.verdict, e.summary, e.window_days, e.created_at
         FROM evaluations e JOIN insights i ON i.id = e.insight_id WHERE i.project_id = $1",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for (insight_id, kpi_id, baseline, current, baseline_n, current_n, p_value, verdict, summary, window_days, created_at) in rows {
        out.insert(
            insight_id,
            Evaluation {
                insight_id,
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

pub async fn list(pool: &Pool, project_id: i64, status: Option<InsightStatus>) -> Result<Vec<Insight>> {
    let sql = match status {
        Some(_) => format!("SELECT {COLS} FROM insights WHERE project_id = $1 AND status = $2 ORDER BY updated_at DESC, id DESC"),
        None => format!("SELECT {COLS} FROM insights WHERE project_id = $1 ORDER BY updated_at DESC, id DESC"),
    };
    let mut q = sqlx::query(&sql).bind(project_id);
    if let Some(s) = status {
        q = q.bind(s.as_str());
    }
    let rows = q.fetch_all(pool).await?;
    let mut evals = evaluations(pool, project_id).await?;
    rows.iter()
        .map(|r| {
            let mut insight = from_row(r)?;
            insight.evaluation = evals.remove(&insight.id);
            Ok(insight)
        })
        .collect()
}

pub async fn get(pool: &Pool, project_id: i64, id: i64) -> Result<Option<Insight>> {
    let row = sqlx::query(&format!("SELECT {COLS} FROM insights WHERE project_id = $1 AND id = $2"))
        .bind(project_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut insight = from_row(&row)?;
    insight.evaluation = evaluations(pool, project_id).await?.remove(&id);
    Ok(Some(insight))
}

/// Inserts an insight unless one with the same fingerprint or title already exists
/// (in any status, so dismissed insights stay dismissed). Returns the new id.
pub async fn insert(pool: &Pool, project_id: i64, insight: &NewInsight) -> Result<Option<i64>> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM insights WHERE project_id = $1
           AND ((fingerprint IS NOT NULL AND fingerprint = $2) OR lower(trim(title)) = lower(trim($3)))
         LIMIT 1",
    )
    .bind(project_id)
    .bind(insight.fingerprint.as_deref())
    .bind(&insight.title)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Ok(None);
    }
    let now = now_ms();
    let id = sqlx::query_scalar(
        "INSERT INTO insights (project_id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id,
           expected_impact, confidence, status, source, fingerprint, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'new', $11, $12, $13, $14) RETURNING id",
    )
    .bind(project_id)
    .bind(insight.title.trim())
    .bind(insight.problem.trim())
    .bind(serde_json::to_string(&insight.evidence)?)
    .bind(insight.proposal.trim())
    .bind(serde_json::to_string(&insight.acceptance_criteria)?)
    .bind(serde_json::to_string(&insight.pages)?)
    .bind(insight.kpi_id)
    .bind(&insight.expected_impact)
    .bind(&insight.confidence)
    .bind(&insight.source)
    .bind(insight.fingerprint.as_deref())
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(Some(id))
}

pub async fn set_status(
    pool: &Pool,
    project_id: i64,
    id: i64,
    status: InsightStatus,
    reason: Option<&str>,
) -> Result<Option<Insight>> {
    let now = now_ms();
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    sqlx::query(
        "UPDATE insights SET
           status = $1,
           updated_at = $2,
           dismiss_reason = CASE WHEN $1 = 'dismissed' THEN $3 ELSE NULL END,
           released_at = CASE WHEN $1 = 'released' AND status <> 'released' THEN $2 ELSE released_at END
         WHERE project_id = $4 AND id = $5",
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

/// Links an insight to a different KPI (or none).
pub async fn set_kpi(pool: &Pool, project_id: i64, id: i64, kpi_id: Option<i64>) -> Result<Option<Insight>> {
    sqlx::query("UPDATE insights SET kpi_id = $1, updated_at = $2 WHERE project_id = $3 AND id = $4")
        .bind(kpi_id)
        .bind(now_ms())
        .bind(project_id)
        .bind(id)
        .execute(pool)
        .await?;
    get(pool, project_id, id).await
}
