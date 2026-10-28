//! Insight storage and lifecycle: new → selected → in progress → released, or dismissed.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::Row;
use crate::Pool;

use crate::model::{Evaluation, Evidence, Insight, InsightStatus, ItemVerdict, NewInsight, Reading};
use crate::util::{DAY_MS, now_ms};

const COLS: &str = "id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id, expected_impact,
  confidence, status, dismiss_reason, source, fingerprint, created_at, updated_at, released_at,
  created_by, measured_from, measured_to";

/// How long a draft waits for the person's approval.
pub const DRAFT_TTL_MS: i64 = 60 * 60 * 1000;

/// Evidence as stored: measured items and notes, or (from before evidence was
/// measured) plain strings, which read as notes.
fn evidence_list(row: &AnyRow) -> Result<Vec<Evidence>> {
    let raw: String = row.try_get("evidence")?;
    let items: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(items
        .into_iter()
        .map(|v| match v {
            serde_json::Value::String(s) => Evidence::note(s),
            other => serde_json::from_value(other.clone()).unwrap_or_else(|_| Evidence::note(other.to_string())),
        })
        .collect())
}

fn json_list(row: &AnyRow, col: &str) -> Result<Vec<String>> {
    let raw: String = row.try_get(col)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn from_row(row: &AnyRow) -> Result<Insight> {
    Ok(Insight {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        problem: row.try_get("problem")?,
        evidence: evidence_list(row)?,
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
        created_by: row.try_get("created_by")?,
        measured_from: row.try_get("measured_from")?,
        measured_to: row.try_get("measured_to")?,
        readings: Vec::new(),
        item_verdicts: Vec::new(),
    })
}

async fn item_verdicts(pool: &Pool, project_id: i64, only: Option<i64>) -> Result<HashMap<i64, Vec<ItemVerdict>>> {
    let rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT v.insight_id, v.item, v.verdict, v.summary, v.settled_at FROM evaluation_items v JOIN insights i ON i.id = v.insight_id
         WHERE i.project_id = $1 AND ($2 < 0 OR i.id = $2) ORDER BY v.item",
    )
    .bind(project_id)
    .bind(only.unwrap_or(-1))
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<i64, Vec<ItemVerdict>> = HashMap::new();
    for (insight, item, verdict, summary, settled_at) in rows {
        out.entry(insight).or_default().push(ItemVerdict { item: item as usize, verdict: verdict.parse()?, summary, settled_at });
    }
    Ok(out)
}

/// Re-measures an insight's evidence over the same number of days as its
/// first measurement, ending now. The text stays as it was approved.
pub async fn refresh(pool: &Pool, project_id: i64, insight: &Insight) -> Result<Vec<Reading>> {
    let days = match (insight.measured_from, insight.measured_to) {
        (Some(f), Some(t)) if t > f => (((t - f) as f64 / DAY_MS as f64).round() as u32).clamp(1, 90),
        _ => 14,
    };
    let readings = measure_evidence(pool, project_id, &insight.evidence, days, "refresh").await?;
    store_readings(pool, insight.id, &readings).await?;
    if insight.measured_from.is_none() && !readings.is_empty() {
        // Evidence measured for the first time (an older insight): this is its window.
        sqlx::query("UPDATE insights SET measured_from = $1, measured_to = $2 WHERE id = $3")
            .bind(readings[0].from)
            .bind(readings[0].to)
            .bind(insight.id)
            .execute(pool)
            .await?;
    }
    Ok(readings)
}

/// Refreshes insights that aren't dismissed and weren't measured in the last
/// day, at most `limit` of them. Returns how many were refreshed.
pub async fn refresh_due(pool: &Pool, limit: i64) -> Result<usize> {
    let due: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT i.id, i.project_id FROM insights i
         WHERE i.status <> 'dismissed'
           AND EXISTS (SELECT 1 FROM insight_measurements m WHERE m.insight_id = i.id)
           AND (SELECT MAX(m.measured_at) FROM insight_measurements m WHERE m.insight_id = i.id AND m.kind IN ('first', 'refresh')) < $1
         ORDER BY i.id LIMIT $2",
    )
    .bind(now_ms() - DAY_MS)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (id, project_id) in due {
        if let Some(insight) = get(pool, project_id, id).await? {
            refresh(pool, project_id, &insight).await?;
            n += 1;
        }
    }
    Ok(n)
}

pub async fn store_item_verdict(pool: &Pool, insight_id: i64, v: &ItemVerdict) -> Result<()> {
    sqlx::query("INSERT INTO evaluation_items (insight_id, item, verdict, summary, settled_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(insight_id)
        .bind(v.item as i64)
        .bind(v.verdict.as_str())
        .bind(&v.summary)
        .bind(v.settled_at)
        .execute(pool)
        .await?;
    Ok(())
}

type ReadingRow = (i64, i64, String, Option<f64>, i64, i64, i64, i64);

/// Every reading of a website's insights (or one insight's), by insight, oldest first.
async fn readings(pool: &Pool, project_id: i64, only: Option<i64>) -> Result<HashMap<i64, Vec<Reading>>> {
    let rows: Vec<ReadingRow> = sqlx::query_as(
        "SELECT m.insight_id, m.item, m.kind, m.value, m.sessions, m.window_from, m.window_to, m.measured_at
         FROM insight_measurements m JOIN insights i ON i.id = m.insight_id
         WHERE i.project_id = $1 AND ($2 < 0 OR i.id = $2) ORDER BY m.measured_at, m.id",
    )
    .bind(project_id)
    .bind(only.unwrap_or(-1))
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<i64, Vec<Reading>> = HashMap::new();
    for (insight, item, kind, value, sessions, from, to, at) in rows {
        out.entry(insight).or_default().push(Reading { item: item as usize, kind, value, formatted: None, sessions, from, to, at });
    }
    Ok(out)
}

/// Attaches readings, with each value formatted as its metric shows it.
fn with_readings(mut insight: Insight, readings: Option<Vec<Reading>>) -> Insight {
    let mut rs = readings.unwrap_or_default();
    for r in &mut rs {
        r.formatted = r.value.zip(insight.evidence.get(r.item).and_then(Evidence::metric)).map(|(v, m)| m.format_value(v));
    }
    insight.readings = rs;
    insight
}

/// Measures every measured evidence item over the last `days`.
pub async fn measure_evidence(pool: &Pool, project_id: i64, evidence: &[Evidence], days: u32, kind: &str) -> Result<Vec<Reading>> {
    let to = now_ms();
    let from = to - i64::from(days) * DAY_MS;
    measure_window(pool, project_id, evidence, from, to, kind).await
}

pub async fn measure_window(pool: &Pool, project_id: i64, evidence: &[Evidence], from: i64, to: i64, kind: &str) -> Result<Vec<Reading>> {
    let mut out = Vec::new();
    for (item, e) in evidence.iter().enumerate() {
        let Some(metric) = e.metric() else { continue };
        let v = crate::query::measure(pool, project_id, metric, from, to).await?;
        out.push(Reading {
            item,
            kind: kind.to_string(),
            formatted: v.value.map(|x| metric.format_value(x)),
            value: v.value,
            sessions: v.sessions,
            from,
            to,
            at: now_ms(),
        });
    }
    Ok(out)
}

pub async fn store_readings(pool: &Pool, insight_id: i64, readings: &[Reading]) -> Result<()> {
    for r in readings {
        sqlx::query(
            "INSERT INTO insight_measurements (insight_id, item, kind, value, sessions, window_from, window_to, measured_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(insight_id)
        .bind(r.item as i64)
        .bind(&r.kind)
        .bind(crate::db::Real(r.value))
        .bind(r.sessions)
        .bind(r.from)
        .bind(r.to)
        .bind(r.at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

type EvalRow = (i64, i64, Option<f64>, Option<f64>, i64, i64, Option<f64>, String, String, f64, i64, Option<i64>);

async fn evaluations(pool: &Pool, project_id: i64) -> Result<HashMap<i64, Evaluation>> {
    let rows: Vec<EvalRow> = sqlx::query_as(
        "SELECT e.insight_id, e.kpi_id, e.baseline, e.current, e.baseline_n, e.current_n, e.p_value,
                e.verdict, e.summary, e.window_days, e.created_at, e.settled_at
         FROM evaluations e JOIN insights i ON i.id = e.insight_id WHERE i.project_id = $1",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for (insight_id, kpi_id, baseline, current, baseline_n, current_n, p_value, verdict, summary, window_days, created_at, settled_at) in rows {
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
                settled_at,
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
    let mut all = readings(pool, project_id, None).await?;
    let mut verdicts = item_verdicts(pool, project_id, None).await?;
    rows.iter()
        .map(|r| {
            let mut insight = from_row(r)?;
            insight.evaluation = evals.remove(&insight.id);
            insight.item_verdicts = verdicts.remove(&insight.id).unwrap_or_default();
            let rs = all.remove(&insight.id);
            Ok(with_readings(insight, rs))
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
    insight.item_verdicts = item_verdicts(pool, project_id, Some(id)).await?.remove(&id).unwrap_or_default();
    let rs = readings(pool, project_id, Some(id)).await?.remove(&id);
    Ok(Some(with_readings(insight, rs)))
}

/// An existing insight with this fingerprint or title, in any status, so
/// dismissed findings stay dismissed.
pub async fn duplicate(pool: &Pool, project_id: i64, fingerprint: Option<&str>, title: &str) -> Result<Option<(i64, InsightStatus)>> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, status FROM insights WHERE project_id = $1
           AND ((fingerprint IS NOT NULL AND fingerprint = $2) OR lower(trim(title)) = lower(trim($3)))
         LIMIT 1",
    )
    .bind(project_id)
    .bind(fingerprint)
    .bind(title)
    .fetch_optional(pool)
    .await?;
    row.map(|(id, s)| Ok((id, s.parse()?))).transpose()
}

/// Keeps a measured, validated insight until the person approves it. Returns its id.
pub async fn create_draft(pool: &Pool, project_id: i64, insight: &NewInsight) -> Result<String> {
    let now = now_ms();
    sqlx::query("DELETE FROM insight_drafts WHERE created_at < $1").bind(now - DRAFT_TTL_MS).execute(pool).await?;
    let id = format!("draft_{}", crate::auth::random_token(12));
    sqlx::query("INSERT INTO insight_drafts (id, project_id, payload, created_at) VALUES ($1, $2, $3, $4)")
        .bind(&id)
        .bind(project_id)
        .bind(serde_json::to_string(insight)?)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(id)
}

/// Takes a draft out, if it belongs to this website and is less than an hour old.
pub async fn take_draft(pool: &Pool, project_id: i64, id: &str) -> Result<Option<NewInsight>> {
    let row: Option<(String, i64)> = sqlx::query_as("DELETE FROM insight_drafts WHERE id = $1 AND project_id = $2 RETURNING payload, created_at")
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some((payload, at)) if at >= now_ms() - DRAFT_TTL_MS => Some(serde_json::from_str(&payload)?),
        _ => None,
    })
}

/// Inserts an insight unless one with the same fingerprint or title already exists
/// (in any status, so dismissed insights stay dismissed). Returns the new id.
/// Measured evidence without readings (an insight from Trace's own analysis)
/// is measured now, over the last 14 days.
pub async fn insert(pool: &Pool, project_id: i64, insight: &NewInsight) -> Result<Option<i64>> {
    if duplicate(pool, project_id, insight.fingerprint.as_deref(), &insight.title).await?.is_some() {
        return Ok(None);
    }
    let readings = if insight.readings.is_empty() {
        measure_evidence(pool, project_id, &insight.evidence, 14, "first").await?
    } else {
        insight.readings.clone()
    };
    let now = now_ms();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO insights (project_id, title, problem, evidence, proposal, acceptance_criteria, pages, kpi_id,
           expected_impact, confidence, status, source, fingerprint, created_at, updated_at, created_by, measured_from, measured_to)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'new', $11, $12, $13, $14, $15, $16, $17) RETURNING id",
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
    .bind(insight.created_by.as_deref())
    .bind(readings.iter().map(|r| r.from).min())
    .bind(readings.iter().map(|r| r.to).max())
    .fetch_one(pool)
    .await?;
    store_readings(pool, id, &readings).await?;
    Ok(Some(id))
}

/// Moves an insight to `status`. Releasing records when the change went live:
/// `released_at` when given (a deploy's time), otherwise now. Giving it again
/// for a released insight corrects the time and starts its measurement over.
pub async fn set_status(
    pool: &Pool,
    project_id: i64,
    id: i64,
    status: InsightStatus,
    reason: Option<&str>,
    released_at: Option<i64>,
) -> Result<Option<Insight>> {
    let Some(before) = get(pool, project_id, id).await? else { return Ok(None) };
    let now = now_ms();
    if let Some(at) = released_at {
        anyhow::ensure!(status == InsightStatus::Released, "a release time only goes with the status \"released\"");
        anyhow::ensure!(at <= now + 5 * 60_000, "the release time is in the future");
        anyhow::ensure!(
            at >= before.created_at,
            "the release time is before Trace found this insight ({}), so the change can't be its fix",
            crate::util::date(before.created_at)
        );
    }
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    sqlx::query(
        "UPDATE insights SET
           status = $1,
           updated_at = $2,
           dismiss_reason = CASE WHEN $1 = 'dismissed' THEN $3 ELSE NULL END
         WHERE project_id = $4 AND id = $5",
    )
    .bind(status.as_str())
    .bind(now)
    .bind(reason)
    .bind(project_id)
    .bind(id)
    .execute(pool)
    .await?;
    if status == InsightStatus::Released && (before.status != InsightStatus::Released || released_at.is_some()) {
        sqlx::query("UPDATE insights SET released_at = $1 WHERE id = $2").bind(released_at.unwrap_or(now)).bind(id).execute(pool).await?;
        snapshot_release(pool, id).await?;
    }
    get(pool, project_id, id).await
}

/// Keeps the linked KPI's definition as it is now, for judging the release,
/// and drops any measurement made against another time or KPI.
async fn snapshot_release(pool: &Pool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE insights SET
           released_definition = (SELECT definition FROM kpis WHERE kpis.id = insights.kpi_id),
           released_direction = (SELECT direction FROM kpis WHERE kpis.id = insights.kpi_id)
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM evaluations WHERE insight_id = $1").bind(id).execute(pool).await?;
    sqlx::query("DELETE FROM evaluation_items WHERE insight_id = $1").bind(id).execute(pool).await?;
    sqlx::query("DELETE FROM insight_measurements WHERE insight_id = $1 AND kind IN ('before', 'after')").bind(id).execute(pool).await?;
    Ok(())
}

/// The KPI definition and direction a released insight is judged by, as
/// stored at release. None for releases from before Trace kept them.
pub async fn released_kpi(pool: &Pool, id: i64) -> Result<Option<(String, String)>> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT released_definition, released_direction FROM insights WHERE id = $1").bind(id).fetch_optional(pool).await?;
    Ok(row.and_then(|(d, dir)| d.zip(dir)))
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
    // A released insight linked to another KPI is judged by that one from now on.
    if get(pool, project_id, id).await?.is_some_and(|i| i.status == InsightStatus::Released) {
        snapshot_release(pool, id).await?;
    }
    get(pool, project_id, id).await
}
