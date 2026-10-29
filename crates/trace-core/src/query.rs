//! Aggregated questions about behaviour: one measurement of a website's events
//! over a window, optionally broken down by page, element, device or day. The
//! same `Metric` is what insight evidence stores, and it uses the KPI matcher
//! language, so questions, evidence and KPIs can't drift apart.
//!
//! Matching runs in SQL (exact or prefix), unlike `kpi::measure`, so questions
//! stay cheap on busy websites. Answers are aggregates only: values with the
//! number of sessions behind them, never sessions, visitors or single events.

use std::collections::HashMap;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Pool;
use crate::model::{EventMatch, KpiDefinition};
use crate::util::{DAY_MS, date, now_ms};

/// Breakdown rows from fewer sessions than this are added into "other".
pub const MIN_ROW_SESSIONS: i64 = 3;
pub const MAX_ROWS: usize = 50;
pub const MAX_DAYS: u32 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// Sessions with a matching event.
    Sessions,
    /// Matching events.
    Events,
    /// Of the sessions that started in the window, the share with a matching event.
    SessionRate,
    /// Matching events per session that started in the window.
    PerSession,
    /// Of the sessions with a `from` event, the share with a `to` event after it.
    Conversion,
}

impl Measure {
    pub fn is_rate(self) -> bool {
        matches!(self, Self::SessionRate | Self::Conversion)
    }
}

/// One measurement: what to count, and how.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub measure: Measure,
    /// The events counted (every measure but conversion).
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub matcher: Option<EventMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<EventMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<EventMatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Breakdown {
    Path,
    Selector,
    Device,
    Day,
}

impl Metric {
    /// Reads a metric from an agent's JSON, accepting trace.json's matcher shorthands.
    pub fn parse(v: &Value) -> Result<Metric> {
        let obj = v.as_object().ok_or_else(|| anyhow::anyhow!("a metric is an object, e.g. {{\"measure\": \"session_rate\", \"match\": {{\"rage_click\": \"button#pay\"}}}}"))?;
        let measure: Measure = serde_json::from_value(obj.get("measure").cloned().unwrap_or(Value::Null))
            .map_err(|_| anyhow::anyhow!("`measure` is sessions, events, session_rate, per_session or conversion"))?;
        let matcher = |key: &str| -> Result<EventMatch> {
            let m = obj.get(key).ok_or_else(|| anyhow::anyhow!("`{key}` is required for {measure:?}"))?;
            let m = crate::config::parse_matcher(m, key).map_err(|e| anyhow::anyhow!(e))?;
            for (field, text) in [("path", &m.path), ("selector", &m.selector), ("name", &m.name), ("text", &m.text)] {
                if let Some(t) = text {
                    ensure!(t.chars().count() <= 200, "`{key}.{field}` is longer than 200 characters");
                    ensure!(!t.chars().any(char::is_control), "`{key}.{field}` can't contain control characters");
                }
            }
            Ok(m)
        };
        let metric = if measure == Measure::Conversion {
            Metric { measure, matcher: None, from: Some(matcher("from")?), to: Some(matcher("to")?) }
        } else {
            Metric { measure, matcher: Some(matcher("match")?), from: None, to: None }
        };
        Ok(metric)
    }

    /// The same measurement as a KPI definition, where one exists, so it can
    /// be judged like a KPI (`evaluate::welch`).
    pub fn as_kpi(&self) -> Option<KpiDefinition> {
        match self.measure {
            Measure::SessionRate => Some(KpiDefinition::SessionRate { matcher: self.matcher.clone()? }),
            Measure::PerSession => Some(KpiDefinition::PerSession { matcher: self.matcher.clone()? }),
            Measure::Conversion => Some(KpiDefinition::Conversion { from: self.from.clone()?, to: self.to.clone()? }),
            Measure::Sessions | Measure::Events => None,
        }
    }

    /// In words, for people and agents.
    pub fn describe(&self) -> String {
        let m = |m: &Option<EventMatch>| m.as_ref().map(describe_match).unwrap_or_default();
        match self.measure {
            Measure::Sessions => format!("sessions with {}", m(&self.matcher)),
            Measure::Events => format!("{} events", m(&self.matcher)),
            Measure::SessionRate => format!("share of sessions with {}", m(&self.matcher)),
            Measure::PerSession => format!("{} per session", m(&self.matcher)),
            Measure::Conversion => format!("sessions with {} that go on to {}", m(&self.from), m(&self.to)),
        }
    }

    pub fn format_value(&self, v: f64) -> String {
        match self.measure {
            Measure::SessionRate | Measure::Conversion => crate::util::pct(v),
            Measure::PerSession => format!("{v:.2}"),
            Measure::Sessions | Measure::Events => format!("{}", v.round() as i64),
        }
    }
}

fn describe_match(m: &EventMatch) -> String {
    let mut s = match m.kind.as_str() {
        "custom" => format!("custom \"{}\"", m.name.as_deref().unwrap_or("*")),
        k => k.replace('_', " "),
    };
    if m.kind != "custom" && let Some(n) = &m.name {
        s.push_str(&format!(" {n}"));
    }
    if let Some(sel) = &m.selector {
        s.push_str(&format!(" on `{sel}`"));
    }
    if let Some(t) = m.text.as_deref().filter(|t| *t != "*") {
        s.push_str(&format!(" \"{t}\""));
    }
    if let Some(p) = &m.path {
        s.push_str(&format!(" at {p}"));
    }
    s
}

/// One question: a metric over the last `days`, optionally broken down and
/// compared with the window before.
#[derive(Debug, Clone)]
pub struct Query {
    pub metric: Metric,
    pub days: u32,
    pub breakdown: Option<Breakdown>,
    pub compare: bool,
}

impl Query {
    pub fn parse(v: &Value) -> Result<Query> {
        let metric = Metric::parse(v.get("metric").unwrap_or(v))?;
        let days = match v.get("days") {
            None | Some(Value::Null) => 14,
            Some(d) => d.as_u64().filter(|d| (1..=MAX_DAYS as u64).contains(d)).ok_or_else(|| anyhow::anyhow!("`days` is 1 to {MAX_DAYS}"))? as u32,
        };
        let breakdown = match v.get("breakdown").and_then(Value::as_str) {
            None => None,
            Some(b) => Some(serde_json::from_value::<Breakdown>(json!(b)).map_err(|_| {
                anyhow::anyhow!("`breakdown` is path, selector, device or day; Trace has no per-session or per-visitor breakdowns")
            })?),
        };
        if metric.measure == Measure::Conversion && matches!(breakdown, Some(Breakdown::Path | Breakdown::Selector)) {
            bail!("a conversion can be broken down by device or day, not by {}", if breakdown == Some(Breakdown::Path) { "path" } else { "selector" });
        }
        let compare = v.get("compare").and_then(Value::as_bool).unwrap_or(false);
        Ok(Query { metric, days, breakdown, compare })
    }
}

/// A measured value and how many sessions it covers.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Value1 {
    pub value: Option<f64>,
    /// The sessions the value is about: the base of a rate, or the sessions counted.
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub key: String,
    pub value: Option<f64>,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub measures: String,
    pub from: String,
    pub to: String,
    pub days: u32,
    pub value: Option<f64>,
    pub formatted: Option<String>,
    pub sessions: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<Breakdown>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Row>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<Value1>,
}

/// Answers a question over the last `days`, ending now.
pub async fn ask(pool: &Pool, site: i64, q: &Query) -> Result<Answer> {
    let to = now_ms();
    let span = i64::from(q.days) * DAY_MS;
    let (total, rows) = measure_grouped(pool, site, &q.metric, to - span, to, q.breakdown).await?;
    let previous = if q.compare { Some(measure_grouped(pool, site, &q.metric, to - 2 * span, to - span, None).await?.0) } else { None };
    Ok(Answer {
        measures: q.metric.describe(),
        from: date(to - span),
        to: date(to),
        days: q.days,
        formatted: total.value.map(|v| q.metric.format_value(v)),
        value: total.value,
        sessions: total.sessions,
        breakdown: q.breakdown,
        rows,
        previous,
    })
}

/// One measurement over `[from, to)`, with no breakdown.
pub async fn measure(pool: &Pool, site: i64, metric: &Metric, from: i64, to: i64) -> Result<Value1> {
    Ok(measure_grouped(pool, site, metric, from, to, None).await?.0)
}

/// Per-session samples for a significance test: 0/1 for rates, counts per session for `per_session`.
pub async fn samples(pool: &Pool, site: i64, metric: &Metric, from: i64, to: i64) -> Result<Vec<f64>> {
    match metric.measure {
        Measure::SessionRate | Measure::Conversion => {
            let c = counts(pool, site, metric, from, to, None).await?;
            let (k, n) = c.get("").map(|c| (c.hits, c.base)).unwrap_or((0, 0));
            Ok((0..n).map(|i| if i < k { 1.0 } else { 0.0 }).collect())
        }
        Measure::PerSession => {
            let m = metric.matcher.as_ref().expect("per_session has a matcher");
            let (cond, binds) = matcher_sql(m, 4);
            let sql = format!(
                "SELECT COUNT(*) FROM events e JOIN sessions s ON s.id = e.session_id AND s.project_id = e.project_id
                 WHERE e.project_id = $1 AND e.ts >= $2 AND e.ts < $3 AND s.started_at >= $2 AND s.started_at < $3 AND {cond}
                 GROUP BY e.session_id"
            );
            let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(site).bind(from).bind(to);
            for b in &binds {
                query = query.bind(b);
            }
            let per: Vec<i64> = query.fetch_all(pool).await?;
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = $1 AND started_at >= $2 AND started_at < $3")
                .bind(site).bind(from).bind(to).fetch_one(pool).await?;
            let mut out: Vec<f64> = per.iter().map(|c| *c as f64).collect();
            out.resize(n.max(per.len() as i64) as usize, 0.0);
            Ok(out)
        }
        Measure::Sessions | Measure::Events => bail!("{:?} has no per-session samples", metric.measure),
    }
}

/// Counts behind one row.
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    /// Sessions with a match (converted sessions, for conversion).
    hits: i64,
    /// Matching events.
    events: i64,
    /// The base of a rate: sessions started (session rate, per session) or with `from` (conversion).
    base: i64,
}

impl Counts {
    fn add(&mut self, o: Counts) {
        self.hits += o.hits;
        self.events += o.events;
        self.base += o.base;
    }
    fn value(self, m: Measure) -> Value1 {
        let ratio = |a: i64, b: i64| (b > 0).then(|| a as f64 / b as f64);
        match m {
            Measure::Sessions => Value1 { value: Some(self.hits as f64), sessions: self.hits },
            Measure::Events => Value1 { value: Some(self.events as f64), sessions: self.hits },
            Measure::SessionRate | Measure::Conversion => Value1 { value: ratio(self.hits, self.base), sessions: self.base },
            Measure::PerSession => Value1 { value: ratio(self.events, self.base), sessions: self.base },
        }
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// SQL conditions on `e` for a matcher, with its binds numbered from `first`.
fn matcher_sql(m: &EventMatch, first: usize) -> (String, Vec<String>) {
    let mut conds = vec![format!("e.type = ${first}")];
    let mut binds = vec![m.kind.clone()];
    for (col, pattern) in [("path", &m.path), ("selector", &m.selector), ("name", &m.name), ("text", &m.text)] {
        let Some(p) = pattern.as_deref().map(str::trim).filter(|p| !p.is_empty() && *p != "*") else { continue };
        let n = first + binds.len();
        match p.strip_suffix('*') {
            Some(prefix) => {
                conds.push(format!("e.{col} LIKE ${n} ESCAPE '\\'"));
                binds.push(format!("{}%", escape_like(prefix)));
            }
            None => {
                conds.push(format!("e.{col} = ${n}"));
                binds.push(p.to_string());
            }
        }
    }
    (conds.join(" AND "), binds)
}

/// The SQL expression a breakdown groups by, on `e` (events) and `s` (sessions).
fn group_sql(b: Option<Breakdown>) -> &'static str {
    match b {
        None => "''",
        Some(Breakdown::Path) => "COALESCE(e.path, '')",
        Some(Breakdown::Selector) => "COALESCE(e.selector, '')",
        Some(Breakdown::Device) => "COALESCE(s.device, 'unknown')",
        Some(Breakdown::Day) => "CAST(s.started_at / 86400000 AS TEXT)",
    }
}

async fn grouped(pool: &Pool, sql: &str, site: i64, from: i64, to: i64, binds: &[String]) -> Result<Vec<(String, i64, i64)>> {
    let mut q = sqlx::query_as::<_, (String, i64, i64)>(sql).bind(site).bind(from).bind(to);
    for b in binds {
        q = q.bind(b);
    }
    Ok(q.fetch_all(pool).await?)
}

/// Counts per breakdown value ("" without a breakdown).
async fn counts(pool: &Pool, site: i64, metric: &Metric, from: i64, to: i64, b: Option<Breakdown>) -> Result<HashMap<String, Counts>> {
    let mut out: HashMap<String, Counts> = HashMap::new();
    let key = group_sql(b);
    let started = "s.started_at >= $2 AND s.started_at < $3";
    let in_window = "e.project_id = $1 AND e.ts >= $2 AND e.ts < $3";
    let join = "JOIN sessions s ON s.id = e.session_id AND s.project_id = e.project_id";
    match metric.measure {
        Measure::Sessions | Measure::Events => {
            let (cond, binds) = matcher_sql(metric.matcher.as_ref().expect("a matcher"), 4);
            let sql = format!("SELECT {key}, COUNT(DISTINCT e.session_id), COUNT(*) FROM events e {join} WHERE {in_window} AND {cond} GROUP BY 1");
            for (k, hits, events) in grouped(pool, &sql, site, from, to, &binds).await? {
                out.entry(k).or_default().add(Counts { hits, events, base: 0 });
            }
        }
        Measure::SessionRate | Measure::PerSession => {
            let (cond, binds) = matcher_sql(metric.matcher.as_ref().expect("a matcher"), 4);
            let sql = format!(
                "SELECT {key}, COUNT(DISTINCT e.session_id), COUNT(*) FROM events e {join} WHERE {in_window} AND {started} AND {cond} GROUP BY 1"
            );
            for (k, hits, events) in grouped(pool, &sql, site, from, to, &binds).await? {
                out.entry(k).or_default().add(Counts { hits, events, base: 0 });
            }
            // The base: sessions that started in the window, per device or day;
            // for page and element breakdowns, every session in the window.
            let base_key = match b {
                Some(Breakdown::Device | Breakdown::Day) => key,
                _ => "''",
            };
            let sql = format!("SELECT {base_key}, COUNT(*), CAST(0 AS BIGINT) FROM sessions s WHERE s.project_id = $1 AND {started} GROUP BY 1");
            let bases = grouped(pool, &sql, site, from, to, &[]).await?;
            if base_key == "''" {
                let total = bases.first().map(|r| r.1).unwrap_or(0);
                if out.is_empty() {
                    out.insert(String::new(), Counts::default());
                }
                out.values_mut().for_each(|c| c.base = total);
            } else {
                for (k, base, _) in bases {
                    out.entry(k).or_default().base = base;
                }
            }
        }
        Measure::Conversion => {
            let (from_cond, from_binds) = matcher_sql(metric.from.as_ref().expect("from"), 4);
            let sql = format!("SELECT e.session_id, MIN(e.ts), MIN(s.started_at) FROM events e {join} WHERE {in_window} AND {from_cond} GROUP BY e.session_id");
            let starts = grouped(pool, &sql, site, from, to, &from_binds).await?;
            let (to_cond, to_binds) = matcher_sql(metric.to.as_ref().expect("to"), 4);
            let sql = format!("SELECT e.session_id, MAX(e.ts), CAST(0 AS BIGINT) FROM events e WHERE {in_window} AND {to_cond} GROUP BY e.session_id");
            let goals: HashMap<String, i64> = grouped(pool, &sql, site, from, to, &to_binds).await?.into_iter().map(|(s, t, _)| (s, t)).collect();
            let devices: HashMap<String, String> = if b == Some(Breakdown::Device) {
                sqlx::query_as::<_, (String, String)>(
                    "SELECT s.id, COALESCE(s.device, 'unknown') FROM sessions s WHERE s.project_id = $1 AND s.last_seen_at >= $2 AND s.started_at < $3",
                )
                .bind(site)
                .bind(from)
                .bind(to)
                .fetch_all(pool)
                .await?
                .into_iter()
                .collect()
            } else {
                HashMap::new()
            };
            for (session, first, started_at) in starts {
                let k = match b {
                    Some(Breakdown::Device) => devices.get(&session).cloned().unwrap_or_else(|| "unknown".into()),
                    Some(Breakdown::Day) => date(started_at),
                    _ => String::new(),
                };
                let converted = goals.get(&session).is_some_and(|t| *t >= first);
                out.entry(k).or_default().add(Counts { hits: i64::from(converted), events: 0, base: 1 });
            }
        }
    }
    // Days come back as day numbers from SQL; show them as dates.
    if b == Some(Breakdown::Day) && metric.measure != Measure::Conversion {
        out = out.into_iter().map(|(k, c)| (k.parse::<i64>().map(|d| date(d * DAY_MS)).unwrap_or(k), c)).collect();
    }
    Ok(out)
}

async fn measure_grouped(pool: &Pool, site: i64, metric: &Metric, from: i64, to: i64, b: Option<Breakdown>) -> Result<(Value1, Vec<Row>)> {
    // The total is its own count: summing rows would count a session once per page it touched.
    let total = counts(pool, site, metric, from, to, None).await?.remove("").unwrap_or_default();
    let Some(b) = b else { return Ok((total.value(metric.measure), Vec::new())) };
    let per_key = counts(pool, site, metric, from, to, Some(b)).await?;
    // Page and element rows of a session rate share the website's base; device and day rows have their own.
    let shared_base = matches!(b, Breakdown::Path | Breakdown::Selector);
    let covered = |c: &Counts| match metric.measure {
        Measure::Sessions | Measure::Events => c.hits,
        Measure::SessionRate | Measure::PerSession if shared_base => c.hits,
        _ => c.base,
    };
    let add = |into: &mut Counts, c: Counts| {
        into.hits += c.hits;
        into.events += c.events;
        into.base = if shared_base { c.base } else { into.base + c.base };
    };
    // Rows below the minimum are added into "other", so no row describes one or two visits.
    let mut kept: Vec<(String, Counts)> = Vec::new();
    let mut other = Counts::default();
    for (k, c) in per_key {
        if covered(&c) >= MIN_ROW_SESSIONS && !k.is_empty() {
            kept.push((crate::privacy::one_line(&k), c));
        } else {
            add(&mut other, c);
        }
    }
    let sort_key = |c: &Counts| if metric.measure == Measure::Events { c.events } else { covered(c) };
    kept.sort_by(|a, b| sort_key(&b.1).cmp(&sort_key(&a.1)).then_with(|| a.0.cmp(&b.0)));
    if kept.len() > MAX_ROWS {
        for (_, c) in kept.split_off(MAX_ROWS) {
            add(&mut other, c);
        }
    }
    let mut rows: Vec<Row> = kept
        .into_iter()
        .map(|(key, c)| {
            let v = c.value(metric.measure);
            Row { key, value: v.value, sessions: v.sessions }
        })
        .collect();
    if other.hits > 0 || other.events > 0 || (!shared_base && other.base > 0) {
        let v = other.value(metric.measure);
        rows.push(Row { key: "other".into(), value: v.value, sessions: v.sessions });
    }
    Ok((total.value(metric.measure), rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One session: its device, and events as (type, path, selector, name) a second apart.
    async fn session(pool: &Pool, site: i64, id: &str, device: &str, at: i64, events: &[(&str, &str, &str, &str)]) {
        sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at, device) VALUES ($1, $2, $3, $3, $4)")
            .bind(id).bind(site).bind(at).bind(device).execute(pool).await.unwrap();
        for (i, (kind, path, sel, name)) in events.iter().enumerate() {
            let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, selector, name) VALUES ($1, $2, $3, $4, $5, $6, $7)")
                .bind(site).bind(id).bind(at + i as i64 * 1000).bind(*kind).bind(opt(path)).bind(opt(sel)).bind(opt(name))
                .execute(pool).await.unwrap();
        }
    }

    fn q(v: Value) -> Query {
        Query::parse(&v).unwrap()
    }

    #[tokio::test]
    async fn errors_match_by_their_message() {
        let target = crate::db::test_target("query_text").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let t = now_ms() - DAY_MS;
        session(&pool, site.id, "a", "desktop", t, &[("pageview", "/sign-in", "", "")]).await;
        session(&pool, site.id, "b", "desktop", t, &[("pageview", "/sign-in", "", "")]).await;
        for (id, text) in [("a", "POST stats.example.com/api/event \u{2192} failed"), ("a", "POST stats.example.com/api/event \u{2192} failed"), ("b", "POST /sign-in \u{2192} 422")] {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, name, text) VALUES ($1, $2, $3, 'error', '/sign-in', 'network', $4)")
                .bind(site.id).bind(id).bind(t + 5000).bind(text).execute(&pool).await.unwrap();
        }
        let count = |m: Value| {
            let pool = pool.clone();
            async move { ask(&pool, site.id, &q(json!({ "measure": "events", "match": m, "days": 7 }))).await.unwrap().value }
        };
        assert_eq!(count(json!({ "error": "*", "text": "POST stats.example.com/*" })).await, Some(2.0));
        assert_eq!(count(json!({ "type": "error", "name": "network", "text": "POST /sign-in \u{2192} 422" })).await, Some(1.0));
        assert_eq!(count(json!({ "error": "/sign-in" })).await, Some(3.0), "without text, every error counts as before");
        // KPIs and funnels read the same events through the scan.
        let m = crate::config::parse_matcher(&json!({ "error": "*", "text": "POST stats.example.com/*" }), "m").unwrap();
        let hits = crate::kpi::hits(&pool, site.id, &m, t - 1, t + DAY_MS).await.unwrap();
        assert_eq!(hits.iter().map(|h| h.session_id.as_str()).collect::<Vec<_>>(), ["a", "a"]);
        crate::db::drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn measures_breakdowns_and_agreement_with_kpis() {
        let target = crate::db::test_target("query").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let s = site.id;
        let t = now_ms() - 2 * DAY_MS;
        // 4 mobile and 2 desktop sessions see /checkout; 3 mobile ones rage-click Pay twice.
        // 2 desktop ones buy, one of them before ever reaching checkout (doesn't convert).
        for i in 0..4 {
            let mut ev = vec![("pageview", "/checkout", "", "")];
            if i < 3 {
                ev.extend([("rage_click", "/checkout", "button#pay", ""), ("rage_click", "/checkout", "button#pay", "")]);
            }
            session(&pool, s, &format!("m{i}"), "mobile", t + i * 60_000, &ev).await;
        }
        session(&pool, s, "d0", "desktop", t, &[("pageview", "/checkout", "", ""), ("custom", "/checkout", "", "purchase")]).await;
        session(&pool, s, "d1", "desktop", t, &[("custom", "/", "", "purchase"), ("pageview", "/checkout", "", "")]).await;
        // Two pages seen once, with characters LIKE treats specially.
        session(&pool, s, "x0", "tablet", t, &[("pageview", "/docs/a_b", "", "")]).await;
        session(&pool, s, "x1", "tablet", t, &[("pageview", "/docs/axb", "", "")]).await;

        let rage = json!({ "measure": "session_rate", "match": { "rage_click": "button#pay", "path": "/checkout" }, "days": 7 });
        let a = ask(&pool, s, &q(rage.clone())).await.unwrap();
        assert_eq!((a.value, a.sessions), (Some(3.0 / 8.0), 8), "3 of the 8 sessions that started");
        assert_eq!(a.formatted.as_deref(), Some("37.5%"));

        let mut by_device = rage.clone();
        by_device["breakdown"] = json!("device");
        let rows = ask(&pool, s, &q(by_device)).await.unwrap().rows;
        assert_eq!(rows[0].key, "mobile");
        assert_eq!((rows[0].value, rows[0].sessions), (Some(0.75), 4));
        assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["mobile", "other"], "desktop (2) and tablet (2) are under 3 sessions");

        let per = ask(&pool, s, &q(json!({ "measure": "per_session", "match": { "rage_click": "*" }, "days": 7 }))).await.unwrap();
        assert_eq!(per.value, Some(6.0 / 8.0));
        let events = ask(&pool, s, &q(json!({ "measure": "events", "match": { "rage_click": "*" }, "days": 7 }))).await.unwrap();
        assert_eq!((events.value, events.sessions), (Some(6.0), 3));

        let conv = json!({ "measure": "conversion", "from": { "pageview": "/checkout" }, "to": { "event": "purchase" }, "days": 7 });
        let c = ask(&pool, s, &q(conv.clone())).await.unwrap();
        assert_eq!((c.value, c.sessions), (Some(1.0 / 6.0), 6), "only d0 bought after checkout");

        // Prefixes match by prefix; LIKE's special characters match only themselves.
        let pages = |p: &str| json!({ "measure": "sessions", "match": { "pageview": p }, "days": 7 });
        assert_eq!(ask(&pool, s, &q(pages("/docs/*"))).await.unwrap().value, Some(2.0));
        assert_eq!(ask(&pool, s, &q(pages("/docs/a_b"))).await.unwrap().value, Some(1.0));
        assert_eq!(ask(&pool, s, &q(pages("/docs/a_*"))).await.unwrap().value, Some(1.0), "_ is not a wildcard");

        // A page seen by one session doesn't appear by name.
        let mut by_path = pages("*");
        by_path["breakdown"] = json!("path");
        let rows = ask(&pool, s, &q(by_path)).await.unwrap().rows;
        assert_eq!(rows.iter().map(|r| (r.key.as_str(), r.sessions)).collect::<Vec<_>>(), [("/checkout", 6), ("other", 2)]);

        // The same numbers as the KPI code, sample for sample.
        let now = now_ms();
        for v in [rage, conv] {
            let m = Metric::parse(&v).unwrap();
            let kpi = crate::kpi::measure(&pool, s, &m.as_kpi().unwrap(), now - 7 * DAY_MS, now).await.unwrap();
            let mut ours = samples(&pool, s, &m, now - 7 * DAY_MS, now).await.unwrap();
            let mut theirs = kpi.samples.clone();
            ours.sort_by(f64::total_cmp);
            theirs.sort_by(f64::total_cmp);
            assert_eq!(ours, theirs, "{}", m.describe());
            assert_eq!(measure(&pool, s, &m, now - 7 * DAY_MS, now).await.unwrap().value, kpi.value);
        }

        // Comparing with the window before, and asking by day.
        let mut cmp = json!({ "measure": "sessions", "match": { "pageview": "*" }, "days": 1, "compare": true });
        let a = ask(&pool, s, &q(cmp.clone())).await.unwrap();
        // The data sits two days back: none in the last day, some in the day before it.
        assert_eq!(a.value, Some(0.0));
        assert!(a.previous.and_then(|p| p.value).is_some_and(|v| v > 0.0));
        cmp["days"] = json!(7);
        cmp["breakdown"] = json!("day");
        let rows = ask(&pool, s, &q(cmp)).await.unwrap().rows;
        assert_eq!(rows[0].key, date(t));
        crate::db::drop_test_target(&target).await;
    }

    #[test]
    fn refuses_what_it_doesnt_offer() {
        let err = |v: Value| Query::parse(&v).unwrap_err().to_string();
        assert!(err(json!({ "measure": "sessions", "match": { "pageview": "*" }, "breakdown": "visitor" })).contains("no per-session"));
        assert!(err(json!({ "measure": "sessions", "match": { "pageview": "*" }, "days": 365 })).contains("1 to 90"));
        assert!(err(json!({ "measure": "median", "match": { "pageview": "*" } })).contains("session_rate"));
        assert!(err(json!({ "measure": "conversion", "from": { "pageview": "/" }, "to": { "event": "x" }, "breakdown": "path" })).contains("device or day"));
        assert!(err(json!({ "measure": "sessions", "match": { "teleport": "*" } })).contains("unknown event type"));
    }
}
