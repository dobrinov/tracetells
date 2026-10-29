//! Funnels: steps people take in order within one visit, and where they stop.
//!
//! A funnel is either declared in `trace.json` under `"funnels"`, or
//! recognised from the KPIs: conversion KPIs whose goal is the next one's
//! start chain into a funnel (`/` → `/pricing` and `/pricing` → `signup` make
//! `/` → `/pricing` → `signup`). A step counts a session that reached it after
//! reaching the step before, in the same session, like a conversion KPI does
//! for two steps. Measurements are aggregates: sessions per step, never which.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Pool;
use crate::model::{EventMatch, KpiDefinition};
use crate::util::{DAY_MS, now_ms};

/// The most steps a funnel can have, declared or recognised.
pub const MAX_STEPS: usize = 8;
/// Recognised funnels shown at most, longest first.
const MAX_RECOGNISED: usize = 6;

/// A funnel as declared in `trace.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunnelSpec {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<EventMatch>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Step {
    /// What the step is, for people: a page's path or an event's name.
    pub label: String,
    #[serde(rename = "match")]
    pub matcher: EventMatch,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Funnel {
    pub key: String,
    pub name: String,
    pub description: String,
    /// "trace.json" for a declared funnel, "kpis" for one recognised from them.
    pub source: &'static str,
    /// For a recognised funnel, the KPIs it was assembled from, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kpis: Vec<String>,
    pub steps: Vec<Step>,
}

/// How a step reads to people.
pub fn label(m: &EventMatch) -> String {
    let any = |p: &Option<String>| p.clone().filter(|p| !p.is_empty() && p != "*");
    match m.kind.as_str() {
        "pageview" => any(&m.path).unwrap_or_else(|| "any page".into()),
        "custom" => any(&m.name).unwrap_or_else(|| "any event".into()),
        kind => {
            let what = any(&m.selector).or_else(|| any(&m.path)).unwrap_or_else(|| "anywhere".into());
            format!("{} {what}", kind.replace('_', " "))
        }
    }
}

fn step(m: &EventMatch) -> Step {
    Step { label: label(m), matcher: m.clone() }
}

/// Whether `inner`'s steps appear in `outer` in order (not necessarily next to each other).
fn is_subsequence(inner: &[EventMatch], outer: &[EventMatch]) -> bool {
    let mut it = outer.iter();
    inner.iter().all(|m| it.any(|o| o == m))
}

/// Funnels assembled from conversion KPIs that chain, longest first. Each is a
/// path of at least three steps that isn't contained in a longer one.
pub fn recognise(kpis: &[crate::model::Kpi]) -> Vec<Funnel> {
    let edges: Vec<(&EventMatch, &EventMatch, String)> = kpis
        .iter()
        .filter_map(|k| match &k.definition {
            KpiDefinition::Conversion { from, to } if from != to => {
                Some((from, to, k.key.clone().unwrap_or_else(|| k.id.to_string())))
            }
            _ => None,
        })
        .collect();
    let mut paths: Vec<(Vec<EventMatch>, Vec<String>)> = Vec::new();
    // Depth-first from every edge, never revisiting a step, so cycles end.
    fn walk(
        edges: &[(&EventMatch, &EventMatch, String)],
        steps: &mut Vec<EventMatch>,
        used: &mut Vec<String>,
        out: &mut Vec<(Vec<EventMatch>, Vec<String>)>,
    ) {
        let last = steps.last().expect("a path has a step").clone();
        let mut extended = false;
        if steps.len() < MAX_STEPS {
            for (from, to, key) in edges {
                if **from == last && !steps.contains(to) {
                    extended = true;
                    steps.push((*to).clone());
                    used.push(key.clone());
                    walk(edges, steps, used, out);
                    steps.pop();
                    used.pop();
                }
            }
        }
        if !extended && steps.len() >= 3 {
            out.push((steps.clone(), used.clone()));
        }
    }
    // Start where nothing leads in, so a funnel isn't found again from its middle;
    // a pure cycle has no such place, so then every edge is a start.
    let starts: Vec<&(&EventMatch, &EventMatch, String)> = {
        let sources: Vec<_> = edges.iter().filter(|(from, ..)| !edges.iter().any(|(_, to, _)| to == from)).collect();
        if sources.is_empty() { edges.iter().collect() } else { sources }
    };
    for (from, to, key) in starts {
        let mut steps = vec![(*from).clone(), (*to).clone()];
        let mut used = vec![key.clone()];
        walk(&edges, &mut steps, &mut used, &mut paths);
    }
    paths.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.1.cmp(&b.1)));
    let mut kept: Vec<(Vec<EventMatch>, Vec<String>)> = Vec::new();
    for (steps, used) in paths {
        if !kept.iter().any(|(k, _)| is_subsequence(&steps, k)) {
            kept.push((steps, used));
        }
    }
    kept.into_iter()
        .take(MAX_RECOGNISED)
        .map(|(steps, used)| {
            let labels: Vec<String> = steps.iter().map(label).collect();
            Funnel {
                key: format!("kpis:{}", used.join("+")),
                name: labels.join(" → "),
                description: format!("Recognised from the KPIs {}", used.join(", ")),
                source: "kpis",
                kpis: used,
                steps: steps.iter().map(step).collect(),
            }
        })
        .collect()
}

/// The funnels declared in the website's last applied `trace.json`.
pub async fn declared(pool: &Pool, project_id: i64) -> Result<Vec<FunnelSpec>> {
    let config: Option<String> = sqlx::query_scalar("SELECT config FROM projects WHERE id = $1").bind(project_id).fetch_optional(pool).await?.flatten();
    #[derive(Deserialize)]
    struct Stored {
        #[serde(default)]
        funnels: Vec<FunnelSpec>,
    }
    Ok(config.and_then(|c| serde_json::from_str::<Stored>(&c).ok()).map(|s| s.funnels).unwrap_or_default())
}

/// Every funnel of a website: the declared ones first, then the ones
/// recognised from its KPIs that a declared one doesn't already cover.
pub async fn list(pool: &Pool, project_id: i64) -> Result<Vec<Funnel>> {
    let declared = declared(pool, project_id).await?;
    let mut out: Vec<Funnel> = declared
        .iter()
        .map(|d| Funnel {
            key: d.key.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            source: "trace.json",
            kpis: Vec::new(),
            steps: d.steps.iter().map(step).collect(),
        })
        .collect();
    for r in recognise(&crate::kpi::list(pool, project_id).await?) {
        let steps: Vec<EventMatch> = r.steps.iter().map(|s| s.matcher.clone()).collect();
        if !declared.iter().any(|d| is_subsequence(&steps, &d.steps)) {
            out.push(r);
        }
    }
    Ok(out)
}

/// Sessions reaching each step in `[from, to)`, overall and per device.
pub struct Counts {
    pub total: Vec<i64>,
    pub by_device: BTreeMap<String, Vec<i64>>,
}

/// Walks every session that reached the first step through the rest, in
/// order: a step counts when it happens at or after the step before it.
pub async fn count(pool: &Pool, project_id: i64, steps: &[EventMatch], from: i64, to: i64) -> Result<Counts> {
    let mut per_step: Vec<HashMap<String, Vec<i64>>> = Vec::new();
    for m in steps {
        let mut by_session: HashMap<String, Vec<i64>> = HashMap::new();
        for h in crate::kpi::hits(pool, project_id, m, from, to).await? {
            by_session.entry(h.session_id).or_default().push(h.ts);
        }
        for times in by_session.values_mut() {
            times.sort_unstable();
        }
        per_step.push(by_session);
    }
    let devices: HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT id, COALESCE(device, 'unknown') FROM sessions WHERE project_id = $1 AND last_seen_at >= $2 AND started_at < $3",
    )
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    let mut total = vec![0i64; steps.len()];
    let mut by_device: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let Some(first) = per_step.first() else { return Ok(Counts { total, by_device }) };
    for (session, times) in first {
        let device = devices.get(session).cloned().unwrap_or_else(|| "unknown".into());
        let row = by_device.entry(device).or_insert_with(|| vec![0; steps.len()]);
        let mut at = times[0];
        total[0] += 1;
        row[0] += 1;
        for (i, step) in per_step.iter().enumerate().skip(1) {
            let Some(next) = step.get(session).and_then(|t| t.iter().find(|t| **t >= at)) else { break };
            at = *next;
            total[i] += 1;
            row[i] += 1;
        }
    }
    Ok(Counts { total, by_device })
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub label: String,
    pub sessions: i64,
    /// Share of the step before that reached this one (None for the first step).
    pub of_previous: Option<f64>,
    /// Share of the first step that reached this one.
    pub of_first: Option<f64>,
    /// Sessions that reached the step before but not this one.
    pub lost: i64,
    /// Sessions in the window before, for comparison.
    pub previous_sessions: i64,
    pub previous_of_previous: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Drop {
    /// The step people don't reach (1-based), and the one before it.
    pub to_step: usize,
    pub from_label: String,
    pub to_label: String,
    pub lost: i64,
    pub rate: f64,
}

#[derive(Debug, Serialize)]
pub struct DeviceRow {
    pub device: String,
    pub sessions: Vec<i64>,
    /// First step to last, for this device.
    pub conversion: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct FunnelReport {
    #[serde(flatten)]
    pub funnel: Funnel,
    pub days: u32,
    pub steps: Vec<StepReport>,
    /// First step to last.
    pub conversion: Option<f64>,
    pub previous_conversion: Option<f64>,
    /// Where the most sessions are lost: the place to look first. Only named
    /// once someone finished the funnel; before that, every step is a drop.
    pub biggest_drop: Option<Drop>,
    pub by_device: Vec<DeviceRow>,
}

fn ratio(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// Every funnel of a website, measured over the last `days` and the `days` before.
pub async fn report(pool: &Pool, project_id: i64, days: u32) -> Result<Vec<FunnelReport>> {
    let to = now_ms();
    let window = i64::from(days) * DAY_MS;
    let mut out = Vec::new();
    for funnel in list(pool, project_id).await? {
        let matchers: Vec<EventMatch> = funnel.steps.iter().map(|s| s.matcher.clone()).collect();
        let now = count(pool, project_id, &matchers, to - window, to).await?;
        let before = count(pool, project_id, &matchers, to - 2 * window, to - window).await?;
        let steps: Vec<StepReport> = funnel
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sessions = now.total[i];
                let prev = (i > 0).then(|| now.total[i - 1]);
                StepReport {
                    label: s.label.clone(),
                    sessions,
                    of_previous: prev.and_then(|p| ratio(sessions, p)),
                    of_first: ratio(sessions, now.total[0]),
                    lost: prev.map(|p| p - sessions).unwrap_or(0),
                    previous_sessions: before.total[i],
                    previous_of_previous: (i > 0).then(|| ratio(before.total[i], before.total[i - 1])).flatten(),
                }
            })
            .collect();
        let last = steps.len().saturating_sub(1);
        let biggest_drop = steps
            .iter()
            .filter(|_| now.total[last] > 0)
            .enumerate()
            .skip(1)
            .filter(|(_, s)| s.lost > 0)
            .max_by_key(|(_, s)| s.lost)
            .map(|(i, s)| Drop {
                to_step: i + 1,
                from_label: steps[i - 1].label.clone(),
                to_label: s.label.clone(),
                lost: s.lost,
                rate: s.of_previous.map(|r| 1.0 - r).unwrap_or(0.0),
            });
        let by_device = now
            .by_device
            .into_iter()
            .map(|(device, sessions)| DeviceRow { conversion: ratio(sessions[last], sessions[0]), device, sessions })
            .collect();
        out.push(FunnelReport {
            conversion: ratio(now.total[last], now.total[0]),
            previous_conversion: ratio(before.total[last], before.total[0]),
            funnel,
            days,
            steps,
            biggest_drop,
            by_device,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Direction, Kpi};

    fn page(p: &str) -> EventMatch {
        EventMatch { kind: "pageview".into(), path: Some(p.into()), ..Default::default() }
    }
    fn event(n: &str) -> EventMatch {
        EventMatch { kind: "custom".into(), name: Some(n.into()), ..Default::default() }
    }
    fn conversion(key: &str, from: EventMatch, to: EventMatch) -> Kpi {
        Kpi {
            id: 0,
            key: Some(key.into()),
            name: key.into(),
            description: String::new(),
            definition: KpiDefinition::Conversion { from, to },
            direction: Direction::Increase,
            target: None,
            created_at: 0,
        }
    }

    #[test]
    fn chained_conversion_kpis_become_funnels() {
        let kpis = [
            conversion("home_to_writer", page("/jsonme"), page("/jsonme/write")),
            conversion("writer_copies_prompt", page("/jsonme/write"), event("prompt_copied")),
            conversion("writer_gets_ready", page("/jsonme/write"), event("cv_ready")),
            conversion("writer_saves", page("/jsonme/write"), event("cv_saved")),
            conversion("ready_to_saved", event("cv_ready"), event("cv_saved")),
        ];
        let found = recognise(&kpis);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        // The longest path, then the prompt branch; /jsonme/write → cv_saved is inside the first.
        assert_eq!(names[0], "/jsonme → /jsonme/write → cv_ready → cv_saved", "{names:?}");
        assert!(names.contains(&"/jsonme → /jsonme/write → prompt_copied"), "{names:?}");
        assert!(!names.contains(&"/jsonme → /jsonme/write → cv_saved"), "contained paths are dropped: {names:?}");
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(found[0].kpis, ["home_to_writer", "writer_gets_ready", "ready_to_saved"]);
        assert!(found.iter().all(|f| f.steps.len() >= 3 && f.steps.len() <= MAX_STEPS));
        assert!(recognise(&kpis[..1]).is_empty(), "one KPI is not a funnel");
    }

    #[test]
    fn labels_read_like_the_site() {
        assert_eq!(label(&page("/pricing")), "/pricing");
        assert_eq!(label(&event("signup")), "signup");
        assert_eq!(label(&EventMatch { kind: "click".into(), selector: Some("button#pay".into()), ..Default::default() }), "click button#pay");
    }

    async fn visit(pool: &Pool, site: i64, sid: &str, device: &str, steps: &[(&str, &str)]) {
        let start = now_ms() - 3_600_000;
        sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at, device) VALUES ($1, $2, $3, $3, $4)")
            .bind(sid)
            .bind(site)
            .bind(start)
            .bind(device)
            .execute(pool)
            .await
            .unwrap();
        for (i, (kind, what)) in steps.iter().enumerate() {
            let (path, name) = if *kind == "pageview" { (Some(*what), None) } else { (Some("/x"), Some(*what)) };
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, name) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(site)
                .bind(sid)
                .bind(start + i as i64 * 1000)
                .bind(kind)
                .bind(path)
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn steps_count_in_order_and_the_biggest_drop_is_named() {
        let path = crate::db::test_target("funnel").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let config = crate::config::parse(
            r#"{ "events": { "signup": "Signed up" },
                 "kpis": { "k": { "session_rate": { "event": "signup" }, "goal": "increase" } },
                 "funnels": { "signup": { "name": "Sign-up", "steps": [ { "pageview": "/" }, { "pageview": "/pricing" }, { "event": "signup" } ] } } }"#,
        )
        .unwrap();
        crate::config::apply(&pool, site.id, &config, "test").await.unwrap();

        visit(&pool, site.id, "a", "desktop", &[("pageview", "/"), ("pageview", "/pricing"), ("custom", "signup")]).await;
        visit(&pool, site.id, "b", "desktop", &[("pageview", "/"), ("pageview", "/pricing")]).await;
        visit(&pool, site.id, "c", "mobile", &[("pageview", "/"), ("pageview", "/about")]).await;
        visit(&pool, site.id, "d", "mobile", &[("pageview", "/"), ("pageview", "/about")]).await;
        // Signed up before seeing pricing: the order matters, so it stops at step 1.
        visit(&pool, site.id, "e", "mobile", &[("custom", "signup"), ("pageview", "/"), ("pageview", "/blog")]).await;

        let reports = report(&pool, site.id, 14).await.unwrap();
        let r = &reports[0];
        assert_eq!((r.funnel.key.as_str(), r.funnel.source), ("signup", "trace.json"));
        let counts: Vec<i64> = r.steps.iter().map(|s| s.sessions).collect();
        assert_eq!(counts, [5, 2, 1]);
        assert_eq!(r.conversion, Some(0.2));
        let drop = r.biggest_drop.as_ref().unwrap();
        assert_eq!((drop.to_step, drop.from_label.as_str(), drop.to_label.as_str(), drop.lost), (2, "/", "/pricing", 3));
        let mobile = r.by_device.iter().find(|d| d.device == "mobile").unwrap();
        assert_eq!(mobile.sessions, [3, 0, 0]);

        // Until someone finishes, there's no drop to name: every step loses everyone.
        sqlx::query("DELETE FROM events WHERE project_id = $1 AND name = 'signup'").bind(site.id).execute(&pool).await.unwrap();
        let r = &report(&pool, site.id, 14).await.unwrap()[0];
        assert_eq!((r.steps[2].sessions, r.conversion), (0, Some(0.0)));
        assert!(r.biggest_drop.is_none());
        crate::db::drop_test_target(&path).await;
    }
}
