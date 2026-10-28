//! A compact, aggregated summary of user behaviour over a window. This is what
//! the analyst agent reads instead of raw events, and what the Behaviour page shows.

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;
use crate::Pool;

use crate::kpi;
use crate::util::{DAY_MS, now_ms};

#[derive(Debug, Serialize)]
pub struct Digest {
    pub window: Window,
    pub totals: Totals,
    pub devices: Vec<NamedCount>,
    pub pages: Vec<PageStat>,
    pub transitions: Vec<Transition>,
    pub rage_clicks: Vec<ClickStat>,
    pub dead_clicks: Vec<ClickStat>,
    pub errors: Vec<ErrorStat>,
    pub forms: Vec<FormStat>,
    /// Custom events seen in the window, plus declared events that never arrived.
    pub custom_events: Vec<EventStat>,
    pub kpis: Vec<KpiSnapshot>,
}

#[derive(Debug, Serialize)]
pub struct Window {
    pub from: i64,
    pub to: i64,
    pub days: u32,
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub sessions: i64,
    pub visitors: i64,
    pub pageviews: i64,
    pub events: i64,
}

#[derive(Debug, Serialize)]
pub struct NamedCount {
    pub name: String,
    pub count: i64,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct EventStat {
    pub name: String,
    /// From the event catalogue in trace.json.
    pub description: Option<String>,
    pub declared: bool,
    pub count: i64,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct PageStat {
    pub path: String,
    pub views: i64,
    pub sessions: i64,
    /// Sessions whose last pageview was this page.
    pub exits: i64,
    pub exit_rate: f64,
    pub avg_scroll_pct: Option<f64>,
    pub avg_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct ClickStat {
    pub path: String,
    pub selector: String,
    pub text: Option<String>,
    pub count: i64,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct ErrorStat {
    /// exception, rejection, console, network or resource.
    pub kind: String,
    pub message: String,
    pub source: Option<String>,
    pub path: String,
    pub count: i64,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct FormStat {
    pub path: String,
    pub selector: String,
    pub started: i64,
    pub submitted: i64,
    pub completion_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct KpiSnapshot {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub definition: String,
    pub direction: String,
    pub target: Option<f64>,
    pub current: Option<f64>,
    /// Same-length window immediately before the current one.
    pub previous: Option<f64>,
    pub sessions: usize,
    pub target_met: Option<bool>,
}

impl Digest {
    pub fn page(&self, path: &str) -> Option<&PageStat> {
        self.pages.iter().find(|p| p.path == path)
    }
}

pub async fn build(pool: &Pool, project_id: i64, days: u32) -> Result<Digest> {
    let to = now_ms();
    let from = to - days as i64 * DAY_MS;
    let p = project_id;

    let (sessions, visitors): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(DISTINCT visitor_id) FROM sessions WHERE project_id = $1 AND started_at >= $2 AND started_at < $3",
    )
    .bind(p).bind(from).bind(to)
    .fetch_one(pool).await?;
    let (events, pageviews): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(CASE WHEN type = 'pageview' THEN 1 END) FROM events WHERE project_id = $1 AND ts >= $2 AND ts < $3",
    )
    .bind(p).bind(from).bind(to)
    .fetch_one(pool).await?;

    let devices = sqlx::query_as::<_, (String, i64)>(
        "SELECT COALESCE(device, 'unknown'), COUNT(*) FROM sessions
         WHERE project_id = $1 AND started_at >= $2 AND started_at < $3 GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(name, count)| NamedCount { name, count, sessions: count })
    .collect();

    let exits: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT path, COUNT(*) FROM (
           SELECT path, ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY ts DESC, id DESC) AS nth FROM events
           WHERE project_id = $1 AND type = 'pageview' AND ts >= $2 AND ts < $3
         ) AS last_views WHERE nth = 1 AND path IS NOT NULL GROUP BY path",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .collect();

    let page_avgs: HashMap<String, (Option<f64>, Option<f64>)> = sqlx::query_as::<_, (String, Option<f64>, Option<f64>)>(
        "SELECT path,
           AVG(CASE WHEN type = 'scroll' THEN value END),
           AVG(CASE WHEN type = 'leave' THEN value END) / 1000.0
         FROM events WHERE project_id = $1 AND type IN ('scroll', 'leave') AND ts >= $2 AND ts < $3
         GROUP BY path",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(path, scroll, secs)| (path, (scroll, secs)))
    .collect();

    let pages = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT path, COUNT(*), COUNT(DISTINCT session_id) FROM events
         WHERE project_id = $1 AND type = 'pageview' AND ts >= $2 AND ts < $3 AND path IS NOT NULL
         GROUP BY path ORDER BY 2 DESC LIMIT 25",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(path, views, sessions)| {
        let exits = exits.get(&path).copied().unwrap_or(0);
        let (avg_scroll_pct, avg_seconds) = page_avgs.get(&path).copied().unwrap_or((None, None));
        PageStat {
            exit_rate: if sessions > 0 { exits as f64 / sessions as f64 } else { 0.0 },
            path,
            views,
            sessions,
            exits,
            avg_scroll_pct: avg_scroll_pct.map(round1),
            avg_seconds: avg_seconds.map(round1),
        }
    })
    .collect();

    let transitions = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT prev, path, COUNT(*) FROM (
           SELECT path, LAG(path) OVER (PARTITION BY session_id ORDER BY ts) AS prev FROM events
           WHERE project_id = $1 AND type = 'pageview' AND ts >= $2 AND ts < $3
         ) AS steps WHERE prev IS NOT NULL AND prev <> path GROUP BY prev, path ORDER BY 3 DESC LIMIT 25",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(from, to, count)| Transition { from, to, count })
    .collect();

    let clicks = |kind: &'static str| {
        sqlx::query_as::<_, (String, String, Option<String>, i64, i64)>(
            "SELECT COALESCE(path, ''), COALESCE(selector, ''), MAX(text), COUNT(*), COUNT(DISTINCT session_id) FROM events
             WHERE project_id = $1 AND type = $2 AND ts >= $3 AND ts < $4
             GROUP BY path, selector ORDER BY 5 DESC, 4 DESC LIMIT 15",
        )
        .bind(p).bind(kind).bind(from).bind(to)
        .fetch_all(pool)
    };
    let to_click = |(path, selector, text, count, sessions)| ClickStat { path, selector, text, count, sessions };
    let rage_clicks = clicks("rage_click").await?.into_iter().map(to_click).collect();
    let dead_clicks = clicks("dead_click").await?.into_iter().map(to_click).collect();

    let errors = sqlx::query_as::<_, (String, String, Option<String>, String, i64, i64)>(
        "SELECT COALESCE(name, 'exception'), COALESCE(text, 'Unknown error'), MAX(selector), COALESCE(path, ''),
           COUNT(*), COUNT(DISTINCT session_id) FROM events
         WHERE project_id = $1 AND type = 'error' AND ts >= $2 AND ts < $3
         GROUP BY 1, text, path ORDER BY 6 DESC, 5 DESC LIMIT 20",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(kind, message, source, path, count, sessions)| ErrorStat { kind, message, source, path, count, sessions })
    .collect();

    let forms = sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT COALESCE(path, ''), COALESCE(selector, ''),
           COUNT(DISTINCT CASE WHEN type = 'form_start' THEN session_id END),
           COUNT(DISTINCT CASE WHEN type = 'submit' THEN session_id END)
         FROM events WHERE project_id = $1 AND type IN ('form_start', 'submit') AND ts >= $2 AND ts < $3
         GROUP BY path, selector ORDER BY 3 DESC LIMIT 15",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .filter(|(_, _, started, _)| *started > 0)
    .map(|(path, selector, started, submitted)| FormStat {
        completion_rate: (submitted as f64 / started as f64).min(1.0),
        path,
        selector,
        started,
        submitted,
    })
    .collect();

    let mut catalogue = crate::config::events(pool, p).await?;
    let mut custom_events: Vec<EventStat> = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT name, COUNT(*), COUNT(DISTINCT session_id) FROM events
         WHERE project_id = $1 AND type = 'custom' AND name IS NOT NULL AND ts >= $2 AND ts < $3
         GROUP BY name ORDER BY 2 DESC LIMIT 25",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(name, count, sessions)| {
        let description = catalogue.remove(&name);
        EventStat { declared: description.is_some(), description, name, count, sessions }
    })
    .collect();
    custom_events.extend(catalogue.into_iter().map(|(name, description)| EventStat {
        name,
        description: Some(description),
        declared: true,
        count: 0,
        sessions: 0,
    }));

    let span = to - from;
    let mut kpis = Vec::new();
    for k in kpi::list(pool, p).await? {
        let current = kpi::measure(pool, p, &k.definition, from, to).await?;
        let previous = kpi::measure(pool, p, &k.definition, from - span, from).await?;
        kpis.push(KpiSnapshot {
            target_met: current.value.and_then(|v| kpi::target_met(&k, v)),
            id: k.id,
            kind: k.definition.kind().to_string(),
            definition: k.definition.describe(),
            name: k.name,
            description: k.description,
            direction: k.direction.as_str().to_string(),
            target: k.target,
            current: current.value,
            previous: previous.value,
            sessions: current.n(),
        });
    }

    Ok(Digest {
        window: Window { from, to, days },
        totals: Totals { sessions, visitors, pageviews, events },
        devices,
        pages,
        transitions,
        rage_clicks,
        dead_clicks,
        errors,
        forms,
        custom_events,
        kpis,
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
