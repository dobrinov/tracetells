//! Problems by page: where a website's visitors hit dead clicks, rage clicks
//! and browser errors, and how many sessions each page's problems touched.
//! A page's problems in detail are in its story ([`crate::story`]).

use anyhow::Result;
use serde::Serialize;

use crate::Pool;
use crate::digest::Window;
use crate::util::{DAY_MS, now_ms};

/// Pages listed, most sessions hit first.
pub const MAX_PAGES: i64 = 50;

#[derive(Debug, Serialize, PartialEq)]
pub struct PageProblems {
    pub path: String,
    pub dead_clicks: i64,
    pub rage_clicks: i64,
    pub errors: i64,
    /// Sessions that hit any of them on this page.
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
pub struct Problems {
    pub window: Window,
    pub pages: Vec<PageProblems>,
    /// Sessions that hit a problem anywhere.
    pub sessions: i64,
}

pub async fn report(pool: &Pool, site: i64, days: u32) -> Result<Problems> {
    let to = now_ms();
    let from = to - days as i64 * DAY_MS;
    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT path,
           SUM(CASE WHEN type = 'dead_click' THEN 1 ELSE 0 END),
           SUM(CASE WHEN type = 'rage_click' THEN 1 ELSE 0 END),
           SUM(CASE WHEN type = 'error' THEN 1 ELSE 0 END),
           COUNT(DISTINCT session_id)
         FROM events
         WHERE project_id = $1 AND type IN ('dead_click', 'rage_click', 'error') AND ts >= $2 AND ts < $3 AND path IS NOT NULL
         GROUP BY path ORDER BY 5 DESC, 2 DESC, 1 LIMIT $4",
    )
    .bind(site).bind(from).bind(to).bind(MAX_PAGES)
    .fetch_all(pool).await?;
    let (sessions,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT session_id) FROM events WHERE project_id = $1 AND type IN ('dead_click', 'rage_click', 'error') AND ts >= $2 AND ts < $3",
    )
    .bind(site).bind(from).bind(to)
    .fetch_one(pool).await?;
    let pages = rows
        .into_iter()
        .map(|(path, dead_clicks, rage_clicks, errors, sessions)| PageProblems { path: crate::privacy::one_line_path(&path), dead_clicks, rage_clicks, errors, sessions })
        .collect();
    Ok(Problems { window: Window { from, to, days }, pages, sessions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pages_by_sessions_hit() {
        let target = crate::db::test_target("problems").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Kids", None).await.unwrap();
        let t = now_ms() - DAY_MS;
        let mut tx = pool.begin().await.unwrap();
        // /questions/:id: 247 dead, 81 rage, 4 errors across 19 sessions.
        let add = |kind: &'static str, path: &'static str, n: usize, sessions: usize| {
            (0..n).map(move |i| (kind, path, format!("{path}-s{}", i % sessions))).collect::<Vec<_>>()
        };
        let mut all = add("dead_click", "/questions/:id", 247, 19);
        all.extend(add("rage_click", "/questions/:id", 81, 10));
        all.extend(add("error", "/questions/:id", 4, 2));
        all.extend(add("dead_click", "/sign-in", 11, 6));
        for (i, (kind, path, sid)) in all.into_iter().enumerate() {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path) VALUES ($1, $2, $3, $4, $5)")
                .bind(site.id).bind(sid).bind(t + i as i64).bind(kind).bind(path).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let r = report(&pool, site.id, 7).await.unwrap();
        assert_eq!(r.pages, [
            PageProblems { path: "/questions/:id".into(), dead_clicks: 247, rage_clicks: 81, errors: 4, sessions: 19 },
            PageProblems { path: "/sign-in".into(), dead_clicks: 11, rage_clicks: 0, errors: 0, sessions: 6 },
        ]);
        assert_eq!(r.sessions, 25);
        crate::db::drop_test_target(&target).await;
    }
}
