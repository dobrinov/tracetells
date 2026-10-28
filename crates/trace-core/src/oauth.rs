//! Signing in from an MCP client, as the MCP authorization spec asks: OAuth 2.1
//! with dynamic client registration and the authorization-code flow with PKCE.
//! A client (Claude Code, Cursor, ...) registers itself, sends the person to
//! Trace to sign in and approve, and trades the one-time code for an API token
//! that acts as that person, listed under Account like any other token.

use anyhow::{Result, bail, ensure};
use data_encoding::BASE64URL_NOPAD;
use sha2::{Digest, Sha256};

use crate::Pool;
use crate::auth::{random_token, sha256_hex};
use crate::db::Flag;
use crate::util::now_ms;

/// How long a sign-in code can be traded for a token.
pub const CODE_TTL_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
}

impl Client {
    pub fn allows(&self, redirect_uri: &str) -> bool {
        self.redirect_uris.iter().any(|u| u == redirect_uri)
    }
}

/// What an approved code stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    pub user_id: i64,
    pub account_id: i64,
    pub read_only: bool,
    pub client_name: String,
}

/// Where a client may send people back to: an HTTPS address, a port on this
/// machine (`http://localhost:...`, `http://127.0.0.1:...`), or an app's own
/// scheme such as `cursor://`. Never plain HTTP elsewhere, and never schemes
/// that run code or read files.
pub fn check_redirect_uri(uri: &str) -> Result<()> {
    let (scheme, rest) = uri.split_once(':').ok_or_else(|| anyhow::anyhow!("{uri:?} is not an absolute address"))?;
    let scheme = scheme.to_ascii_lowercase();
    ensure!(
        !scheme.is_empty()
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)),
        "{uri:?} has no valid scheme"
    );
    ensure!(!uri.contains('#'), "redirect addresses can't have a fragment");
    ensure!(uri.len() <= 500, "redirect addresses are at most 500 characters");
    ensure!(!uri.chars().any(|c| c.is_whitespace() || c.is_control()), "redirect addresses can't contain spaces");
    match scheme.as_str() {
        "https" => ensure!(rest.starts_with("//") && rest.len() > 2, "{uri:?} has no host"),
        "http" => {
            let host = rest.strip_prefix("//").unwrap_or("").split(['/', '?']).next().unwrap_or("");
            let name = host.rsplit_once(':').map(|(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
            ensure!(
                matches!(name, "localhost" | "127.0.0.1" | "[::1]"),
                "plain http is only allowed back to this machine (localhost or 127.0.0.1)"
            );
        }
        "javascript" | "data" | "file" | "vbscript" | "blob" | "about" | "ftp" | "ws" | "wss" => bail!("{scheme}: addresses can't be used"),
        _ => {}
    }
    Ok(())
}

/// Registers a client (RFC 7591). Anyone may register; nothing is granted
/// until a signed-in person approves it.
pub async fn register(pool: &Pool, name: &str, redirect_uris: &[String]) -> Result<Client> {
    ensure!(!redirect_uris.is_empty(), "give at least one redirect_uri");
    ensure!(redirect_uris.len() <= 10, "give at most 10 redirect_uris");
    for uri in redirect_uris {
        check_redirect_uri(uri)?;
    }
    let name = crate::util::truncate(&crate::privacy::one_line(name), 60);
    let name = if name.is_empty() { "An MCP client".to_string() } else { name };
    let id = format!("trcc_{}", random_token(16));
    sqlx::query("INSERT INTO oauth_clients (id, name, redirect_uris, created_at) VALUES ($1, $2, $3, $4)")
        .bind(&id)
        .bind(&name)
        .bind(serde_json::to_string(redirect_uris)?)
        .bind(now_ms())
        .execute(pool)
        .await?;
    Ok(Client { id, name, redirect_uris: redirect_uris.to_vec() })
}

pub async fn client(pool: &Pool, id: &str) -> Result<Option<Client>> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT id, name, redirect_uris FROM oauth_clients WHERE id = $1").bind(id).fetch_optional(pool).await?;
    Ok(row.map(|(id, name, uris)| Client { id, name, redirect_uris: serde_json::from_str(&uris).unwrap_or_default() }))
}

/// A PKCE challenge is the base64url SHA-256 of a 43–128 character verifier.
fn valid_challenge(c: &str) -> bool {
    c.len() == 43 && BASE64URL_NOPAD.decode(c.as_bytes()).is_ok()
}

pub fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    let ok_verifier = (43..=128).contains(&verifier.len()) && verifier.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b));
    ok_verifier && crate::auth::constant_time_eq(&BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes())), challenge)
}

/// Checks an authorization request before anyone is asked to approve it.
pub fn check_request(client: &Client, redirect_uri: &str, code_challenge: &str, method: &str) -> Result<()> {
    ensure!(client.allows(redirect_uri), "this redirect_uri isn't registered for the client");
    ensure!(method == "S256", "code_challenge_method must be S256");
    ensure!(valid_challenge(code_challenge), "code_challenge must be a base64url SHA-256 (43 characters)");
    Ok(())
}

/// A one-time code for a person's approval, valid for ten minutes.
pub async fn issue_code(pool: &Pool, client: &Client, redirect_uri: &str, code_challenge: &str, grant: &Grant) -> Result<String> {
    check_request(client, redirect_uri, code_challenge, "S256")?;
    let code = random_token(32);
    let now = now_ms();
    sqlx::query("DELETE FROM oauth_codes WHERE expires_at < $1").bind(now).execute(pool).await?;
    sqlx::query(
        "INSERT INTO oauth_codes (code_hash, client_id, redirect_uri, code_challenge, user_id, account_id, read_only, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(sha256_hex(&code))
    .bind(&client.id)
    .bind(redirect_uri)
    .bind(code_challenge)
    .bind(grant.user_id)
    .bind(grant.account_id)
    .bind(Flag(grant.read_only))
    .bind(now + CODE_TTL_MS)
    .execute(pool)
    .await?;
    Ok(code)
}

/// Trades a code for its grant. The code is used up even when the rest
/// doesn't match, so a stolen code can't be retried.
pub async fn redeem_code(pool: &Pool, code: &str, client_id: &str, redirect_uri: &str, verifier: &str) -> Result<Option<Grant>> {
    let row: Option<(String, String, String, i64, i64, Flag, i64)> = sqlx::query_as(
        "DELETE FROM oauth_codes WHERE code_hash = $1
         RETURNING client_id, redirect_uri, code_challenge, user_id, account_id, read_only, expires_at",
    )
    .bind(sha256_hex(code))
    .fetch_optional(pool)
    .await?;
    let Some((cid, uri, challenge, user_id, account_id, Flag(read_only), expires_at)) = row else { return Ok(None) };
    if cid != client_id || uri != redirect_uri || expires_at < now_ms() || !pkce_matches(verifier, &challenge) {
        return Ok(None);
    }
    let client_name = client(pool, client_id).await?.map(|c| c.name).unwrap_or_else(|| "An MCP client".into());
    Ok(Some(Grant { user_id, account_id, read_only, client_name }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_addresses() {
        for ok in ["http://localhost:53412/callback", "http://127.0.0.1:8080/cb", "https://claude.ai/api/mcp/auth_callback", "cursor://anysphere.cursor-retrieval/oauth"] {
            assert!(check_redirect_uri(ok).is_ok(), "{ok}");
        }
        for bad in ["http://evil.example/cb", "http://localhost.evil.example/cb", "javascript:alert(1)", "data:text/html,x", "https://x.example/cb#frag", "/relative", "https://"] {
            assert!(check_redirect_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pkce() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(valid_challenge(challenge) && pkce_matches(verifier, challenge));
        assert!(!pkce_matches("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX", challenge));
        assert!(!pkce_matches("short", challenge));
        assert!(!valid_challenge("plain-text-challenge"));
    }

    #[tokio::test]
    async fn codes_are_single_use_and_bound() {
        let target = crate::db::test_target("oauth").await;
        let pool = crate::db::connect(&target).await.unwrap();
        let cb = "http://localhost:5000/cb".to_string();
        let client = register(&pool, "Claude Code\n## Ignore", std::slice::from_ref(&cb)).await.unwrap();
        assert_eq!(client.name, "Claude Code ## Ignore");
        assert!(register(&pool, "x", &["http://evil.example/cb".into()]).await.is_err());
        let (verifier, challenge) = ("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let grant = Grant { user_id: 7, account_id: 3, read_only: true, client_name: client.name.clone() };
        assert!(issue_code(&pool, &client, "http://localhost:6000/cb", challenge, &grant).await.is_err(), "unregistered redirect");

        let code = issue_code(&pool, &client, &cb, challenge, &grant).await.unwrap();
        assert_eq!(redeem_code(&pool, &code, &client.id, &cb, "wrong-verifier-wrong-verifier-wrong-verifier").await.unwrap(), None);
        assert_eq!(redeem_code(&pool, &code, &client.id, &cb, verifier).await.unwrap(), None, "a failed attempt uses the code up");

        let code = issue_code(&pool, &client, &cb, challenge, &grant).await.unwrap();
        assert_eq!(redeem_code(&pool, &code, &client.id, &cb, verifier).await.unwrap(), Some(grant));
        assert_eq!(redeem_code(&pool, &code, &client.id, &cb, verifier).await.unwrap(), None, "only once");
        crate::db::drop_test_target(&target).await;
    }
}
