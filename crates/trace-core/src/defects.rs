//! Defect candidates: things that are plainly broken on a website, found in
//! behaviour Trace already has. Unlike patterns, one occurrence is enough once
//! the agent has found the cause in the code, so they're worth looking at from
//! the first session on, when there is too little traffic for anything else.

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

use crate::Pool;
use crate::model::EventMatch;
use crate::privacy::{one_line, one_line_path};
use crate::util::{DAY_MS, ellipsize, now_ms};

/// At most this many candidates, the ones affecting the most sessions first.
pub const LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// Stable across runs; used as the insight's fingerprint, it marks the candidate covered.
    pub key: String,
    /// failing_request, exception, console_error, missing_file, unreached_step, missing_event or dead_control.
    pub kind: &'static str,
    /// What happens, for people: the error message, the step, the event or the element.
    pub label: String,
    pub pages: Vec<String>,
    /// Occurrences in the window (0 for something that should happen and doesn't).
    pub count: i64,
    /// Sessions it affected; for an unreached step, the sessions that reached the step before.
    pub sessions: i64,
    /// When it last happened; None for something that never happens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
    /// The measurement to use as the insight's evidence, as `query_behaviour` takes it.
    pub metric: Value,
    /// Which way the metric moves once it's fixed.
    pub goal: &'static str,
    /// The insight that already has this candidate's key as its fingerprint, in any status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covered_by: Option<Covered>,
    /// Not covered, and not looked at by a sweep since it last happened.
    pub uncovered: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Covered {
    pub insight: i64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct Defects {
    pub days: u32,
    pub swept_at: Option<i64>,
    pub uncovered: usize,
    pub candidates: Vec<Candidate>,
}

/// Whether a dead click's element is meant to be clicked, judged from its
/// selector alone (the collector never reads styles): the last part is a
/// control, has a role, or a class that names one.
pub fn looks_interactive(selector: &str) -> bool {
    let last = selector.rsplit('>').next().unwrap_or("").trim();
    let tag: String = last.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if matches!(tag.to_ascii_lowercase().as_str(), "button" | "a" | "input" | "select" | "textarea" | "summary" | "label") {
        return true;
    }
    if last.contains("[role") {
        return true;
    }
    last.split(['.', '#', '[', ':']).skip(1).map(str::to_ascii_lowercase).any(|class| {
        ["btn", "button", "link", "choice"].iter().any(|word| class.split(['-', '_']).any(|part| part == *word))
    })
}

/// A key or text pattern stays within what fingerprints and metrics accept;
/// a message cut short matches by prefix.
fn pattern(message: &str) -> String {
    if message.chars().count() <= 150 { message.to_string() } else { format!("{}*", message.chars().take(149).collect::<String>()) }
}

fn error_kind(name: &str) -> &'static str {
    match name {
        "network" => "failing_request",
        "resource" => "missing_file",
        "console" => "console_error",
        _ => "exception",
    }
}

fn matcher_json(m: &EventMatch) -> Value {
    serde_json::to_value(m).unwrap_or(Value::Null)
}

pub async fn candidates(pool: &Pool, project_id: i64, days: u32) -> Result<Defects> {
    let to = now_ms();
    let from = to - i64::from(days) * DAY_MS;
    let p = project_id;
    let mut out: Vec<Candidate> = Vec::new();

    // Errors, one candidate per kind and message wherever it happened.
    let errors: Vec<(String, String, String, i64, i64)> = sqlx::query_as(
        "SELECT COALESCE(name, 'exception'), COALESCE(text, 'Unknown error'), COALESCE(path, ''), COUNT(*), MAX(ts)
         FROM events WHERE project_id = $1 AND type = 'error' AND ts >= $2 AND ts < $3 GROUP BY 1, 2, 3",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?;
    let sessions_of: std::collections::HashMap<(String, String), i64> = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT COALESCE(name, 'exception'), COALESCE(text, 'Unknown error'), COUNT(DISTINCT session_id)
         FROM events WHERE project_id = $1 AND type = 'error' AND ts >= $2 AND ts < $3 GROUP BY 1, 2",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?
    .into_iter()
    .map(|(name, text, sessions)| ((name, text), sessions))
    .collect();
    let mut by_error: Vec<Candidate> = Vec::new();
    for (name, text, path, count, last) in errors {
        let message = pattern(&one_line(&text));
        let key = format!("error:{name}:{message}");
        let path = one_line_path(&path);
        if let Some(c) = by_error.iter_mut().find(|c| c.key == key) {
            c.count += count;
            c.last_seen = c.last_seen.max(Some(last));
            if !path.is_empty() && !c.pages.contains(&path) {
                c.pages.push(path);
            }
            continue;
        }
        let m = EventMatch { kind: "error".into(), name: Some(name.clone()), text: Some(message.clone()), ..Default::default() };
        by_error.push(Candidate {
            sessions: sessions_of.get(&(name.clone(), text.clone())).copied().unwrap_or(0),
            key,
            kind: error_kind(&name),
            label: ellipsize(&one_line(&text), 200),
            pages: if path.is_empty() { vec![] } else { vec![path] },
            count,
            last_seen: Some(last),
            metric: json!({ "measure": "events", "match": matcher_json(&m) }),
            goal: "decrease",
            covered_by: None,
            uncovered: false,
        });
    }
    out.extend(by_error);

    // Dead clicks on things that are meant to be clicked.
    let dead: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT COALESCE(path, ''), COALESCE(selector, ''), COUNT(*), COUNT(DISTINCT session_id), MAX(ts) FROM events
         WHERE project_id = $1 AND type = 'dead_click' AND ts >= $2 AND ts < $3 AND selector IS NOT NULL GROUP BY 1, 2",
    )
    .bind(p).bind(from).bind(to)
    .fetch_all(pool).await?;
    for (path, selector, count, sessions, last) in dead.into_iter().filter(|(_, s, ..)| looks_interactive(s)) {
        let (path, selector) = (one_line_path(&path), one_line(&selector));
        let m = EventMatch { kind: "dead_click".into(), path: Some(path.clone()).filter(|p| !p.is_empty()), selector: Some(selector.clone()), ..Default::default() };
        out.push(Candidate {
            key: format!("dead:{path}:{}", pattern(&selector)),
            kind: "dead_control",
            label: selector,
            pages: if path.is_empty() { vec![] } else { vec![path] },
            count,
            sessions,
            last_seen: Some(last),
            metric: json!({ "measure": "events", "match": matcher_json(&m) }),
            goal: "decrease",
            covered_by: None,
            uncovered: false,
        });
    }

    // Declared events that have never arrived, while visits have.
    let health = crate::config::tracking_health(pool, p, i64::from(days)).await?;
    let missing: Vec<String> = if health.collecting_since.is_some() {
        health.events.into_iter().filter(|e| e.last_seen.is_none()).map(|e| e.name).collect()
    } else {
        vec![]
    };

    // The first step of each funnel that nobody reaches, although the step
    // before it has sessions. Later steps are unreached because of it.
    for report in crate::funnel::report(pool, p, days).await? {
        let steps = &report.funnel.steps;
        let Some(i) = (1..report.steps.len()).find(|&i| report.steps[i].sessions == 0 && report.steps[i - 1].sessions > 0) else { continue };
        let (before, step) = (&steps[i - 1].matcher, &steps[i].matcher);
        // An event that never arrives is its own candidate.
        if step.kind == "custom" && step.name.as_ref().is_some_and(|n| missing.contains(n)) {
            continue;
        }
        let pages = [before, step].iter().filter(|m| m.kind == "pageview").filter_map(|m| m.path.clone()).collect();
        out.push(Candidate {
            key: format!("funnel:{}:{}", report.funnel.key, pattern(&steps[i].label)),
            kind: "unreached_step",
            label: format!("{} → {} ({})", steps[i - 1].label, steps[i].label, report.funnel.name),
            pages,
            count: 0,
            sessions: report.steps[i - 1].sessions,
            last_seen: None,
            metric: json!({ "measure": "conversion", "from": matcher_json(before), "to": matcher_json(step) }),
            goal: "increase",
            covered_by: None,
            uncovered: false,
        });
    }
    for name in missing {
        let m = EventMatch { kind: "custom".into(), name: Some(name.clone()), ..Default::default() };
        out.push(Candidate {
            key: format!("event:{name}"),
            kind: "missing_event",
            label: name,
            pages: vec![],
            count: 0,
            sessions: 0,
            last_seen: None,
            metric: json!({ "measure": "sessions", "match": matcher_json(&m) }),
            goal: "increase",
            covered_by: None,
            uncovered: false,
        });
    }

    // Covered when an insight has the key as its fingerprint (agents' are
    // stored with an "agent:" prefix). Seen by a sweep since it last happened,
    // it waits until it happens again; something that never happens waits for good.
    let fingerprints: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, status, fingerprint FROM insights WHERE project_id = $1 AND fingerprint IS NOT NULL").bind(p).fetch_all(pool).await?;
    let swept_at: Option<i64> = sqlx::query_scalar("SELECT swept_at FROM projects WHERE id = $1").bind(p).fetch_one(pool).await?;
    for c in &mut out {
        c.covered_by = fingerprints
            .iter()
            .find(|(_, _, f)| f.strip_prefix("agent:").unwrap_or(f) == c.key)
            .map(|(id, status, _)| Covered { insight: *id, status: status.clone() });
        let seen_since_sweep = match (swept_at, c.last_seen) {
            (None, _) => true,
            (Some(swept), Some(last)) => last > swept,
            (Some(_), None) => false,
        };
        c.uncovered = c.covered_by.is_none() && seen_since_sweep;
    }

    // Uncovered first, then the most sessions, the most occurrences.
    out.sort_by(|a, b| b.uncovered.cmp(&a.uncovered).then(b.sessions.cmp(&a.sessions)).then(b.count.cmp(&a.count)).then(a.key.cmp(&b.key)));
    out.truncate(LIMIT);
    Ok(Defects { days, swept_at, uncovered: out.iter().filter(|c| c.uncovered).count(), candidates: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_look_interactive_and_text_does_not() {
        for yes in ["button.pay", "div.actions > button", "a", "nav > a.home", "[role=button]", "div[role=\"tab\"]", "li.choice-button", "span.btn", "div.link-card", "input#email"] {
            assert!(looks_interactive(yes), "{yes}");
        }
        for no in ["div.question-body > p", "html", "body.h-full > div.flex.min-h-full", "div.border-2.bg-white > math-field", "span.text-xl.font-bold", "div.linked-list"] {
            assert!(!looks_interactive(no), "{no}");
        }
    }

    async fn session(pool: &Pool, site: i64, id: &str, at: i64, events: &[(&str, &str, &str, &str, &str)]) {
        sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at) VALUES ($1, $2, $3, $3)").bind(id).bind(site).bind(at).execute(pool).await.unwrap();
        for (i, (kind, path, selector, name, text)) in events.iter().enumerate() {
            let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, selector, name, text) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
                .bind(site).bind(id).bind(at + i as i64 * 1000).bind(*kind).bind(opt(path)).bind(opt(selector)).bind(opt(name)).bind(opt(text))
                .execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn a_small_site_s_defects() {
        let target = crate::db::test_target("defects").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Kids", None).await.unwrap();
        let s = site.id;
        let trace_json = r#"{
          "events": { "signup": "Made an account", "login": "Signed in" },
          "funnels": { "acquisition": { "name": "Visitor to student", "steps": [{ "pageview": "/" }, { "pageview": "/sign-up" }, { "event": "signup" }] } }
        }"#;
        crate::config::apply(&pool, s, &crate::config::parse(trace_json).unwrap(), "test").await.unwrap();
        let t = now_ms() - DAY_MS;
        let failed = "POST stats.example.com/api/event \u{2192} failed";
        // One session's analytics script fails on every page; eight sessions see / and none signs up.
        session(&pool, s, "a", t, &[("pageview", "/", "", "", ""), ("error", "/", "", "network", failed), ("pageview", "/home", "", "", ""),
            ("error", "/home", "", "network", failed), ("dead_click", "/home", "button.pay", "", ""), ("dead_click", "/home", "div.question-body > p", "", ""),
            ("custom", "/home", "", "login", "")]).await;
        for i in 0..7 {
            session(&pool, s, &format!("v{i}"), t + i, &[("pageview", "/", "", "", "")]).await;
        }

        let d = candidates(&pool, s, 14).await.unwrap();
        let find = |key: &str| d.candidates.iter().find(|c| c.key == key).unwrap_or_else(|| panic!("{key} in {:?}", d.candidates.iter().map(|c| &c.key).collect::<Vec<_>>()));
        let err = find(&format!("error:network:{failed}"));
        assert_eq!((err.kind, err.count, err.sessions, err.pages.len()), ("failing_request", 2, 1, 2));
        assert_eq!(err.metric["match"]["text"], json!(failed));
        let step = find("funnel:acquisition:/sign-up");
        assert_eq!((step.kind, step.sessions, step.goal), ("unreached_step", 8, "increase"));
        assert_eq!(step.metric["measure"], json!("conversion"));
        assert_eq!(find("event:signup").kind, "missing_event");
        assert!(!d.candidates.iter().any(|c| c.key == "event:login"), "login arrives");
        assert_eq!(find("dead:/home:button.pay").kind, "dead_control");
        assert!(!d.candidates.iter().any(|c| c.label.contains("question-body")), "text isn't meant to be clicked");
        assert_eq!(d.uncovered, d.candidates.len());
        // Every suggested metric is one query_behaviour takes.
        for c in &d.candidates {
            crate::query::Metric::parse(&c.metric).unwrap_or_else(|e| panic!("{}: {e}", c.key));
        }
        crate::db::drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn covered_by_an_insight_or_a_sweep_until_it_happens_again() {
        let target = crate::db::test_target("defects_cover").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let s = site.id;
        crate::config::apply(&pool, s, &crate::config::parse(r#"{ "events": { "signup": "Made an account" } }"#).unwrap(), "test").await.unwrap();
        let t = now_ms() - DAY_MS;
        session(&pool, s, "a", t, &[("error", "/cart", "", "exception", "TypeError: x is undefined"), ("dead_click", "/cart", "a.next", "", "")]).await;
        sqlx::query("INSERT INTO insights (project_id, title, problem, proposal, source, fingerprint, created_at, updated_at, status, kind)
                     VALUES ($1, 'Fix the cart', 'p', 'p', 'agent', 'agent:error:exception:TypeError: x is undefined', 0, 0, 'dismissed', 'defect')")
            .bind(s).execute(&pool).await.unwrap();
        let d = candidates(&pool, s, 14).await.unwrap();
        let err = d.candidates.iter().find(|c| c.kind == "exception").unwrap();
        assert_eq!((err.covered_by.as_ref().map(|c| c.status.as_str()), err.uncovered), (Some("dismissed"), false));
        assert_eq!(d.uncovered, 2, "the dead link and the missing event");

        crate::readiness::mark_swept(&pool, s).await.unwrap();
        assert_eq!(candidates(&pool, s, 14).await.unwrap().uncovered, 0, "looked at by the sweep");
        sqlx::query("UPDATE projects SET swept_at = $1 WHERE id = $2").bind(now_ms() - 60_000).bind(s).execute(&pool).await.unwrap();
        session(&pool, s, "b", now_ms() - 1000, &[("dead_click", "/cart", "a.next", "", "")]).await;
        let d = candidates(&pool, s, 14).await.unwrap();
        assert_eq!(d.candidates.iter().filter(|c| c.uncovered).map(|c| c.key.as_str()).collect::<Vec<_>>(), ["dead:/cart:a.next"], "it happened again");
        crate::db::drop_test_target(&target).await;
    }
}
