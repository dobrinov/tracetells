//! Server errors over HTTP: `POST /v1/errors`, where a website's servers send
//! them with its secret server key, and the website endpoints that read them
//! and manage that key (see `trace_core::errors`).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use trace_core::errors;

use crate::api::{ApiError, ApiResult, AppStateRef};
use crate::auth_api::{CurrentSite, Principal};

/// Requests one server key may make per minute: a busy fleet flushing once a second each.
const BATCHES_PER_KEY: u32 = 1200;

pub fn ingest_routes() -> Router<AppStateRef> {
    Router::new().route("/v1/errors", post(ingest))
}

/// Website-scoped, under `/api/sites/{site}`. Reading is for everyone in the
/// account; changing a group or the key needs an admin (`auth_api::required_role`).
pub fn site_routes() -> Router<AppStateRef> {
    Router::new()
        .route("/errors", get(list))
        .route("/errors/{id}", get(detail))
        .route("/errors/{id}/prompt", get(prompt))
        .route("/errors/{id}/status", post(set_status))
        .route("/server-key", get(key_status).post(create_key).delete(revoke_key))
}

async fn ingest(State(state): State<AppStateRef>, headers: HeaderMap, body: Bytes) -> ApiResult<(StatusCode, Json<Value>)> {
    let key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    let Some((site, account)) = errors::site_for_key(&state.pool, key).await? else {
        return Err(ApiError::unauthorized("send errors with the website's server key: Authorization: Bearer tse_…"));
    };
    if !state.collect_limits.hit(&format!("errors:{site}"), BATCHES_PER_KEY, 60_000) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests for this website; back off and retry".into()));
    }
    let rules = crate::api::site_rules(&state, site).await?;
    let (batch, skipped) = errors::parse_batch(&body, &rules, trace_core::util::now_ms()).map_err(ApiError::bad_request)?;
    // Over the monthly limit, the rest is counted as skipped and nothing more
    // is stored; the answer stays a success so servers never retry it.
    let wanted: i64 = batch.iter().map(|e| e.count).sum();
    let mut allowed = crate::api::take_events(&state, account, wanted).await?;
    let granted = allowed;
    let mut keep = Vec::with_capacity(batch.len());
    for mut e in batch {
        if allowed == 0 {
            break;
        }
        e.count = e.count.min(allowed);
        allowed -= e.count;
        keep.push(e);
    }
    let accepted = keep.len();
    let stored = match errors::store(&state.pool, site, &keep).await {
        Ok(n) => n,
        Err(e) => {
            crate::api::give_back_events(&state, account, granted);
            return Err(e.into());
        }
    };
    trace_core::usage::add_events(&state.pool, account, stored, wanted - stored).await?;
    crate::api::forget_site(&state, site);
    Ok((StatusCode::ACCEPTED, Json(json!({ "accepted": accepted, "skipped": skipped, "over_limit": wanted - stored }))))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_days")]
    days: u32,
    environment: Option<String>,
    #[serde(default)]
    ignored: bool,
}

fn default_days() -> u32 {
    14
}

async fn list(State(state): State<AppStateRef>, Extension(CurrentSite(s)): Extension<CurrentSite>, Query(q): Query<ListQuery>) -> ApiResult<Json<errors::Report>> {
    Ok(Json(errors::report(&state.pool, s.id, q.days, q.environment.as_deref(), q.ignored).await?))
}

async fn detail(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Path(id): Path<i64>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<errors::Detail>> {
    let d = errors::detail(&state.pool, s.id, id, q.days, q.environment.as_deref()).await?.ok_or_else(|| ApiError::not_found("server error"))?;
    Ok(Json(d))
}

async fn prompt(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Path(id): Path<i64>,
) -> ApiResult<([(header::HeaderName, &'static str); 1], String)> {
    let d = errors::detail(&state.pool, s.id, id, 14, None).await?.ok_or_else(|| ApiError::not_found("server error"))?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], errors::fix_prompt(&s.name, &d)))
}

#[derive(Deserialize)]
struct StatusBody {
    status: String,
    release: Option<String>,
}

async fn set_status(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
    Path(id): Path<i64>,
    Json(b): Json<StatusBody>,
) -> ApiResult<StatusCode> {
    let changed = errors::set_status(&state.pool, s.account_id, s.id, id, &b.status, b.release.as_deref(), &who.actor()).await.map_err(ApiError::bad_request)?;
    if changed { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found("server error")) }
}

async fn key_status(State(state): State<AppStateRef>, Extension(CurrentSite(s)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "key": errors::key_status(&state.pool, s.id).await? })))
}

async fn create_key(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let (info, secret) = errors::create_key(&state.pool, s.account_id, s.id, &who.actor()).await?;
    Ok(Json(json!({ "key": info, "secret": secret, "note": "Shown only now. Set it as TRACE_SERVER_KEY on your servers; creating another revokes this one." })))
}

async fn revoke_key(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
) -> ApiResult<StatusCode> {
    if errors::revoke_key(&state.pool, s.account_id, s.id, &who.actor()).await? { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found("server key")) }
}
