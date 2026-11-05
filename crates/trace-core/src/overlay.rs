//! Overlay links: a short-lived, read-only token that lets the overlay script,
//! running on the website's own page in a team member's browser, read that
//! website's page stories. Only the token's hash is stored.

use anyhow::Result;

use crate::Pool;
use crate::auth::{random_token, sha256_hex};
use crate::util::now_ms;

/// How long an overlay link works.
pub const LIFETIME_MS: i64 = 15 * 60 * 1000;

/// A new token for `site`, made by `by`. The token itself is returned once and never stored.
pub async fn mint(pool: &Pool, site: i64, by: &str) -> Result<String> {
    let token = random_token(32);
    let now = now_ms();
    sqlx::query("INSERT INTO overlay_tokens (token_hash, project_id, created_by, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(sha256_hex(&token)).bind(site).bind(by).bind(now).bind(now + LIFETIME_MS)
        .execute(pool).await?;
    Ok(token)
}

/// What a token is good for.
#[derive(Debug, PartialEq)]
pub enum Check {
    /// The website it reads.
    Valid(i64),
    Expired,
    Unknown,
}

pub async fn check(pool: &Pool, token: &str) -> Result<Check> {
    let row: Option<(i64, i64)> = sqlx::query_as("SELECT project_id, expires_at FROM overlay_tokens WHERE token_hash = $1")
        .bind(sha256_hex(token)).fetch_optional(pool).await?;
    Ok(match row {
        Some((site, expires)) if expires > now_ms() => Check::Valid(site),
        Some(_) => Check::Expired,
        None => Check::Unknown,
    })
}

/// Deletes tokens that stopped working more than a day ago (kept that long so
/// an old link can still say it expired). Returns how many went.
pub async fn purge(pool: &Pool) -> Result<u64> {
    Ok(sqlx::query("DELETE FROM overlay_tokens WHERE expires_at < $1").bind(now_ms() - crate::util::DAY_MS).execute(pool).await?.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tokens_work_for_their_website_for_a_while() {
        let target = crate::db::test_target("overlay").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let (_, a) = crate::db::create_account_with_site(&pool, "A", None).await.unwrap();
        let (_, b) = crate::db::create_account_with_site(&pool, "B", None).await.unwrap();
        let token = mint(&pool, a.id, "ana@example.com").await.unwrap();
        assert_eq!(check(&pool, &token).await.unwrap(), Check::Valid(a.id));
        assert_ne!(check(&pool, &token).await.unwrap(), Check::Valid(b.id), "a token reads only its own website");
        assert_eq!(check(&pool, "made-up").await.unwrap(), Check::Unknown);
        let stored: String = sqlx::query_scalar("SELECT token_hash FROM overlay_tokens").fetch_one(&pool).await.unwrap();
        assert_ne!(stored, token, "only the hash is stored");

        sqlx::query("UPDATE overlay_tokens SET expires_at = $1").bind(now_ms() - 1000).execute(&pool).await.unwrap();
        assert_eq!(check(&pool, &token).await.unwrap(), Check::Expired);
        assert_eq!(purge(&pool).await.unwrap(), 0, "an expired link can still say so for a day");
        sqlx::query("UPDATE overlay_tokens SET expires_at = $1").bind(now_ms() - 2 * crate::util::DAY_MS).execute(&pool).await.unwrap();
        crate::retention::purge(&pool, 0).await.unwrap();
        assert_eq!(check(&pool, &token).await.unwrap(), Check::Unknown, "the retention sweep removes old tokens");
        crate::db::drop_test_target(&target).await;
    }
}
