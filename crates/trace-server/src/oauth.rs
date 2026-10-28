//! The OAuth endpoints MCP clients use to sign people in (see
//! `trace_core::oauth`): discovery metadata, client registration, the
//! authorization redirect into the dashboard's Connect screen, the approval
//! that screen sends, and the token exchange.

use axum::extract::{ConnectInfo, Form, Query, State};

type Connect = Option<Extension<ConnectInfo<std::net::SocketAddr>>>;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use trace_core::{auth, oauth};

use crate::api::{ApiError, ApiResult, AppStateRef};
use crate::auth_api::Principal;

/// Client registrations per address per hour.
const MAX_REGISTRATIONS: u32 = 20;
/// Token requests per address per 10 minutes.
const MAX_TOKEN_REQUESTS: u32 = 60;

/// Public endpoints, outside `/api`. No cookies are read here, so any origin may call them.
pub fn routes() -> Router<AppStateRef> {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(resource_metadata))
        .route("/.well-known/oauth-protected-resource/mcp", get(resource_metadata))
        .route("/.well-known/oauth-authorization-server", get(server_metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .layer(tower_http::cors::CorsLayer::new().allow_origin(tower_http::cors::Any).allow_headers(tower_http::cors::Any))
}

/// The dashboard's side, for signed-in people only (`/api/account/connect`).
pub fn api_routes() -> Router<AppStateRef> {
    Router::new().route("/account/connect", get(describe).post(approve))
}

/// This server's address as clients see it: `TRACE_PUBLIC_URL`, or the request's host.
pub fn origin(state: &AppStateRef, headers: &HeaderMap) -> String {
    if let Some(url) = &state.public_url {
        return url.trim_end_matches('/').to_string();
    }
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
    let scheme = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or("http");
    format!("{scheme}://{host}")
}

/// What `/mcp` answers without a valid token, so clients can find the sign-in.
pub fn challenge(state: &AppStateRef, headers: &HeaderMap, invalid: bool) -> Response {
    let meta = format!("{}/.well-known/oauth-protected-resource", origin(state, headers));
    let value = if invalid {
        format!(r#"Bearer error="invalid_token", resource_metadata="{meta}""#)
    } else {
        format!(r#"Bearer resource_metadata="{meta}""#)
    };
    let mut res = (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "sign in: your MCP client opens Trace in the browser, or send a personal API token as `Authorization: Bearer trc_...`" })),
    )
        .into_response();
    if let Ok(v) = header::HeaderValue::from_str(&value) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

async fn resource_metadata(State(state): State<AppStateRef>, headers: HeaderMap) -> Json<Value> {
    let o = origin(&state, &headers);
    Json(json!({
        "resource": format!("{o}/mcp"),
        "authorization_servers": [o],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["trace"],
        "resource_name": "Trace",
    }))
}

async fn server_metadata(State(state): State<AppStateRef>, headers: HeaderMap) -> Json<Value> {
    let o = origin(&state, &headers);
    Json(json!({
        "issuer": o,
        "authorization_endpoint": format!("{o}/oauth/authorize"),
        "token_endpoint": format!("{o}/oauth/token"),
        "registration_endpoint": format!("{o}/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["trace"],
    }))
}

fn oauth_error(status: StatusCode, error: &str, description: impl std::fmt::Display) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(json!({ "error": error, "error_description": description.to_string() }))).into_response()
}

fn address(connect: &Connect, headers: &HeaderMap) -> String {
    crate::guard::client_ip(connect.as_ref().map(|Extension(c)| c.0), headers)
}

#[derive(Deserialize)]
struct Registration {
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
}

async fn register(
    State(state): State<AppStateRef>,
    connect: Connect,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !state.limiter.hit(&format!("oauth-register:{}", address(&connect, &headers)), MAX_REGISTRATIONS, 60 * 60 * 1000) {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "slow_down", "too many registrations from this address; try again later");
    }
    let Ok(r) = serde_json::from_slice::<Registration>(&body) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "send the registration as JSON");
    };
    match oauth::register(&state.pool, r.client_name.as_deref().unwrap_or(""), &r.redirect_uris).await {
        Ok(c) => (
            StatusCode::CREATED,
            Json(json!({
                "client_id": c.id,
                "client_name": c.name,
                "redirect_uris": c.redirect_uris,
                "token_endpoint_auth_method": "none",
                "grant_types": ["authorization_code"],
                "response_types": ["code"],
                "client_id_issued_at": trace_core::util::now_ms() / 1000,
            })),
        )
            .into_response(),
        Err(e) => oauth_error(StatusCode::BAD_REQUEST, "invalid_redirect_uri", e),
    }
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    state: Option<String>,
}

/// Adds query parameters to a client's redirect address.
fn with_params(uri: &str, params: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect::<String>()
    };
    let query = params.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&");
    format!("{uri}{}{query}", if uri.contains('?') { '&' } else { '?' })
}

/// Checks the request and hands it to the dashboard, which signs the person in
/// (with two-step verification) and asks them to approve. A bad client or
/// redirect address is shown here and never redirected to.
async fn authorize(State(state): State<AppStateRef>, uri: axum::http::Uri, Query(q): Query<AuthorizeQuery>) -> Response {
    let client = match oauth::client(&state.pool, &q.client_id).await {
        Ok(Some(c)) if c.allows(&q.redirect_uri) => c,
        Ok(_) => return (StatusCode::BAD_REQUEST, "Trace doesn't know this app or where it wants to send you back. Connect it again from the app.").into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let st = q.state.as_deref().unwrap_or("");
    if q.response_type != "code" {
        return Redirect::to(&with_params(&q.redirect_uri, &[("error", "unsupported_response_type"), ("state", st)])).into_response();
    }
    if let Err(e) = oauth::check_request(&client, &q.redirect_uri, &q.code_challenge, &q.code_challenge_method) {
        return Redirect::to(&with_params(&q.redirect_uri, &[("error", "invalid_request"), ("error_description", &e.to_string()), ("state", st)])).into_response();
    }
    Redirect::to(&format!("/#/connect?{}", uri.query().unwrap_or(""))).into_response()
}

#[derive(Deserialize)]
struct DescribeQuery {
    client_id: String,
    redirect_uri: String,
}

/// What the Connect screen shows: who is asking, and where approval sends you.
async fn describe(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Query(q): Query<DescribeQuery>) -> ApiResult<Json<Value>> {
    let s = p.session().ok_or_else(|| ApiError::bad_request("only a signed-in person can connect an app"))?;
    let client = oauth::client(&state.pool, &q.client_id)
        .await?
        .filter(|c| c.allows(&q.redirect_uri))
        .ok_or_else(|| ApiError::bad_request("this connection request isn't valid any more; start it again from your app"))?;
    let back_to = q.redirect_uri.split_once("://").map(|(_, rest)| rest.split(['/', '?']).next().unwrap_or(rest)).unwrap_or(&q.redirect_uri);
    let account = trace_core::db::accounts(&state.pool).await?.into_iter().find(|a| a.id == s.user.account_id).map(|a| a.name);
    // A token works in one account; someone in several picks which.
    let accounts = auth::memberships(&state.pool, s.user.id).await?;
    Ok(Json(json!({ "client_name": client.name, "back_to": back_to, "account": account, "account_id": s.user.account_id,
        "accounts": accounts, "role": s.user.role })))
}

#[derive(Deserialize)]
struct Approval {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    read_only: bool,
    /// Which of the person's accounts the app works in; the session's when left out.
    #[serde(default)]
    account_id: Option<i64>,
    approve: bool,
}

async fn approve(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(a): Json<Approval>) -> ApiResult<Json<Value>> {
    let s = p.session().ok_or_else(|| ApiError::bad_request("only a signed-in person can connect an app"))?;
    let client = oauth::client(&state.pool, &a.client_id)
        .await?
        .filter(|c| c.allows(&a.redirect_uri))
        .ok_or_else(|| ApiError::bad_request("this connection request isn't valid any more; start it again from your app"))?;
    let st = a.state.as_deref().unwrap_or("");
    if !a.approve {
        return Ok(Json(json!({ "redirect": with_params(&a.redirect_uri, &[("error", "access_denied"), ("state", st)]) })));
    }
    oauth::check_request(&client, &a.redirect_uri, &a.code_challenge, &a.code_challenge_method).map_err(ApiError::bad_request)?;
    let account_id = a.account_id.unwrap_or(s.user.account_id);
    let user = auth::get_user(&state.pool, account_id, s.user.id).await?.ok_or_else(|| ApiError::bad_request("you're not a member of that account"))?;
    let grant = oauth::Grant { user_id: user.id, account_id: user.account_id, read_only: a.read_only, client_name: client.name.clone() };
    let code = oauth::issue_code(&state.pool, &client, &a.redirect_uri, &a.code_challenge, &grant).await?;
    Ok(Json(json!({ "redirect": with_params(&a.redirect_uri, &[("code", &code), ("state", st)]) })))
}

#[derive(Deserialize)]
struct TokenRequest {
    #[serde(default)]
    grant_type: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    code_verifier: String,
}

async fn token(
    State(state): State<AppStateRef>,
    connect: Connect,
    headers: HeaderMap,
    Form(t): Form<TokenRequest>,
) -> Response {
    if !state.limiter.hit(&format!("oauth-token:{}", address(&connect, &headers)), MAX_TOKEN_REQUESTS, 10 * 60 * 1000) {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "slow_down", "too many token requests from this address");
    }
    if t.grant_type != "authorization_code" {
        return oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", "only authorization_code is supported");
    }
    let grant = match oauth::redeem_code(&state.pool, &t.code, &t.client_id, &t.redirect_uri, &t.code_verifier).await {
        Ok(Some(g)) => g,
        Ok(None) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "the code is unknown, used, expired or doesn't match this request"),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let issued = async {
        let user = auth::get_user(&state.pool, grant.account_id, grant.user_id).await?.ok_or_else(|| anyhow::anyhow!("that person is no longer in the account"))?;
        let access = if grant.read_only { ", read-only" } else { "" };
        let name = format!("{} (signed in {}{access})", grant.client_name, trace_core::util::date(trace_core::util::now_ms()));
        let (t, secret) = auth::create_api_token(&state.pool, grant.account_id, &user, &name, grant.read_only).await?;
        let access = if grant.read_only { "read-only".to_string() } else { format!("acts as {}", user.role) };
        auth::audit(&state.pool, grant.account_id, &user.email, "token.created", &format!("{} ({access}), by signing in from an MCP client", t.name)).await?;
        anyhow::Ok(secret)
    };
    match issued.await {
        Ok(secret) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({ "access_token": secret, "token_type": "Bearer", "scope": "trace" })),
        )
            .into_response(),
        Err(e) => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", e),
    }
}
