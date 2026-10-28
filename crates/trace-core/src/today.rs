//! A website's home: what needs the team's attention now, most important first.
//!
//! Each item is something to do or to know, with a link to where it's done:
//! finish setup, analyse now that there's enough new behaviour, review saved
//! insights, a release verdict that just came in, a KPI moving the wrong way,
//! the funnel step losing the most people, errors visitors hit, an event that
//! stopped arriving. Everything comes from what Trace already measures.

use anyhow::Result;
use serde::Serialize;

use crate::Pool;
use crate::model::{Direction, InsightStatus, Project, Verdict};
use crate::util::{DAY_MS, now_ms, pct};

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// What kind of thing this is: setup, analyse, review, verdict, measuring, kpi, funnel, errors, tracking.
    pub kind: &'static str,
    /// action (something to do), bad (something wrong), good (good news) or info.
    pub tone: &'static str,
    pub title: String,
    pub detail: String,
    /// Where to act on it: a dashboard page such as "insights" or "kpis".
    pub page: &'static str,
    pub action: String,
    #[serde(skip)]
    weight: i64,
}

#[derive(Debug, Serialize)]
pub struct Today {
    pub collecting: bool,
    /// Sessions in the last 7 days, and in the 7 before.
    pub sessions_week: i64,
    pub sessions_week_before: i64,
    pub items: Vec<Item>,
}

fn item(kind: &'static str, tone: &'static str, weight: i64, title: String, detail: String, page: &'static str, action: &str) -> Item {
    Item { kind, tone, title, detail, page, action: action.into(), weight }
}

fn step_name(key: &str) -> &'static str {
    match key {
        "website" => "add the website",
        "agent" => "connect your coding agent",
        "install" => "install the collector",
        "privacy" => "check what's protected",
        "kpis" => "define KPIs in code",
        "team" => "invite your team",
        _ => "the next setup step",
    }
}

async fn sessions_between(pool: &Pool, site: i64, from: i64, to: i64) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = $1 AND started_at >= $2 AND started_at < $3")
        .bind(site)
        .bind(from)
        .bind(to)
        .fetch_one(pool)
        .await?)
}

pub async fn build(pool: &Pool, site: &Project) -> Result<Today> {
    let now = now_ms();
    let collecting = crate::db::collection(pool, site.id).await?.active;
    let sessions_week = sessions_between(pool, site.id, now - 7 * DAY_MS, now).await?;
    let sessions_week_before = sessions_between(pool, site.id, now - 14 * DAY_MS, now - 7 * DAY_MS).await?;
    let mut items = Vec::new();

    // Setup first while it's unfinished: nothing else means much until data arrives.
    let checklist = crate::onboarding::checklist(pool, site).await?;
    if !checklist.complete {
        let next = checklist.steps.iter().find(|s| !s.done && !s.optional).map(|s| step_name(s.key)).unwrap_or("the next setup step");
        items.push(item(
            "setup",
            "action",
            if collecting { 500 } else { 10_000 },
            format!("Finish setting up: {next}"),
            format!("{} of {} setup steps done.", checklist.done, checklist.total),
            "start",
            "Continue setup",
        ));
    }

    let insights = crate::insights::list(pool, site.id, None).await?;
    // Verdicts settled in the last week: the news a release was waiting for.
    for i in insights.iter().filter(|i| i.status == InsightStatus::Released) {
        let Some(e) = i.evaluation.as_ref() else { continue };
        if e.settled_at.is_some_and(|t| now - t < 7 * DAY_MS) {
            let (tone, word) = match e.verdict {
                Verdict::Improved => ("good", "improved after the release"),
                Verdict::Regressed => ("bad", "got worse after the release"),
                Verdict::NoChange => ("info", "showed no significant change"),
                _ => ("info", "couldn't be judged"),
            };
            items.push(item("verdict", tone, 3_000, format!("\"{}\": the KPI {word}", i.title), e.summary.clone(), "insights", "See the verdict"));
        }
    }
    let measuring: Vec<&crate::model::Insight> = insights
        .iter()
        .filter(|i| i.status == InsightStatus::Released && i.evaluation.as_ref().is_some_and(|e| e.verdict == Verdict::Measuring))
        .collect();
    if let Some(first) = measuring.first() {
        let detail = first.evaluation.as_ref().map(|e| e.summary.clone()).unwrap_or_default();
        let title = if measuring.len() == 1 {
            format!("Measuring \"{}\"", first.title)
        } else {
            format!("Measuring {} releases", measuring.len())
        };
        items.push(item("measuring", "info", 200, title, detail, "insights", "See progress"));
    }

    let count = |s: InsightStatus| insights.iter().filter(|i| i.status == s).count();
    let (new, selected, in_progress) = (count(InsightStatus::New), count(InsightStatus::Selected), count(InsightStatus::InProgress));
    if new > 0 {
        items.push(item(
            "review",
            "action",
            2_000 + new as i64,
            format!("{new} new insight{} to review", if new == 1 { "" } else { "s" }),
            "Pick the ones worth doing; dismiss the rest with a reason, and analysis learns from it.".into(),
            "insights",
            "Review insights",
        ));
    }
    if selected + in_progress > 0 {
        items.push(item(
            "review",
            "info",
            300,
            format!("{} insight{} picked, {} in progress", selected, if selected == 1 { "" } else { "s" }, in_progress),
            "Hand one to your coding agent with /trace:fix, and mark it released when it ships.".into(),
            "insights",
            "Open insights",
        ));
    }

    let readiness = crate::readiness::status(pool, site).await?;
    if readiness.ready {
        items.push(item("analyse", "action", 2_500, "Worth analysing now".into(), readiness.message.clone(), "insights", "Analyse"));
    }

    // KPIs off target that also got worse than the window before.
    let span = 14 * DAY_MS;
    let mut kpi_items = Vec::new();
    for k in crate::kpi::list(pool, site.id).await? {
        let current = crate::kpi::measure(pool, site.id, &k.definition, now - span, now).await?;
        let previous = crate::kpi::measure(pool, site.id, &k.definition, now - 2 * span, now - span).await?;
        let (Some(c), Some(p)) = (current.value, previous.value) else { continue };
        if current.n() < 10 || crate::kpi::target_met(&k, c) == Some(true) {
            continue;
        }
        let worse = match k.direction {
            Direction::Increase => c < p,
            Direction::Decrease => c > p,
        };
        if !worse {
            continue;
        }
        let rate = k.definition.is_rate();
        let fmt = |v: f64| if rate { pct(v) } else { format!("{v:.2}") };
        let change = ((c - p).abs() * 1000.0) as i64;
        kpi_items.push(item(
            "kpi",
            "bad",
            1_000 + change,
            format!("{} is falling short and got worse", k.name),
            format!(
                "{} → {} over the last 14 days{}.",
                fmt(p),
                fmt(c),
                k.target.map(|t| format!(", target {}", fmt(t))).unwrap_or_default()
            ),
            "kpis",
            "Open KPIs",
        ));
    }
    kpi_items.sort_by_key(|i| std::cmp::Reverse(i.weight));
    items.extend(kpi_items.into_iter().take(2));

    // The funnel step losing the most people: where conversion is won.
    let funnels = crate::funnel::report(pool, site.id, 14).await?;
    if let Some((f, d)) = funnels.iter().filter_map(|f| f.biggest_drop.as_ref().map(|d| (f, d))).max_by_key(|(_, d)| d.lost)
        && d.lost >= 5
    {
        items.push(item(
            "funnel",
            "bad",
            800 + d.lost,
            format!("Most people stop between {} and {}", d.from_label, d.to_label),
            format!("In \"{}\", {} sessions stopped there in 14 days ({} didn't continue).", f.funnel.name, d.lost, pct(d.rate)),
            "kpis",
            "See the funnel",
        ));
    }

    // Errors visitors hit this week.
    let digest = crate::digest::build(pool, site.id, 7).await?;
    if let Some(top) = digest.errors.iter().max_by_key(|e| e.sessions)
        && top.sessions >= 3
    {
        let total: i64 = digest.errors.iter().map(|e| e.sessions).sum();
        items.push(item(
            "errors",
            "bad",
            700 + total,
            format!("Visitors hit errors in {total} sessions this week"),
            format!("Most often on {}: {}", top.path, crate::util::ellipsize(&top.message, 140)),
            "behaviour",
            "See errors",
        ));
    }

    // Declared events that stopped arriving: a KPI built on them reads zero.
    if collecting {
        let health = crate::config::tracking_health(pool, site.id, 14).await?;
        let silent: Vec<&str> = health.events.iter().filter(|e| e.count == 0).map(|e| e.name.as_str()).collect();
        if !silent.is_empty() {
            items.push(item(
                "tracking",
                "bad",
                600,
                format!("{} declared event{} not arriving", silent.len(), if silent.len() == 1 { " is" } else { "s are" }),
                format!("Not seen in 14 days: {}. KPIs and funnels that use them read zero.", silent.join(", ")),
                "kpis",
                "Check tracking",
            ));
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.weight));
    Ok(Today { collecting, sessions_week, sessions_week_before, items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn setup_comes_first_until_data_arrives_then_the_news() {
        let path = crate::db::test_target("today").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let t = build(&pool, &site).await.unwrap();
        assert!(!t.collecting);
        assert_eq!(t.items[0].kind, "setup", "{:?}", t.items);

        let config = crate::config::parse(r#"{ "events": { "purchase": "Paid" }, "kpis": { "k": { "session_rate": { "event": "purchase" }, "goal": "increase" } } }"#).unwrap();
        crate::config::apply(&pool, site.id, &config, "test").await.unwrap();
        sqlx::query("UPDATE projects SET last_event_at = $1 WHERE id = $2").bind(now_ms()).bind(site.id).execute(&pool).await.unwrap();
        let id = crate::insights::insert(&pool, site.id, &crate::model::NewInsight { title: "Fix pay".into(), problem: "p".into(), proposal: "x".into(), source: "test".into(), ..Default::default() })
            .await
            .unwrap()
            .unwrap();
        let t = build(&pool, &site).await.unwrap();
        let kinds: Vec<&str> = t.items.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&"review") && kinds.contains(&"tracking"), "{kinds:?}");
        let review = t.items.iter().find(|i| i.kind == "review").unwrap();
        assert_eq!((review.title.as_str(), review.page), ("1 new insight to review", "insights"));
        let tracking = t.items.iter().find(|i| i.kind == "tracking").unwrap();
        assert!(tracking.detail.contains("purchase"), "{}", tracking.detail);
        let _ = id;
        crate::db::drop_test_target(&path).await;
    }
}
