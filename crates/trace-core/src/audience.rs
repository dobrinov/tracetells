//! A website's audience: how many people visit, how many leave the marketing
//! pages straight away, how many do something that makes them engaged, and,
//! for people the app signs in, daily and monthly actives and retention.
//!
//! Visitors are cookieless (see [`crate::visits`]): an anonymous visitor can't
//! be followed from one day to the next, so visitors are counted per day and a
//! window's total is the sum of its days. People the app identifies with
//! `trace.identify()` are the same person every day, so DAU, MAU and retention
//! count only them. Everything here is a count; no person is ever listed.
//!
//! What counts as a marketing page and as an engaging action is declared in
//! trace.json:
//!
//! ```json
//! "audience": {
//!   "marketing": ["/", "/pricing", "/blog/*"],
//!   "engaged": [{ "event": "signup" }, { "event": "login" }]
//! }
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Pool;
use crate::model::{EventMatch, pattern_matches};
use crate::util::{DAY_MS, date, now_ms};

/// Weeks after the first one that retention follows.
pub const RETENTION_WEEKS: usize = 8;
/// Event types that mean someone did something on the page, not only looked.
const INTERACTIONS: &str = "'click', 'rage_click', 'dead_click', 'form_start', 'submit', 'custom'";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudienceSpec {
    /// Path patterns of the marketing pages; empty means every landing page.
    #[serde(default)]
    pub marketing: Vec<String>,
    /// Actions that make a visitor engaged: a sign-up, a sign-in, a first real use.
    #[serde(default)]
    pub engaged: Vec<EventMatch>,
}

pub async fn declared(pool: &Pool, project_id: i64) -> Result<AudienceSpec> {
    let config: Option<String> = sqlx::query_scalar("SELECT config FROM projects WHERE id = $1").bind(project_id).fetch_optional(pool).await?.flatten();
    #[derive(Deserialize)]
    struct Stored {
        #[serde(default)]
        audience: Option<AudienceSpec>,
    }
    Ok(config.and_then(|c| serde_json::from_str::<Stored>(&c).ok()).and_then(|s| s.audience).unwrap_or_default())
}

#[derive(Debug, Serialize)]
pub struct Day {
    pub day: String,
    pub visitors: i64,
    pub sessions: i64,
    /// People the app signed in that day.
    pub signed_in: i64,
}

#[derive(Debug, Serialize)]
pub struct Visitors {
    /// Visitors per day added up: an anonymous visitor counts once a day.
    pub visitors: i64,
    pub previous_visitors: i64,
    pub sessions: i64,
    pub previous_sessions: i64,
    pub daily_average: f64,
    /// Different signed-in people in the window: these are counted exactly.
    pub signed_in: i64,
    pub days: Vec<Day>,
}

#[derive(Debug, Serialize)]
pub struct Landing {
    pub path: String,
    pub sessions: i64,
    pub bounced: i64,
    pub rate: f64,
}

#[derive(Debug, Serialize)]
pub struct DeviceRate {
    pub device: String,
    pub sessions: i64,
    pub rate: f64,
}

#[derive(Debug, Serialize)]
pub struct Bounce {
    /// The marketing pages' patterns; empty when trace.json declares none and every landing page counts.
    pub pages: Vec<String>,
    /// Sessions that landed on a marketing page, and those that left after it without doing anything.
    pub sessions: i64,
    pub bounced: i64,
    pub rate: Option<f64>,
    pub previous_rate: Option<f64>,
    pub by_page: Vec<Landing>,
    pub by_device: Vec<DeviceRate>,
}

#[derive(Debug, Serialize)]
pub struct Action {
    pub label: String,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct Engaged {
    /// Whether trace.json declares the engaging actions; without them nothing is counted.
    pub declared: bool,
    /// Visitors (per day) who did at least one engaging action, and their share of all visitors.
    pub visitors: i64,
    pub rate: Option<f64>,
    pub previous_rate: Option<f64>,
    /// Different signed-in people who did one.
    pub people: i64,
    pub by_action: Vec<Action>,
}

#[derive(Debug, Serialize)]
pub struct Active {
    /// Whether the app signs anyone in with trace.identify(); DAU, MAU and retention need it.
    pub identified: bool,
    /// Average different signed-in people per day over the last 7 days.
    pub dau: f64,
    pub wau: i64,
    pub mau: i64,
    /// DAU ÷ MAU: how many days a month the average person comes back.
    pub stickiness: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Cohort {
    /// The Monday (UTC) of the week these people were first signed in.
    pub week: String,
    pub people: i64,
    /// Share of the cohort seen again in each following week; None until that week is over.
    pub returned: Vec<Option<f64>>,
}

#[derive(Debug, Serialize)]
pub struct Retention {
    pub cohorts: Vec<Cohort>,
    /// Each week after the first, weighted by cohort size, over the cohorts that reached it.
    pub average: Vec<Option<f64>>,
}

#[derive(Debug, Serialize)]
pub struct Audience {
    pub days: u32,
    pub visitors: Visitors,
    pub bounce: Bounce,
    pub engaged: Engaged,
    pub active: Active,
    pub retention: Retention,
}

fn ratio(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

struct Session {
    id: String,
    /// The visitor that day: a signed-in person, or the day's visitor hash.
    who: String,
    user: Option<String>,
    day: String,
    device: String,
}

async fn sessions(pool: &Pool, project_id: i64, from: i64, to: i64) -> Result<Vec<Session>> {
    let rows: Vec<(String, Option<String>, Option<String>, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, visitor_id, user_id, started_at, device FROM sessions WHERE project_id = $1 AND started_at >= $2 AND started_at < $3",
    )
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, visitor, user, started_at, device)| Session {
            // A signed-in person is the same visitor all day; the prefix keeps a user id from matching a hash.
            who: user.as_ref().map(|u| format!("u:{u}")).or(visitor).unwrap_or_else(|| id.clone()),
            id,
            user,
            day: date(started_at),
            device: device.unwrap_or_else(|| "unknown".into()),
        })
        .collect())
}

/// Visitor-days in a set of sessions: each visitor once per day.
fn visitor_days<'a>(list: impl Iterator<Item = &'a Session>) -> HashSet<(&'a str, &'a str)> {
    list.map(|s| (s.day.as_str(), s.who.as_str())).collect()
}

/// Per session landing on a marketing page: its landing path and whether it bounced.
async fn landings(pool: &Pool, project_id: i64, spec: &AudienceSpec, list: &[Session], from: i64, to: i64) -> Result<HashMap<String, (String, bool)>> {
    let first: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT session_id, path FROM (
           SELECT session_id, path, ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY ts, id) AS nth FROM events
           WHERE project_id = $1 AND type = 'pageview' AND ts >= $2 AND ts < $3
         ) t WHERE nth = 1",
    )
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let activity: HashMap<String, (i64, i64)> = sqlx::query_as::<_, (String, i64, i64)>(&format!(
        "SELECT session_id, COUNT(CASE WHEN type = 'pageview' THEN 1 END), COUNT(CASE WHEN type IN ({INTERACTIONS}) THEN 1 END)
         FROM events WHERE project_id = $1 AND ts >= $2 AND ts < $3 GROUP BY session_id"
    ))
    .bind(project_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id, pages, actions)| (id, (pages, actions)))
    .collect();
    let started: HashSet<&str> = list.iter().map(|s| s.id.as_str()).collect();
    Ok(first
        .into_iter()
        .filter(|(id, _)| started.contains(id.as_str()))
        .filter_map(|(id, path)| {
            let path = path?;
            let marketing = spec.marketing.is_empty() || spec.marketing.iter().any(|p| pattern_matches(&Some(p.clone()), Some(&path)));
            marketing.then(|| {
                let (pages, actions) = activity.get(&id).copied().unwrap_or((1, 0));
                (id, (path, pages <= 1 && actions == 0))
            })
        })
        .collect())
}

async fn engaged_sessions(pool: &Pool, project_id: i64, spec: &AudienceSpec, from: i64, to: i64) -> Result<Vec<(String, HashSet<String>)>> {
    let mut out = Vec::new();
    for m in &spec.engaged {
        let ids = crate::kpi::hits(pool, project_id, m, from, to).await?.into_iter().map(|h| h.session_id).collect();
        out.push((crate::funnel::label(m), ids));
    }
    Ok(out)
}

/// Start of the week (Monday, UTC) a moment falls in.
fn monday(ms: i64) -> i64 {
    let day = ms.div_euclid(DAY_MS);
    // 1970-01-01 was a Thursday: three days after a Monday.
    (day - (day + 3).rem_euclid(7)) * DAY_MS
}

async fn retention(pool: &Pool, project_id: i64, now: i64) -> Result<Retention> {
    let this_week = monday(now);
    let first_week = this_week - RETENTION_WEEKS as i64 * 7 * DAY_MS;
    let firsts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT user_id, MIN(started_at) FROM sessions WHERE project_id = $1 AND user_id IS NOT NULL GROUP BY user_id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let firsts: HashMap<String, i64> = firsts.into_iter().filter(|(_, at)| *at >= first_week).collect();
    let seen: Vec<(String, i64)> =
        sqlx::query_as("SELECT user_id, started_at FROM sessions WHERE project_id = $1 AND user_id IS NOT NULL AND started_at >= $2")
            .bind(project_id)
            .bind(first_week)
            .fetch_all(pool)
            .await?;
    // Which weeks after their first each person came back in, counted from the week they started.
    let mut back: HashMap<&str, HashSet<usize>> = HashMap::new();
    for (user, at) in &seen {
        if let Some(first) = firsts.get(user) {
            let k = ((monday(*at) - monday(*first)) / (7 * DAY_MS)) as usize;
            if k > 0 {
                back.entry(user.as_str()).or_default().insert(k);
            }
        }
    }
    let mut by_week: BTreeMap<i64, Vec<&str>> = BTreeMap::new();
    for (user, first) in &firsts {
        by_week.entry(monday(*first)).or_default().push(user.as_str());
    }
    let mut cohorts = Vec::new();
    let (mut sums, mut sizes) = (vec![0i64; RETENTION_WEEKS], vec![0i64; RETENTION_WEEKS]);
    for (week, people) in by_week {
        let returned = (1..=RETENTION_WEEKS)
            .map(|k| {
                // Only weeks that are over: the current one would read low.
                (week + (k as i64 + 1) * 7 * DAY_MS <= now).then(|| {
                    let n = people.iter().filter(|u| back.get(*u).is_some_and(|w| w.contains(&k))).count() as i64;
                    sums[k - 1] += n;
                    sizes[k - 1] += people.len() as i64;
                    n as f64 / people.len() as f64
                })
            })
            .collect();
        cohorts.push(Cohort { week: date(week), people: people.len() as i64, returned });
    }
    let average = sums.iter().zip(&sizes).map(|(n, size)| ratio(*n, *size)).collect();
    Ok(Retention { cohorts, average })
}

async fn active(pool: &Pool, project_id: i64, now: i64) -> Result<Active> {
    let people = |from: i64| {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT user_id) FROM sessions WHERE project_id = $1 AND user_id IS NOT NULL AND started_at >= $2 AND started_at < $3")
            .bind(project_id)
            .bind(from)
            .bind(now)
            .fetch_one(pool)
    };
    let (wau, mau) = (people(now - 7 * DAY_MS).await?, people(now - 30 * DAY_MS).await?);
    // A rolling day at a time, so today's half-finished day doesn't drag the average down.
    let mut days = 0;
    for k in 0..7 {
        let (from, to) = (now - (k + 1) * DAY_MS, now - k * DAY_MS);
        days += sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT user_id) FROM sessions WHERE project_id = $1 AND user_id IS NOT NULL AND started_at >= $2 AND started_at < $3")
            .bind(project_id)
            .bind(from)
            .bind(to)
            .fetch_one(pool)
            .await?;
    }
    let ever: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE project_id = $1 AND user_id IS NOT NULL").bind(project_id).fetch_one(pool).await?;
    let dau = days as f64 / 7.0;
    Ok(Active { identified: ever > 0, dau, wau, mau, stickiness: (mau > 0).then(|| dau / mau as f64) })
}

/// The audience over the last `days`, compared with the `days` before.
pub async fn report(pool: &Pool, project_id: i64, days: u32) -> Result<Audience> {
    let now = now_ms();
    let window = i64::from(days) * DAY_MS;
    let (from, before) = (now - window, now - 2 * window);
    let spec = declared(pool, project_id).await?;
    let current = sessions(pool, project_id, from, now).await?;
    let previous = sessions(pool, project_id, before, from).await?;

    // Visitors.
    let mut per_day: BTreeMap<&str, (HashSet<&str>, i64, HashSet<&str>)> = BTreeMap::new();
    for s in &current {
        let d = per_day.entry(s.day.as_str()).or_default();
        d.0.insert(s.who.as_str());
        d.1 += 1;
        if let Some(u) = &s.user {
            d.2.insert(u.as_str());
        }
    }
    let day_list: Vec<Day> = per_day
        .into_iter()
        .map(|(day, (who, sessions, users))| Day { day: day.to_string(), visitors: who.len() as i64, sessions, signed_in: users.len() as i64 })
        .collect();
    let all_now = visitor_days(current.iter());
    let all_before = visitor_days(previous.iter());
    let signed_in = current.iter().filter_map(|s| s.user.as_deref()).collect::<HashSet<_>>().len() as i64;
    let visitors = Visitors {
        visitors: all_now.len() as i64,
        previous_visitors: all_before.len() as i64,
        sessions: current.len() as i64,
        previous_sessions: previous.len() as i64,
        daily_average: all_now.len() as f64 / f64::from(days.max(1)),
        signed_in,
        days: day_list,
    };

    // Bounce on the marketing pages.
    let land_now = landings(pool, project_id, &spec, &current, from, now).await?;
    let land_before = landings(pool, project_id, &spec, &previous, before, from).await?;
    let bounced_now = land_now.values().filter(|(_, b)| *b).count() as i64;
    let mut pages: HashMap<&str, (i64, i64)> = HashMap::new();
    for (path, bounced) in land_now.values() {
        let e = pages.entry(path.as_str()).or_default();
        e.0 += 1;
        e.1 += i64::from(*bounced);
    }
    let mut by_page: Vec<Landing> =
        pages.into_iter().map(|(path, (n, b))| Landing { path: path.to_string(), sessions: n, bounced: b, rate: b as f64 / n as f64 }).collect();
    by_page.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(a.path.cmp(&b.path)));
    by_page.truncate(10);
    let device: HashMap<&str, &str> = current.iter().map(|s| (s.id.as_str(), s.device.as_str())).collect();
    let mut devices: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
    for (id, (_, bounced)) in &land_now {
        let e = devices.entry(device.get(id.as_str()).copied().unwrap_or("unknown")).or_default();
        e.0 += 1;
        e.1 += i64::from(*bounced);
    }
    let mut by_device: Vec<DeviceRate> = devices.into_iter().map(|(d, (n, b))| DeviceRate { device: d.to_string(), sessions: n, rate: b as f64 / n as f64 }).collect();
    by_device.sort_by_key(|d| std::cmp::Reverse(d.sessions));
    let bounce = Bounce {
        pages: spec.marketing.clone(),
        sessions: land_now.len() as i64,
        bounced: bounced_now,
        rate: ratio(bounced_now, land_now.len() as i64),
        previous_rate: ratio(land_before.values().filter(|(_, b)| *b).count() as i64, land_before.len() as i64),
        by_page,
        by_device,
    };

    // Engaged.
    let actions_now = engaged_sessions(pool, project_id, &spec, from, now).await?;
    let actions_before = engaged_sessions(pool, project_id, &spec, before, from).await?;
    let any = |actions: &[(String, HashSet<String>)]| actions.iter().flat_map(|(_, ids)| ids.iter().cloned()).collect::<HashSet<String>>();
    let (any_now, any_before) = (any(&actions_now), any(&actions_before));
    let engaged_now = visitor_days(current.iter().filter(|s| any_now.contains(&s.id)));
    let engaged_before = visitor_days(previous.iter().filter(|s| any_before.contains(&s.id)));
    let engaged = Engaged {
        declared: !spec.engaged.is_empty(),
        visitors: engaged_now.len() as i64,
        rate: ratio(engaged_now.len() as i64, all_now.len() as i64).filter(|_| !spec.engaged.is_empty()),
        previous_rate: ratio(engaged_before.len() as i64, all_before.len() as i64).filter(|_| !spec.engaged.is_empty()),
        people: current.iter().filter(|s| any_now.contains(&s.id)).filter_map(|s| s.user.as_deref()).collect::<HashSet<_>>().len() as i64,
        by_action: actions_now.iter().map(|(label, ids)| Action { label: label.clone(), sessions: current.iter().filter(|s| ids.contains(&s.id)).count() as i64 }).collect(),
    };

    Ok(Audience { days, visitors, bounce, engaged, active: active(pool, project_id, now).await?, retention: retention(pool, project_id, now).await? })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn visit(pool: &Pool, site: i64, sid: &str, who: (&str, Option<&str>), at: i64, events: &[(&str, &str)]) {
        sqlx::query("INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at, device) VALUES ($1, $2, $3, $4, $5, $5, 'desktop')")
            .bind(sid)
            .bind(site)
            .bind(who.0)
            .bind(who.1)
            .bind(at)
            .execute(pool)
            .await
            .unwrap();
        for (i, (kind, what)) in events.iter().enumerate() {
            let (path, name) = if *kind == "custom" { ("/x", Some(*what)) } else { (*what, None) };
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, name) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(site)
                .bind(sid)
                .bind(at + i as i64 * 1000)
                .bind(kind)
                .bind(path)
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    #[test]
    fn weeks_start_on_monday() {
        // 2026-10-29 is a Thursday; its week starts on Monday the 26th.
        let thursday = crate::util::parse_time("2026-10-29T15:00Z").unwrap();
        assert_eq!(date(monday(thursday)), "2026-10-26");
        assert_eq!(monday(crate::util::parse_time("2026-10-26").unwrap()), crate::util::parse_time("2026-10-26").unwrap());
    }

    #[tokio::test]
    async fn counts_visitors_bounces_engagement_and_returning_people() {
        let path = crate::db::test_target("audience").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "School", None).await.unwrap();
        let config = crate::config::parse(
            r#"{ "events": { "signup": "Made an account", "login": "Signed in" },
                 "kpis": { "k": { "session_rate": { "event": "signup" }, "goal": "increase" } },
                 "audience": { "marketing": ["/", "/maths*"], "engaged": [ { "event": "signup" }, { "event": "login" } ] } }"#,
        )
        .unwrap();
        crate::config::apply(&pool, site.id, &config, "test").await.unwrap();
        let now = now_ms();
        let hour = 3_600_000;

        // Landed on the home page and left: a bounce.
        visit(&pool, site.id, "a", ("v_a", None), now - hour, &[("pageview", "/")]).await;
        // Landed on a curriculum page, clicked, signed up: engaged, no bounce.
        visit(&pool, site.id, "b", ("v_b", Some("u1")), now - hour, &[("pageview", "/maths/5"), ("click", "/maths/5"), ("custom", "signup")]).await;
        // Landed in the app: not a marketing page, but a signed-in visit.
        visit(&pool, site.id, "c", ("v_c", Some("u2")), now - hour, &[("pageview", "/home"), ("custom", "login")]).await;
        // u1 three weeks ago too, and the week after it.
        let early = now - 21 * DAY_MS;
        visit(&pool, site.id, "d", ("v_d", Some("u1")), early, &[("pageview", "/home")]).await;
        visit(&pool, site.id, "e", ("v_e", Some("u1")), early + 7 * DAY_MS, &[("pageview", "/home")]).await;

        let a = report(&pool, site.id, 7).await.unwrap();
        assert_eq!((a.visitors.visitors, a.visitors.sessions, a.visitors.signed_in), (3, 3, 2));
        assert_eq!((a.bounce.sessions, a.bounce.bounced), (2, 1));
        assert_eq!(a.bounce.by_page.iter().find(|p| p.path == "/").map(|p| p.bounced), Some(1));
        assert_eq!((a.engaged.visitors, a.engaged.people), (2, 2));
        assert_eq!(a.engaged.rate, Some(2.0 / 3.0));
        assert_eq!(a.engaged.by_action.iter().map(|x| x.sessions).collect::<Vec<_>>(), [1, 1]);
        assert!(a.active.identified);
        assert_eq!((a.active.wau, a.active.mau), (2, 2));
        // u1 started three weeks ago and came back the week after; u2 started this week.
        let first = a.retention.cohorts.iter().find(|c| c.week == date(monday(early))).unwrap();
        assert_eq!((first.people, first.returned[0]), (1, Some(1.0)));
        assert_eq!(a.retention.cohorts.iter().map(|c| c.people).sum::<i64>(), 2);

        // Without a declared audience, every landing page counts and engagement isn't measured.
        let (_, other) = crate::db::create_account_with_site(&pool, "Other", None).await.unwrap();
        visit(&pool, other.id, "z", ("v_z", None), now - hour, &[("pageview", "/anything")]).await;
        let b = report(&pool, other.id, 7).await.unwrap();
        assert_eq!((b.bounce.sessions, b.bounce.bounced, b.engaged.declared, b.engaged.rate), (1, 1, false, None));
        assert!(!b.active.identified);
        crate::db::drop_test_target(&path).await;
    }
}
