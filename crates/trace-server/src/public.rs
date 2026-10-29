//! A website's public, read-only view (see `trace_core::public`): the
//! dashboard at `/p/{token}`, reading `/v1/public/{token}/...`. Only GET is
//! mounted, and every response is built from an explicit list of fields, so
//! anything added to the signed-in API later stays private by default.
//! Owners and admins publish, replace and withdraw the link at
//! `/api/sites/{site}/publish`.

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{Value, json};
use trace_core::model::{Insight, InsightStatus, Project};
use trace_core::{config, db, digest, insights, public};

use crate::api::{ApiError, ApiResult, AppStateRef};
use crate::auth_api::{CurrentSite, Principal};

/// Requests per address per minute: the views build summaries, which costs.
const MAX_REQUESTS: u32 = 120;

type Connect = Option<Extension<ConnectInfo<std::net::SocketAddr>>>;

/// Unauthenticated: the page and its data.
pub fn routes() -> Router<AppStateRef> {
    Router::new()
        // Search engines would index every published dashboard as a thin page of numbers.
        .route("/p/{token}", get(|| async { ([(header::HeaderName::from_static("x-robots-tag"), "noindex, nofollow")], Html(crate::api::INDEX_HTML)) }))
        .route("/v1/public/{token}/project", get(overview))
        .route("/v1/public/{token}/kpis", get(kpis))
        .route("/v1/public/{token}/funnels", get(funnels))
        .route("/v1/public/{token}/config", get(tracking))
        .route("/v1/public/{token}/digest", get(behaviour))
        .route("/v1/public/{token}/insights", get(list))
        .route("/v1/public/{token}/insights/{id}/prompt", get(prompt))
}

/// Signed-in and website-scoped: see whether it's public (anyone), change it (admins).
pub fn admin_routes() -> Router<AppStateRef> {
    Router::new()
        .route("/publish", get(status).post(publish).delete(withdraw))
        .route("/publish/rotate", post(rotate))
}

async fn site(state: &AppStateRef, token: &str, connect: &Connect, headers: &HeaderMap) -> ApiResult<Project> {
    let ip = crate::guard::client_ip(connect.as_ref().map(|Extension(c)| c.0), headers);
    if !state.limiter.hit(&format!("public:{ip}"), MAX_REQUESTS, 60_000) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests; try again in a minute".into()));
    }
    public::site(&state.pool, token).await?.ok_or_else(|| ApiError::not_found_msg("this public link doesn't exist or was withdrawn"))
}

async fn overview(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let c = db::collection(&state.pool, s.id).await?;
    Ok(Json(json!({
        "name": s.name,
        "public": true,
        // Not where it collects from: that's the owner's business.
        "collection": { "active": c.active, "last_event_at": c.last_event_at, "sources": [] },
        "suppressed_below": public::MIN_SESSIONS,
    })))
}

async fn kpis(
    State(state): State<AppStateRef>,
    connect: Connect,
    headers: HeaderMap,
    Path(token): Path<String>,
    q: Query<crate::api::DaysQuery>,
) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let Json(list) = crate::api::list_kpis(State(state), Extension(CurrentSite(s)), q).await?;
    let keep = ["id", "key", "name", "description", "direction", "target", "summary", "current", "previous", "sessions", "target_met", "definition"];
    Ok(Json(Value::Array(list.as_array().into_iter().flatten().map(|k| pick(k, &keep)).collect())))
}

/// Funnels, without any built from fewer than 5 sessions, like every public row.
async fn funnels(
    State(state): State<AppStateRef>,
    connect: Connect,
    headers: HeaderMap,
    Path(token): Path<String>,
    q: Query<crate::api::DaysQuery>,
) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let mut list = trace_core::funnel::report(&state.pool, s.id, q.days.clamp(1, 90)).await?;
    trace_core::public::suppress_funnels(&mut list);
    Ok(Json(serde_json::json!(list)))
}

/// Declared events and whether they arrive; events sent without being
/// declared are left out, since anyone can send any name.
async fn tracking(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let h = config::tracking_health(&state.pool, s.id, 14).await?;
    Ok(Json(json!({
        "collecting_since": h.collecting_since,
        "synced_at": h.synced_at,
        "source": null,
        "events": h.events.iter().map(|e| json!({ "name": e.name, "description": e.description, "count": e.count, "sessions": e.sessions, "last_seen": e.last_seen, "status": e.status })).collect::<Vec<_>>(),
        "undeclared_events": [],
    })))
}

async fn behaviour(
    State(state): State<AppStateRef>,
    connect: Connect,
    headers: HeaderMap,
    Path(token): Path<String>,
    Query(q): Query<crate::api::DaysQuery>,
) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let d = public::suppress(digest::build(&state.pool, s.id, q.days.clamp(1, 365)).await?);
    let mut v = serde_json::to_value(d).map_err(anyhow::Error::from)?;
    v["suppressed_below"] = json!(public::MIN_SESSIONS);
    Ok(Json(v))
}

/// Insights without dismissed ones (and so without anyone's dismissal reasons).
fn shown(i: &Insight) -> bool {
    i.status != InsightStatus::Dismissed
}

fn insight_json(i: &Insight) -> Value {
    json!({
        "id": i.id, "title": i.title, "problem": i.problem, "evidence": i.evidence, "proposal": i.proposal,
        "acceptance_criteria": i.acceptance_criteria, "pages": i.pages, "kpi_id": i.kpi_id,
        "expected_impact": i.expected_impact, "confidence": i.confidence, "status": i.status, "source": i.source,
        "created_at": i.created_at, "released_at": i.released_at,
        // Values from fewer than 5 sessions are left out, as everywhere in the public view.
        "readings": i.readings.iter().map(|r| {
            let shown = r.sessions >= public::MIN_SESSIONS;
            json!({ "item": r.item, "kind": r.kind, "value": if shown { r.value } else { None },
                    "formatted": if shown { r.formatted.clone() } else { None }, "sessions": r.sessions,
                    "from": r.from, "to": r.to, "at": r.at })
        }).collect::<Vec<_>>(),
        "item_verdicts": i.item_verdicts.iter().map(|v| json!({ "item": v.item, "verdict": v.verdict, "summary": v.summary })).collect::<Vec<_>>(),
        "evaluation": i.evaluation.as_ref().map(|e| json!({
            "verdict": e.verdict, "summary": e.summary, "baseline": e.baseline, "current": e.current,
            "p_value": e.p_value, "window_days": e.window_days, "settled_at": e.settled_at,
        })),
    })
}

async fn list(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let s = site(&state, &token, &connect, &headers).await?;
    let all = insights::list(&state.pool, s.id, None).await?;
    Ok(Json(Value::Array(all.iter().filter(|i| shown(i)).map(insight_json).collect())))
}

async fn prompt(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Path((token, id)): Path<(String, i64)>) -> ApiResult<Response> {
    let s = site(&state, &token, &connect, &headers).await?;
    let insight = insights::get(&state.pool, s.id, id).await?.filter(shown).ok_or_else(|| ApiError::not_found("insight"))?;
    let text = crate::render_prompt(&state.pool, &s, &insight).await?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], text).into_response())
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|k| v.get(*k).map(|x| (k.to_string(), x.clone()))).collect())
}

// ---- publishing -------------------------------------------------------------

fn link_json(state: &AppStateRef, headers: &HeaderMap, l: Option<public::PublicLink>) -> Value {
    let origin = crate::oauth::origin(state, headers);
    match l {
        Some(l) => json!({ "public": true, "url": format!("{origin}/p/{}", l.token), "since": l.since }),
        None => json!({ "public": false }),
    }
}

async fn status(State(state): State<AppStateRef>, headers: HeaderMap, Extension(CurrentSite(s)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    Ok(Json(link_json(&state, &headers, public::link(&state.pool, s.id).await?)))
}

async fn publish(
    State(state): State<AppStateRef>,
    headers: HeaderMap,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let was = public::link(&state.pool, s.id).await?;
    let l = public::publish(&state.pool, s.id, &who.actor()).await?;
    if was.is_none() {
        trace_core::auth::audit_site(&state.pool, s.account_id, Some(s.id), &who.actor(), "site.public_on", &s.name).await?;
    }
    Ok(Json(link_json(&state, &headers, Some(l))))
}

async fn rotate(
    State(state): State<AppStateRef>,
    headers: HeaderMap,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let l = public::replace(&state.pool, s.id, &who.actor()).await?;
    trace_core::auth::audit_site(&state.pool, s.account_id, Some(s.id), &who.actor(), "site.public_rotated", &s.name).await?;
    Ok(Json(link_json(&state, &headers, Some(l))))
}

async fn withdraw(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(s)): Extension<CurrentSite>,
    Extension(who): Extension<Principal>,
) -> ApiResult<StatusCode> {
    public::withdraw(&state.pool, s.id).await?;
    trace_core::auth::audit_site(&state.pool, s.account_id, Some(s.id), &who.actor(), "site.public_off", &s.name).await?;
    Ok(StatusCode::NO_CONTENT)
}
