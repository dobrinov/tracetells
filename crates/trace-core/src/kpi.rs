//! Reading KPIs (defined in trace.json, see [`crate::config`]) and measuring them over a time window.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use sqlx::SqlitePool;

use crate::model::{Direction, EventMatch, Kpi, KpiDefinition};

/// The value of a KPI over a window, plus one sample per counted session so
/// two windows can be compared statistically.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub value: Option<f64>,
    pub samples: Vec<f64>,
}

impl Measurement {
    pub fn n(&self) -> usize {
        self.samples.len()
    }
}

type KpiRow = (i64, Option<String>, String, String, String, String, Option<f64>, i64);

fn from_row((id, key, name, description, definition, direction, target, created_at): KpiRow) -> Result<Kpi> {
    Ok(Kpi {
        id,
        key,
        name,
        description,
        definition: serde_json::from_str(&definition).context("invalid stored KPI definition")?,
        direction: direction.parse()?,
        target,
        created_at,
    })
}

const COLS: &str = "id, key, name, description, definition, direction, target, created_at";

/// Active KPIs, in the order they were first defined.
pub async fn list(pool: &SqlitePool, project_id: i64) -> Result<Vec<Kpi>> {
    let rows: Vec<KpiRow> =
        sqlx::query_as(&format!("SELECT {COLS} FROM kpis WHERE project_id = ? AND archived_at IS NULL ORDER BY id"))
        .bind(project_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(from_row).collect()
}

/// A KPI by id, including archived ones: released insights keep being evaluated
/// against the KPI they targeted.
pub async fn get(pool: &SqlitePool, project_id: i64, id: i64) -> Result<Option<Kpi>> {
    let row: Option<KpiRow> =
        sqlx::query_as(&format!("SELECT {COLS} FROM kpis WHERE project_id = ? AND id = ?"))
            .bind(project_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    row.map(from_row).transpose()
}

struct Hit {
    session_id: String,
    ts: i64,
}

async fn hits(pool: &SqlitePool, project_id: i64, m: &EventMatch, from: i64, to: i64) -> Result<Vec<Hit>> {
    let rows: Vec<(String, i64, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT session_id, ts, path, selector, name FROM events
         WHERE project_id = ? AND type = ? AND ts >= ? AND ts < ?",
    )
    .bind(project_id)
    .bind(&m.kind)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(_, _, path, sel, name)| m.matches(path.as_deref(), sel.as_deref(), name.as_deref()))
        .map(|(session_id, ts, ..)| Hit { session_id, ts })
        .collect())
}

async fn sessions_started(pool: &SqlitePool, project_id: i64, from: i64, to: i64) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM sessions WHERE project_id = ? AND started_at >= ? AND started_at < ?",
    )
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?)
}

/// Measures a KPI over `[from, to)` (epoch milliseconds).
pub async fn measure(
    pool: &SqlitePool,
    project_id: i64,
    def: &KpiDefinition,
    from: i64,
    to: i64,
) -> Result<Measurement> {
    let samples: Vec<f64> = match def {
        KpiDefinition::Conversion { from: start, to: goal } => {
            let mut first_start: HashMap<String, i64> = HashMap::new();
            for h in hits(pool, project_id, start, from, to).await? {
                first_start
                    .entry(h.session_id)
                    .and_modify(|t| *t = (*t).min(h.ts))
                    .or_insert(h.ts);
            }
            let mut converted: HashSet<String> = HashSet::new();
            for h in hits(pool, project_id, goal, from, to).await? {
                if first_start.get(&h.session_id).is_some_and(|t| h.ts >= *t) {
                    converted.insert(h.session_id);
                }
            }
            first_start
                .keys()
                .map(|s| if converted.contains(s) { 1.0 } else { 0.0 })
                .collect()
        }
        KpiDefinition::SessionRate { matcher } | KpiDefinition::PerSession { matcher } => {
            let mut counts: HashMap<String, f64> = sessions_started(pool, project_id, from, to)
                .await?
                .into_iter()
                .map(|s| (s, 0.0))
                .collect();
            for h in hits(pool, project_id, matcher, from, to).await? {
                if let Some(c) = counts.get_mut(&h.session_id) {
                    *c += 1.0;
                }
            }
            let per_session = matches!(def, KpiDefinition::PerSession { .. });
            counts
                .into_values()
                .map(|c| if per_session { c } else if c > 0.0 { 1.0 } else { 0.0 })
                .collect()
        }
    };
    let value = (!samples.is_empty()).then(|| samples.iter().sum::<f64>() / samples.len() as f64);
    Ok(Measurement { value, samples })
}

/// Whether `value` meets the KPI's target, if it has one.
pub fn target_met(kpi: &Kpi, value: f64) -> Option<bool> {
    kpi.target.map(|t| match kpi.direction {
        Direction::Increase => value >= t,
        Direction::Decrease => value <= t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;
    use crate::{db, ingest};

    async fn seed(pool: &SqlitePool, project_id: i64, sid: &str, paths: &[&str]) {
        let now = now_ms();
        let events = paths
            .iter()
            .enumerate()
            .map(|(i, p)| ingest::IncomingEvent {
                kind: "pageview".into(),
                t: Some(now - 10_000 + i as i64 * 1000),
                path: Some(p.to_string()),
                sel: None,
                text: None,
                name: None,
                value: None,
                props: None,
            })
            .collect();
        let batch = ingest::Batch {
            key: String::new(),
            sid: sid.into(),
            vid: None,
            uid: None,
            vw: Some(1200),
            referrer: None,
            now: Some(now),
            events,
        };
        ingest::store(pool, project_id, batch, None, None, &Default::default()).await.unwrap();
    }

    #[tokio::test]
    async fn conversion_counts_goal_after_start_only() {
        let dir = std::env::temp_dir().join(format!("trace-test-{}.db", crate::util::new_key()));
        let pool = db::connect(dir.to_str().unwrap()).await.unwrap();
        let (_, p) = db::create_account_with_site(&pool, "Test", None).await.unwrap();
        seed(&pool, p.id, "a", &["/", "/checkout", "/checkout/success"]).await;
        seed(&pool, p.id, "b", &["/", "/checkout"]).await;
        seed(&pool, p.id, "c", &["/checkout/success", "/checkout"]).await;
        seed(&pool, p.id, "d", &["/"]).await;

        let def = KpiDefinition::Conversion {
            from: EventMatch { kind: "pageview".into(), path: Some("/checkout".into()), ..Default::default() },
            to: EventMatch { kind: "pageview".into(), path: Some("/checkout/success".into()), ..Default::default() },
        };
        let m = measure(&pool, p.id, &def, 0, now_ms() + 1000).await.unwrap();
        assert_eq!(m.n(), 3);
        assert!((m.value.unwrap() - 1.0 / 3.0).abs() < 1e-9);

        let reached_checkout = KpiDefinition::SessionRate {
            matcher: EventMatch { kind: "pageview".into(), path: Some("/checkout*".into()), ..Default::default() },
        };
        let m = measure(&pool, p.id, &reached_checkout, 0, now_ms() + 1000).await.unwrap();
        assert_eq!(m.n(), 4);
        assert_eq!(m.value, Some(0.75));
        let _ = std::fs::remove_file(dir);
    }
}
