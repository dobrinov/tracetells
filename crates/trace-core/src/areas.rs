//! A website's areas and features: which parts of the product people use,
//! which they don't, and where using them hurts.
//!
//! A larger product has sub-products (areas) and those have features. Each is
//! declared in trace.json by the pages and events that mean someone used it:
//!
//! ```json
//! "areas": {
//!   "battles": {
//!     "name": "Battles",
//!     "features": {
//!       "duels": { "name": "Duels", "match": [{ "pageview": "/challenges*" }] },
//!       "leagues": { "name": "Leagues", "match": [{ "pageview": "/leagues*" }, { "event": "league_joined" }] }
//!     }
//!   }
//! }
//! ```
//!
//! An area is used when any of its own matchers or any of its features' match.
//! Without declared areas, Trace suggests them from the website's URLs. For
//! each it measures reach (the share of visitors who used it, counted per day
//! like every visitor count), adoption among signed-in people, how many come
//! back to it on another day, and the share of sessions using it that hit a
//! rage click, a dead click or an error on its own pages (the pages it
//! declares, or where its events happen when it declares none). Counts only.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Pool;
use crate::model::{EventMatch, pattern_matches};
use crate::util::{DAY_MS, now_ms};

pub const MAX_AREAS: usize = 16;
pub const MAX_FEATURES: usize = 24;
/// Fewer sessions than this and an area is "too little data" rather than judged.
pub const MIN_SESSIONS: i64 = 10;
/// From this share of sessions with a problem, using an area is painful.
pub const PAINFUL: f64 = 0.15;
/// Below this reach an area is rarely used; a feature, below this share of its area's sessions.
pub const RARE_AREA: f64 = 0.05;
pub const RARE_FEATURE: f64 = 0.10;
/// Event types that mean using a page went wrong.
const PROBLEMS: &str = "'rage_click', 'dead_click', 'error'";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AreaSpec {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "match")]
    pub matchers: Vec<EventMatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<AreaSpec>,
}

pub async fn declared(pool: &Pool, project_id: i64) -> Result<Vec<AreaSpec>> {
    let config: Option<String> = sqlx::query_scalar("SELECT config FROM projects WHERE id = $1").bind(project_id).fetch_optional(pool).await?.flatten();
    #[derive(Deserialize)]
    struct Stored {
        #[serde(default)]
        areas: Vec<AreaSpec>,
    }
    Ok(config.and_then(|c| serde_json::from_str::<Stored>(&c).ok()).map(|s| s.areas).unwrap_or_default())
}

#[derive(Debug, Serialize)]
pub struct Problem {
    /// rage_click, dead_click or error.
    pub kind: String,
    pub path: String,
    /// The element (selector and text) or the error message.
    pub what: String,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct NodeReport {
    pub key: String,
    pub name: String,
    pub description: String,
    /// The area's key for a feature; None for an area.
    pub area: Option<String>,
    /// What counts as using it, as labels: "/checkout", "add_to_cart" (an area's include its features').
    pub matches: Vec<String>,
    pub sessions: i64,
    pub previous_sessions: i64,
    /// Visitors (per day) who used it, and their share of all visitors.
    pub visitors: i64,
    pub reach: Option<f64>,
    pub previous_reach: Option<f64>,
    /// Signed-in people who used it, and their share of the signed-in people active in the window.
    pub people: i64,
    pub adoption: Option<f64>,
    /// Of those people, the share who used it on two or more days.
    pub repeat: Option<f64>,
    /// Sessions using it that hit a problem on its pages, and their share.
    pub problem_sessions: i64,
    pub problem_rate: Option<f64>,
    pub top_problem: Option<Problem>,
    /// healthy, painful (used, but too many problems), rare, rare_painful or too_little.
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Areas {
    /// "trace.json", or "suggested" from the website's URLs when none are declared.
    pub source: &'static str,
    pub days: u32,
    pub visitors: i64,
    /// Signed-in people active in the window: adoption is a share of them.
    pub people: i64,
    /// Areas, each followed by its features.
    pub nodes: Vec<NodeReport>,
}

fn ratio(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// Sessions with an event matching `m`, and the paths it matched on.
async fn matched(scan: &mut crate::kpi::Scan, pool: &Pool, project_id: i64, m: &EventMatch, from: i64, to: i64) -> Result<Vec<(String, String)>> {
    Ok(scan
        .rows(pool, project_id, &m.kind, from, to)
        .await?
        .iter()
        .filter(|(_, _, path, sel, name)| m.matches(path.as_deref(), sel.as_deref(), name.as_deref()))
        .map(|(session, _, path, ..)| (session.clone(), path.clone().unwrap_or_default()))
        .collect())
}

/// Where a node is used: its sessions, and the pages problems count on (its
/// pageview patterns, or the pages its other events happened on).
struct Usage {
    sessions: HashSet<String>,
    patterns: Vec<String>,
    paths: HashSet<String>,
}

impl Usage {
    /// Its own pages when it declares any; otherwise the pages its events happened on.
    /// (Adding to the cart happens on a product page, but a product page's errors aren't the cart's.)
    fn on_page(&self, path: &str) -> bool {
        if self.patterns.is_empty() {
            self.paths.contains(path)
        } else {
            self.patterns.iter().any(|p| pattern_matches(&Some(p.clone()), Some(path)))
        }
    }
}

async fn usage(scan: &mut crate::kpi::Scan, pool: &Pool, project_id: i64, matchers: &[&EventMatch], from: i64, to: i64) -> Result<Usage> {
    let mut u = Usage { sessions: HashSet::new(), patterns: Vec::new(), paths: HashSet::new() };
    for m in matchers {
        if m.kind == "pageview"
            && let Some(p) = &m.path
        {
            u.patterns.push(p.clone());
        }
        for (session, path) in matched(scan, pool, project_id, m, from, to).await? {
            if m.kind != "pageview" && !path.is_empty() {
                u.paths.insert(path);
            }
            u.sessions.insert(session);
        }
    }
    Ok(u)
}

/// A node's own matchers, plus its features' for an area.
fn all_matchers(spec: &AreaSpec) -> Vec<&EventMatch> {
    spec.matchers.iter().chain(spec.features.iter().flat_map(|f| f.matchers.iter())).collect()
}

/// Areas from the website's URLs: the first path segment (and the second, as
/// features) of pages at least a few sessions viewed.
async fn suggest(pool: &Pool, project_id: i64, from: i64, to: i64) -> Result<Vec<AreaSpec>> {
    let rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT path, COUNT(DISTINCT session_id) FROM events WHERE project_id = $1 AND type = 'pageview' AND ts >= $2 AND ts < $3 GROUP BY path",
    )
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut first: BTreeMap<String, i64> = BTreeMap::new();
    let mut second: BTreeMap<(String, String), i64> = BTreeMap::new();
    for (path, n) in rows {
        let path = path.unwrap_or_default();
        let mut parts = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty());
        let top = parts.next().unwrap_or("").to_string();
        *first.entry(top.clone()).or_default() += n;
        if let Some(sub) = parts.next() {
            *second.entry((top, sub.to_string())).or_default() += n;
        }
    }
    let mut tops: Vec<(String, i64)> = first.into_iter().filter(|(_, n)| *n >= 5).collect();
    tops.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tops.truncate(MAX_AREAS);
    Ok(tops
        .into_iter()
        .map(|(top, _)| {
            if top.is_empty() {
                return AreaSpec {
                    key: "home".into(),
                    name: "/".into(),
                    description: String::new(),
                    matchers: vec![EventMatch { kind: "pageview".into(), path: Some("/".into()), ..Default::default() }],
                    features: Vec::new(),
                };
            }
            let mut subs: Vec<(String, i64)> = second.iter().filter(|((t, _), n)| *t == top && **n >= 3).map(|((_, s), n)| (s.clone(), *n)).collect();
            subs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            subs.truncate(6);
            AreaSpec {
                key: top.clone(),
                name: format!("/{top}"),
                description: String::new(),
                matchers: vec![EventMatch { kind: "pageview".into(), path: Some(format!("/{top}*")), ..Default::default() }],
                features: if subs.len() < 2 {
                    Vec::new()
                } else {
                    subs.into_iter()
                        .map(|(sub, _)| AreaSpec {
                            key: format!("{top}/{sub}"),
                            name: format!("/{top}/{sub}"),
                            description: String::new(),
                            matchers: vec![EventMatch { kind: "pageview".into(), path: Some(format!("/{top}/{sub}*")), ..Default::default() }],
                            features: Vec::new(),
                        })
                        .collect()
                },
            }
        })
        .collect())
}

/// One problem event: its session, type, page, and what it was about.
struct ProblemEvent {
    session: String,
    kind: String,
    path: String,
    what: String,
}

async fn problems(pool: &Pool, project_id: i64, from: i64, to: i64) -> Result<Vec<ProblemEvent>> {
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(&format!(
        "SELECT session_id, type, path, selector, text FROM events WHERE project_id = $1 AND type IN ({PROBLEMS}) AND ts >= $2 AND ts < $3"
    ))
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(session, kind, path, selector, text)| {
            let what = match (kind.as_str(), selector, text) {
                ("error", _, text) => crate::util::ellipsize(&text.unwrap_or_else(|| "Unknown error".into()), 140),
                (_, Some(sel), Some(text)) if !text.is_empty() => format!("{sel} \"{}\"", crate::util::ellipsize(&text, 40)),
                (_, sel, _) => sel.unwrap_or_default(),
            };
            ProblemEvent { session, kind, path: path.unwrap_or_default(), what }
        })
        .collect())
}

struct Measured {
    sessions: i64,
    visitors: i64,
    people: HashMap<String, HashSet<String>>,
    problem_sessions: i64,
    top: Option<Problem>,
}

fn measure(u: &Usage, list: &[crate::audience::Session], probs: &[ProblemEvent]) -> Measured {
    let mine: Vec<&crate::audience::Session> = list.iter().filter(|s| u.sessions.contains(&s.id)).collect();
    let visitors = mine.iter().map(|s| (s.day.as_str(), s.who.as_str())).collect::<HashSet<_>>().len() as i64;
    let mut people: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &mine {
        if let Some(user) = &s.user {
            people.entry(user.clone()).or_default().insert(s.day.clone());
        }
    }
    let ids: HashSet<&str> = mine.iter().map(|s| s.id.as_str()).collect();
    let mut hit: HashSet<&str> = HashSet::new();
    let mut by: HashMap<(&str, &str, &str), HashSet<&str>> = HashMap::new();
    for p in probs.iter().filter(|p| ids.contains(p.session.as_str()) && u.on_page(&p.path)) {
        hit.insert(p.session.as_str());
        by.entry((p.kind.as_str(), p.path.as_str(), p.what.as_str())).or_default().insert(p.session.as_str());
    }
    let top = by
        .into_iter()
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then(b.0.cmp(&a.0)))
        .map(|((kind, path, what), sessions)| Problem { kind: kind.into(), path: path.into(), what: what.into(), sessions: sessions.len() as i64 });
    Measured { sessions: mine.len() as i64, visitors, people, problem_sessions: hit.len() as i64, top }
}

fn status(sessions: i64, rare: bool, problem_rate: Option<f64>) -> &'static str {
    if sessions < MIN_SESSIONS {
        return "too_little";
    }
    let painful = problem_rate.is_some_and(|r| r >= PAINFUL);
    match (rare, painful) {
        (false, false) => "healthy",
        (false, true) => "painful",
        (true, false) => "rare",
        (true, true) => "rare_painful",
    }
}

/// Every area and feature over the last `days`, compared with the `days` before.
pub async fn report(pool: &Pool, project_id: i64, days: u32) -> Result<Areas> {
    let now = now_ms();
    let window = i64::from(days) * DAY_MS;
    let (from, before) = (now - window, now - 2 * window);
    let mut specs = declared(pool, project_id).await?;
    let source = if specs.is_empty() {
        specs = suggest(pool, project_id, from, now).await?;
        "suggested"
    } else {
        "trace.json"
    };
    let current = crate::audience::sessions(pool, project_id, from, now).await?;
    let previous = crate::audience::sessions(pool, project_id, before, from).await?;
    let all_visitors = current.iter().map(|s| (s.day.as_str(), s.who.as_str())).collect::<HashSet<_>>().len() as i64;
    let before_visitors = previous.iter().map(|s| (s.day.as_str(), s.who.as_str())).collect::<HashSet<_>>().len() as i64;
    let active_people = current.iter().filter_map(|s| s.user.as_deref()).collect::<HashSet<_>>().len() as i64;
    let probs = problems(pool, project_id, from, now).await?;
    // Areas and their features match against the same events: read each type once per window.
    let mut scan = crate::kpi::Scan::default();

    let mut nodes = Vec::new();
    for area in &specs {
        let matchers = all_matchers(area);
        let now_use = usage(&mut scan, pool, project_id, &matchers, from, now).await?;
        let before_use = usage(&mut scan, pool, project_id, &matchers, before, from).await?;
        let m = measure(&now_use, &current, &probs);
        let prev = measure(&before_use, &previous, &[]);
        let reach = ratio(m.visitors, all_visitors);
        let area_sessions = m.sessions;
        nodes.push(node(area, None, &m, prev.sessions, reach, ratio(prev.visitors, before_visitors), active_people, reach.is_some_and(|r| r < RARE_AREA)));
        for feature in &area.features {
            let fm: Vec<&EventMatch> = feature.matchers.iter().collect();
            let now_use = usage(&mut scan, pool, project_id, &fm, from, now).await?;
            let before_use = usage(&mut scan, pool, project_id, &fm, before, from).await?;
            let f = measure(&now_use, &current, &probs);
            let fprev = measure(&before_use, &previous, &[]);
            let rare = ratio(f.sessions, area_sessions).is_some_and(|r| r < RARE_FEATURE);
            let reach = ratio(f.visitors, all_visitors);
            nodes.push(node(feature, Some(&area.key), &f, fprev.sessions, reach, ratio(fprev.visitors, before_visitors), active_people, rare));
        }
    }
    Ok(Areas { source, days, visitors: all_visitors, people: active_people, nodes })
}

#[allow(clippy::too_many_arguments)]
fn node(spec: &AreaSpec, area: Option<&str>, m: &Measured, previous_sessions: i64, reach: Option<f64>, previous_reach: Option<f64>, active_people: i64, rare: bool) -> NodeReport {
    let people = m.people.len() as i64;
    let problem_rate = ratio(m.problem_sessions, m.sessions);
    NodeReport {
        key: spec.key.clone(),
        name: spec.name.clone(),
        description: spec.description.clone(),
        area: area.map(str::to_string),
        matches: all_matchers(spec).into_iter().map(crate::funnel::label).collect::<std::collections::BTreeSet<_>>().into_iter().collect(),
        sessions: m.sessions,
        previous_sessions,
        visitors: m.visitors,
        reach,
        previous_reach,
        people,
        adoption: ratio(people, active_people),
        repeat: ratio(m.people.values().filter(|days| days.len() >= 2).count() as i64, people),
        problem_sessions: m.problem_sessions,
        problem_rate,
        top_problem: m.top.as_ref().map(|p| Problem { kind: p.kind.clone(), path: p.path.clone(), what: p.what.clone(), sessions: p.sessions }),
        status: status(m.sessions, rare, problem_rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn visit(pool: &Pool, site: i64, sid: &str, user: Option<&str>, at: i64, events: &[(&str, &str, &str)]) {
        sqlx::query("INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at, device) VALUES ($1, $2, $3, $4, $5, $5, 'desktop')")
            .bind(sid)
            .bind(site)
            .bind(format!("v_{sid}"))
            .bind(user)
            .bind(at)
            .execute(pool)
            .await
            .unwrap();
        for (i, (kind, path, what)) in events.iter().enumerate() {
            let name = (*kind == "custom").then_some(*what);
            let selector = (kind.ends_with("click")).then_some(*what);
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, selector, text, name) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
                .bind(site)
                .bind(sid)
                .bind(at + i as i64 * 1000)
                .bind(kind)
                .bind(path)
                .bind(selector)
                .bind((*kind == "error").then_some(*what))
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn measures_reach_adoption_repeat_and_problems_per_area_and_feature() {
        let path = crate::db::test_target("areas").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "School", None).await.unwrap();
        let config = crate::config::parse(
            r#"{ "events": { "league_joined": "Joined a league" },
                 "kpis": { "k": { "session_rate": { "event": "league_joined" }, "goal": "increase" } },
                 "areas": {
                   "battles": { "name": "Battles", "features": {
                     "duels": { "name": "Duels", "match": [ { "pageview": "/challenges*" } ] },
                     "leagues": { "name": "Leagues", "match": [ { "event": "league_joined" } ] } } },
                   "practice": { "name": "Practice", "match": [ { "pageview": "/assignments*" } ] } } }"#,
        )
        .unwrap();
        crate::config::apply(&pool, site.id, &config, "test").await.unwrap();
        let now = now_ms();
        // Twelve duel sessions; four of them rage-click the ready button. One person duels on two days.
        for i in 0..12 {
            let rage: &[(&str, &str, &str)] = if i < 4 { &[("pageview", "/challenges/:id", ""), ("rage_click", "/challenges/:id", "button#ready")] } else { &[("pageview", "/challenges/:id", "")] };
            let (user, at) = match i {
                0 => (Some("u1"), now - 3 * DAY_MS),
                1 => (Some("u1"), now - DAY_MS),
                2 => (Some("u2"), now - DAY_MS),
                _ => (None, now - 3_600_000),
            };
            visit(&pool, site.id, &format!("d{i}"), user, at, rage).await;
        }
        // One league joined, from the leagues page; and a practice page with an error that isn't a battle's.
        visit(&pool, site.id, "l", None, now - 3_600_000, &[("pageview", "/leagues", ""), ("custom", "/leagues", "league_joined")]).await;
        visit(&pool, site.id, "p", Some("u3"), now - 3_600_000, &[("pageview", "/assignments/:id", ""), ("error", "/assignments/:id", "TypeError: x")]).await;

        let a = report(&pool, site.id, 7).await.unwrap();
        assert_eq!((a.source, a.people), ("trace.json", 3));
        let by = |k: &str| a.nodes.iter().find(|n| n.key == k).unwrap();
        let battles = by("battles");
        assert_eq!((battles.sessions, battles.people, battles.area.as_deref()), (13, 2, None));
        assert_eq!(battles.adoption, Some(2.0 / 3.0));
        assert_eq!(battles.repeat, Some(0.5), "u1 came back on another day, u2 didn't");
        assert_eq!(battles.problem_sessions, 4, "the practice error isn't on a battle page");
        assert_eq!(battles.status, "painful");
        let top = battles.top_problem.as_ref().unwrap();
        assert_eq!((top.kind.as_str(), top.what.as_str(), top.sessions), ("rage_click", "button#ready", 4));
        let leagues = by("leagues");
        assert_eq!((leagues.sessions, leagues.area.as_deref(), leagues.status), (1, Some("battles"), "too_little"));
        assert_eq!(by("duels").sessions, 12);
        let practice = by("practice");
        assert_eq!((practice.sessions, practice.problem_sessions), (1, 1));
        // Leagues declares no pages, so its problems would be on /leagues, where it was joined: there are none.
        assert_eq!(leagues.problem_sessions, 0);

        // Without declared areas, they're suggested from the URLs.
        let (_, other) = crate::db::create_account_with_site(&pool, "Other", None).await.unwrap();
        for i in 0..6 {
            visit(&pool, other.id, &format!("o{i}"), None, now - 3_600_000, &[("pageview", "/blog/a", ""), ("pageview", if i % 2 == 0 { "/blog/b" } else { "/blog/c" }, "")]).await;
        }
        let b = report(&pool, other.id, 7).await.unwrap();
        assert_eq!(b.source, "suggested");
        assert_eq!(b.nodes[0].name, "/blog");
        assert_eq!(b.nodes.iter().filter(|n| n.area.as_deref() == Some("blog")).count(), 3);
        crate::db::drop_test_target(&path).await;
    }
}
