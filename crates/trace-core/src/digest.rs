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
    /// Open errors from the website's servers (see [`crate::errors`]), most harmful first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub server_errors: Vec<ServerErrorStat>,
    pub forms: Vec<FormStat>,
    /// Custom events seen in the window, plus declared events that never arrived.
    pub custom_events: Vec<EventStat>,
    pub kpis: Vec<KpiSnapshot>,
    /// Declared and recognised funnels, with where each loses the most sessions.
    pub funnels: Vec<crate::funnel::FunnelReport>,
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
    /// People where the site names them with `trace.identify()`, once across
    /// days; everyone else by the cookieless visitor hash, which changes daily.
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
    /// Sessions that went from `from` to `to`.
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
pub struct ServerErrorStat {
    pub id: i64,
    pub class: String,
    pub message: String,
    /// `GET /api/reviews/:id → 500`, the busiest first.
    pub routes: Vec<String>,
    pub occurrences: i64,
    /// Browser visits where the same request failed, and their pages.
    pub visits_broken: i64,
    pub pages: Vec<String>,
    pub new: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regressed_in_release: Option<String>,
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

    /// Ingest already does this; doing it again covers events stored before it
    /// did, since this summary is what agents read.
    fn visitor_text_on_one_line(mut self) -> Self {
        use crate::privacy::{one_line, one_line_path};
        let path = |s: &mut String| *s = one_line_path(s);
        let text = |s: &mut String| *s = one_line(s);
        self.pages.iter_mut().for_each(|p| path(&mut p.path));
        for t in &mut self.transitions {
            path(&mut t.from);
            path(&mut t.to);
        }
        for c in self.rage_clicks.iter_mut().chain(self.dead_clicks.iter_mut()) {
            path(&mut c.path);
            text(&mut c.selector);
            if let Some(t) = &mut c.text {
                text(t);
            }
        }
        for e in &mut self.errors {
            path(&mut e.path);
            text(&mut e.message);
            if let Some(s) = &mut e.source {
                text(s);
            }
        }
        for f in &mut self.forms {
            path(&mut f.path);
            text(&mut f.selector);
        }
        self.custom_events.iter_mut().for_each(|e| text(&mut e.name));
        self
    }
}

pub async fn build(pool: &Pool, project_id: i64, days: u32) -> Result<Digest> {
    let to = now_ms();
    let from = to - days as i64 * DAY_MS;
    let p = project_id;

    // A signed-in person is the same visitor every day; the prefix keeps a user
    // id from ever matching a visitor hash.
    let (sessions, visitors): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(DISTINCT COALESCE('u:' || user_id, visitor_id)) FROM sessions
         WHERE project_id = $1 AND started_at >= $2 AND started_at < $3",
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
        "SELECT prev, path, COUNT(DISTINCT session_id) FROM (
           SELECT session_id, path, LAG(path) OVER (PARTITION BY session_id ORDER BY ts) AS prev FROM events
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

    Ok(Digest::visitor_text_on_one_line(Digest {
        window: Window { from, to, days },
        totals: Totals { sessions, visitors, pageviews, events },
        devices,
        pages,
        transitions,
        rage_clicks,
        dead_clicks,
        errors,
        server_errors: crate::errors::report(pool, p, days.min(90), None, false)
            .await?
            .groups
            .into_iter()
            .filter(|g| g.status == "open")
            .take(15)
            .map(|g| ServerErrorStat {
                routes: g.routes.iter().map(|r| format!("{} {} \u{2192} {}", r.method, r.route, r.status).trim().to_string()).collect(),
                pages: g.impact.pages.into_iter().map(|(p, _)| p).collect(),
                id: g.id,
                class: g.class,
                message: g.message,
                occurrences: g.occurrences,
                visits_broken: g.impact.visits,
                new: g.new,
                regressed_in_release: g.regressed_in_release,
            })
            .collect(),
        forms,
        custom_events,
        kpis,
        funnels: crate::funnel::report(pool, p, days.min(90)).await?,
    }))
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signed_in_people_count_once_across_days() {
        let path = crate::db::test_target("digest").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let account = crate::db::create_account(&pool, "Acme", None).await.unwrap().id;
        let site = crate::db::create_site(&pool, account, "Shop").await.unwrap().id;
        let now = now_ms();
        // One person signed in on two days (a new visitor hash each day), one
        // anonymous visitor with two sessions in a day, and another anonymous one.
        for (id, visitor, user, days_ago) in [
            ("s1", "v_monday", Some("u-42"), 3),
            ("s2", "v_thursday", Some("u-42"), 0),
            ("s3", "v_anon", None, 1),
            ("s4", "v_anon", None, 1),
            ("s5", "v_other", None, 2),
        ] {
            let at = now - days_ago * DAY_MS - 60_000;
            sqlx::query("INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at) VALUES ($1, $2, $3, $4, $5, $5)")
                .bind(id)
                .bind(site)
                .bind(visitor)
                .bind(user)
                .bind(at)
                .execute(&pool)
                .await
                .unwrap();
        }
        let totals = build(&pool, site, 14).await.unwrap().totals;
        assert_eq!((totals.sessions, totals.visitors), (5, 3));
        crate::db::drop_test_target(&path).await;
    }
}
