//! A website's public, read-only view: an owner or admin can publish a link
//! that shows the website's aggregated analytics to anyone, and replace or
//! withdraw it at any time. Only aggregates leave, and rows built from very few
//! sessions are left out so a quiet website can't expose one visitor's path.

use anyhow::Result;
use serde::Serialize;

use crate::Pool;
use crate::auth::random_token;
use crate::digest::Digest;
use crate::model::Project;
use crate::util::now_ms;

/// Rows from fewer sessions than this are left out of the public view.
pub const MIN_SESSIONS: i64 = 5;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PublicLink {
    pub token: String,
    pub since: i64,
    pub by: String,
}

pub async fn link(pool: &Pool, site_id: i64) -> Result<Option<PublicLink>> {
    let row: Option<(Option<String>, Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT public_token, public_since, public_by FROM projects WHERE id = $1").bind(site_id).fetch_optional(pool).await?;
    Ok(row.and_then(|(t, s, b)| Some(PublicLink { token: t?, since: s.unwrap_or(0), by: b.unwrap_or_default() })))
}

/// Publishes the view, keeping the current link if there is one.
pub async fn publish(pool: &Pool, site_id: i64, by: &str) -> Result<PublicLink> {
    if let Some(l) = link(pool, site_id).await? {
        return Ok(l);
    }
    replace(pool, site_id, by).await
}

/// A new link; the old one stops working at once.
pub async fn replace(pool: &Pool, site_id: i64, by: &str) -> Result<PublicLink> {
    let l = PublicLink { token: random_token(24), since: now_ms(), by: by.to_string() };
    sqlx::query("UPDATE projects SET public_token = $1, public_since = $2, public_by = $3 WHERE id = $4")
        .bind(&l.token)
        .bind(l.since)
        .bind(&l.by)
        .bind(site_id)
        .execute(pool)
        .await?;
    Ok(l)
}

pub async fn withdraw(pool: &Pool, site_id: i64) -> Result<()> {
    sqlx::query("UPDATE projects SET public_token = NULL, public_since = NULL, public_by = NULL WHERE id = $1")
        .bind(site_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The website a public link shows, if the link is current.
pub async fn site(pool: &Pool, token: &str) -> Result<Option<Project>> {
    if token.len() < 20 {
        return Ok(None);
    }
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE public_token = $1").bind(token).fetch_optional(pool).await?;
    Ok(match id {
        Some(id) => Some(crate::db::project(pool, id).await?),
        None => None,
    })
}

/// The digest with every row from fewer than [`MIN_SESSIONS`] sessions left
/// out. Totals and KPI values cover the whole website and stay.
pub fn suppress(mut d: Digest) -> Digest {
    let keep = |sessions: i64| sessions >= MIN_SESSIONS;
    d.devices.retain(|x| keep(x.sessions));
    d.pages.retain(|x| keep(x.sessions));
    d.transitions.retain(|x| keep(x.count));
    d.rage_clicks.retain(|x| keep(x.sessions));
    d.dead_clicks.retain(|x| keep(x.sessions));
    d.errors.retain(|x| keep(x.sessions));
    d.forms.retain(|x| keep(x.started));
    d.custom_events.retain(|x| keep(x.sessions) || (x.declared && x.count == 0));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn links_publish_rotate_and_withdraw() {
        let target = crate::db::test_target("public").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        assert_eq!(link(&pool, site.id).await.unwrap(), None);
        let first = publish(&pool, site.id, "a@shop.example").await.unwrap();
        assert_eq!(publish(&pool, site.id, "b@shop.example").await.unwrap(), first, "publishing again keeps the link");
        assert_eq!(self::site(&pool, &first.token).await.unwrap().map(|s| s.id), Some(site.id));
        let second = replace(&pool, site.id, "a@shop.example").await.unwrap();
        assert!(self::site(&pool, &first.token).await.unwrap().is_none(), "the old link stops working");
        withdraw(&pool, site.id).await.unwrap();
        assert!(self::site(&pool, &second.token).await.unwrap().is_none());
        assert!(self::site(&pool, "").await.unwrap().is_none());
        crate::db::drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn rare_rows_are_left_out() {
        let target = crate::db::test_target("suppress").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, site) = crate::db::create_account_with_site(&pool, "Shop", None).await.unwrap();
        let t = now_ms() - 60_000;
        // Six sessions see / then /products; one also visits a page nobody else does, and hits an error there.
        for i in 0..6 {
            let sid = format!("s{i}");
            sqlx::query("INSERT INTO sessions (id, project_id, started_at, last_seen_at) VALUES ($1, $2, $3, $3)").bind(&sid).bind(site.id).bind(t).execute(&pool).await.unwrap();
            let mut steps = vec![("pageview", "/"), ("pageview", "/products")];
            if i == 0 {
                steps.extend([("pageview", "/account/rare"), ("error", "/account/rare")]);
            }
            for (k, (kind, path)) in steps.into_iter().enumerate() {
                sqlx::query("INSERT INTO events (project_id, session_id, ts, type, path, text) VALUES ($1, $2, $3, $4, $5, 'boom')")
                    .bind(site.id).bind(&sid).bind(t + k as i64 * 1000).bind(kind).bind(path).execute(&pool).await.unwrap();
            }
        }
        let full = crate::digest::build(&pool, site.id, 1).await.unwrap();
        assert!(full.pages.iter().any(|p| p.path == "/account/rare") && !full.errors.is_empty());
        let public = suppress(crate::digest::build(&pool, site.id, 1).await.unwrap());
        let mut pages: Vec<&str> = public.pages.iter().map(|p| p.path.as_str()).collect();
        pages.sort();
        assert_eq!(pages, ["/", "/products"]);
        assert!(public.errors.is_empty() && public.transitions.iter().all(|t| t.to != "/account/rare"));
        assert_eq!(public.transitions.len(), 1);
        assert_eq!(public.totals.sessions, full.totals.sessions, "totals stay");
        crate::db::drop_test_target(&target).await;
    }
}
