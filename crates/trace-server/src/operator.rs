//! The operator area: every account with its people, websites, plan and
//! usage, plan templates, and account creation. Access is checked in
//! `auth_api::authorize` (operator rights from the CLI, plus two-step).
//!
//! By design this module reads only account metadata, people, websites,
//! limits and usage: never behaviour data, KPIs, insights or privacy rules.
//! Seeing a customer's analytics means viewing as one of its people
//! (`/operator/impersonate`): read-only, for an hour at most, and recorded in
//! that customer's own activity log.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use trace_core::auth::{self, Role};
use trace_core::plans::{self, Limits, Overrides};
use trace_core::{db, usage};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppStateRef};
use crate::auth_api::Principal;

pub fn routes() -> Router<AppStateRef> {
    Router::new()
        .route("/operator/overview", get(overview))
        .route("/operator/stats", get(stats))
        .route("/operator/people", get(people))
        .route("/operator/sites", get(sites))
        .route("/operator/impersonate", post(impersonate))
        .route("/operator/accounts", post(create_account))
        .route("/operator/accounts/{id}", get(account_detail))
        .route("/operator/accounts/{id}/plan", post(set_plan))
        .route("/operator/accounts/{id}/overrides", post(set_overrides))
        .route("/operator/plans", post(create_plan))
        .route("/operator/plans/{key}", axum::routing::put(update_plan))
        .route("/operator/plans/default", post(set_default_plan))
}

fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(e)
}

/// Operator actions go into the operator's own account log, so customers'
/// logs never show plan names.
async fn record(state: &AppStateRef, p: &Principal, action: &str, detail: &str) -> ApiResult<()> {
    auth::audit(&state.pool, p.account(), &p.actor(), action, detail).await?;
    Ok(())
}

async fn account_summary(state: &AppStateRef, account: &db::Account) -> ApiResult<Value> {
    let plan = plans::for_account(&state.pool, account.id).await?;
    let u = usage::get(&state.pool, account.id, &usage::this_month()).await?;
    Ok(json!({
        "id": account.id,
        "name": account.name,
        "created_at": account.created_at,
        "plan": plan.plan.key,
        "overrides": plan.overrides,
        "limits": plan.effective,
        "usage": u,
        "members": auth::user_count(&state.pool, account.id).await?,
        "sites": db::sites(&state.pool, account.id).await?.len(),
    }))
}

const DAY: i64 = trace_core::util::DAY_MS;

/// The whole server at a glance: how many accounts, people and websites, how
/// much they're used, and sessions per day over the last two weeks.
async fn stats(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let pool = &state.pool;
    let now = trace_core::util::now_ms();
    let scalar = |sql: &'static str, at: i64| async move {
        let n: i64 = sqlx::query_scalar(sql).bind(at).fetch_one(pool).await?;
        anyhow::Ok(n)
    };
    let month = usage::this_month();
    let (events, dropped): (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT CAST(SUM(events) AS BIGINT), CAST(SUM(events_dropped) AS BIGINT) FROM usage WHERE month = $1").bind(&month).fetch_one(pool).await?;
    let days: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT started_at / 86400000, COUNT(*) FROM sessions WHERE started_at >= $1 GROUP BY started_at / 86400000 ORDER BY 1",
    )
    .bind(now - 14 * DAY)
    .fetch_all(pool)
    .await?;
    Ok(Json(json!({
        "accounts": scalar("SELECT COUNT(*) FROM accounts WHERE created_at < $1", now + 1).await?,
        "accounts_new_7d": scalar("SELECT COUNT(*) FROM accounts WHERE created_at >= $1", now - 7 * DAY).await?,
        "people": scalar("SELECT COUNT(*) FROM users WHERE created_at < $1", now + 1).await?,
        "people_new_7d": scalar("SELECT COUNT(*) FROM users WHERE created_at >= $1", now - 7 * DAY).await?,
        "people_active_7d": scalar("SELECT COUNT(*) FROM users WHERE last_login_at >= $1", now - 7 * DAY).await?,
        "websites": scalar("SELECT COUNT(*) FROM projects WHERE created_at < $1", now + 1).await?,
        "websites_collecting": scalar("SELECT COUNT(*) FROM projects WHERE last_event_at >= $1", now - DAY).await?,
        "sessions_7d": scalar("SELECT COUNT(*) FROM sessions WHERE started_at >= $1", now - 7 * DAY).await?,
        "events_month": events.unwrap_or(0),
        "events_dropped_month": dropped.unwrap_or(0),
        "month": month,
        "sessions_by_day": days.into_iter().map(|(d, n)| json!({ "day": trace_core::util::date(d * DAY), "sessions": n })).collect::<Vec<_>>(),
    })))
}

/// Everyone on the server, with the accounts they belong to.
async fn people(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    type Row = (i64, String, String, db::Flag, Option<i64>, i64, Option<i64>, i64, String, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, u.is_operator, u.totp_enabled_at, u.created_at, u.last_login_at, a.id, a.name, m.role
         FROM users u JOIN memberships m ON m.user_id = u.id JOIN accounts a ON a.id = m.account_id ORDER BY u.email, a.name",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut out: Vec<Value> = Vec::new();
    for (id, email, name, op, totp, created, last, account_id, account, role) in rows {
        let membership = json!({ "account_id": account_id, "account": account, "role": role });
        match out.last_mut().filter(|p| p["id"] == json!(id)) {
            Some(p) => p["accounts"].as_array_mut().expect("accounts is a list").push(membership),
            None => out.push(json!({
                "id": id, "email": email, "name": name, "is_operator": op.0, "totp_enabled": totp.is_some(),
                "created_at": created, "last_login_at": last, "accounts": [membership],
            })),
        }
    }
    Ok(Json(json!(out)))
}

/// Every website on the server, with its account and how much it's used.
async fn sites(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let now = trace_core::util::now_ms();
    type Row = (i64, String, Option<String>, i64, String, Option<i64>, i64, i64, i64);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT p.id, p.name, p.slug, a.id, a.name, p.last_event_at, p.created_at,
           (SELECT COUNT(*) FROM sessions s WHERE s.project_id = p.id AND s.started_at >= $1),
           (SELECT COUNT(*) FROM events e WHERE e.project_id = p.id AND e.ts >= $2)
         FROM projects p JOIN accounts a ON a.id = p.account_id ORDER BY a.name, p.name",
    )
    .bind(now - 7 * DAY)
    .bind(now - 30 * DAY)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, name, slug, account_id, account, last, created, sessions_7d, events_30d)| json!({
            "id": id, "name": name, "slug": slug, "account_id": account_id, "account": account,
            "collecting": last.is_some_and(|t| now - t < DAY), "last_event_at": last, "created_at": created,
            "sessions_7d": sessions_7d, "events_30d": events_30d,
        }))
        .collect::<Vec<_>>())))
}

#[derive(Deserialize)]
struct ImpersonateBody {
    user_id: i64,
    #[serde(default)]
    account_id: Option<i64>,
}

/// Views the dashboard as someone else, read-only. Recorded in the operator's
/// own log and, so the customer can see it, in the account being viewed.
async fn impersonate(
    State(state): State<AppStateRef>,
    Extension(p): Extension<Principal>,
    headers: HeaderMap,
    Json(b): Json<ImpersonateBody>,
) -> ApiResult<Response> {
    let s = p.session().ok_or_else(|| ApiError::forbidden("sign in as a person to do this"))?;
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    let (token, target) = auth::impersonate(&state.pool, &s.user, b.user_id, b.account_id, ua).await.map_err(bad)?;
    let account = db::account(&state.pool, target.account_id).await?.name;
    record(&state, &p, "operator.impersonated", &format!("{} in {account} (read-only)", target.email)).await?;
    auth::audit(&state.pool, target.account_id, &s.user.email, "operator.viewed_as", &format!("viewed the dashboard as {} (read-only)", target.email)).await?;
    // The operator's own session ends; stopping signs them in afresh.
    auth::end_user_sessions(&state.pool, s.user.id, None).await?;
    let viewing = auth::session(&state.pool, &token).await?.ok_or_else(|| bad("that session didn't start"))?;
    let mut res = Json(crate::auth_api::state_json(&state, Some(&viewing)).await?).into_response();
    crate::auth_api::set_cookie(&mut res, Some(&token), state.secure_cookies);
    Ok(res)
}

async fn overview(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let mut accounts = Vec::new();
    for a in db::accounts(&state.pool).await? {
        accounts.push(account_summary(&state, &a).await?);
    }
    let database = if db::is_postgres(&state.pool) { "postgres" } else { "sqlite" };
    Ok(Json(json!({ "plans": plans::list(&state.pool).await?, "accounts": accounts, "limits": plans::LIMITS, "database": database })))
}

async fn account_detail(State(state): State<AppStateRef>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let account = db::account(&state.pool, id).await.map_err(|_| ApiError::not_found_msg("no such account"))?;
    let people: Vec<Value> = auth::list_users(&state.pool, id)
        .await?
        .into_iter()
        .map(|u| json!({ "id": u.id, "email": u.email, "name": u.name, "role": u.role, "totp_enabled": u.totp_enabled, "last_login_at": u.last_login_at, "is_operator": u.is_operator }))
        .collect();
    let mut sites = Vec::new();
    for s in db::sites(&state.pool, id).await? {
        let c = db::collection(&state.pool, s.id).await?;
        sites.push(json!({
            "name": s.name,
            "slug": s.slug,
            "collecting": c.active,
            "last_event_at": c.last_event_at,
            "origins": c.sources.iter().map(|o| &o.origin).collect::<Vec<_>>(),
        }));
    }
    let invites = auth::pending_invites(&state.pool, id).await?;
    Ok(Json(json!({ "account": account_summary(&state, &account).await?, "people": people, "sites": sites, "invites": invites })))
}

#[derive(Deserialize)]
struct NewAccount {
    name: String,
    plan: String,
    owner_email: String,
}

/// An account on a chosen plan, with a first website and a single-use invite
/// link that makes its future owner the owner.
/// Someone who already has a login becomes the owner by signing in and joining.
async fn create_account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<NewAccount>) -> ApiResult<Json<Value>> {
    let (account, _) = db::create_account_with_site(&state.pool, &b.name, Some(&b.plan)).await.map_err(bad)?;
    let (invite, token) = auth::create_invite(&state.pool, account.id, &b.owner_email, Role::Owner, None).await.map_err(bad)?;
    record(&state, &p, "operator.account_created", &format!("{} on {} for {}", account.name, b.plan, invite.email)).await?;
    let has_login = auth::has_login(&state.pool, &invite.email).await?;
    Ok(Json(json!({ "account_id": account.id, "invite_token": token, "owner_email": invite.email, "has_login": has_login })))
}

#[derive(Deserialize)]
struct PlanBody {
    plan: String,
}

async fn set_plan(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>, Json(b): Json<PlanBody>) -> ApiResult<StatusCode> {
    let account = db::account(&state.pool, id).await.map_err(|_| ApiError::not_found_msg("no such account"))?;
    plans::set_account_plan(&state.pool, id, &b.plan).await.map_err(bad)?;
    state.quotas.lock().unwrap().remove(&id);
    record(&state, &p, "operator.plan_changed", &format!("{} → {}", account.name, b.plan)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct OverridesBody {
    /// Limits this account overrides; a key with null means "no limit".
    overrides: Overrides,
}

async fn set_overrides(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>, Json(b): Json<OverridesBody>) -> ApiResult<StatusCode> {
    let account = db::account(&state.pool, id).await.map_err(|_| ApiError::not_found_msg("no such account"))?;
    plans::set_overrides(&state.pool, id, &b.overrides).await.map_err(bad)?;
    state.quotas.lock().unwrap().remove(&id);
    record(&state, &p, "operator.overrides_changed", &format!("{}: {}", account.name, serde_json::to_string(&b.overrides).unwrap_or_default())).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TemplateBody {
    #[serde(default)]
    key: String,
    name: String,
    limits: Limits,
}

async fn create_plan(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<TemplateBody>) -> ApiResult<Json<Value>> {
    let plan = plans::create(&state.pool, &b.key, &b.name, &b.limits).await.map_err(bad)?;
    record(&state, &p, "operator.plan_created", &plan.key).await?;
    Ok(Json(json!(plan)))
}

async fn update_plan(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(key): Path<String>, Json(b): Json<TemplateBody>) -> ApiResult<Json<Value>> {
    let plan = plans::update(&state.pool, &key, &b.name, &b.limits).await.map_err(bad)?;
    state.quotas.lock().unwrap().clear();
    record(&state, &p, "operator.plan_updated", &format!("{key}: {}", serde_json::to_string(&b.limits).unwrap_or_default())).await?;
    Ok(Json(json!(plan)))
}

#[derive(Deserialize)]
struct DefaultBody {
    key: String,
}

async fn set_default_plan(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<DefaultBody>) -> ApiResult<StatusCode> {
    plans::set_default(&state.pool, &b.key).await.map_err(bad)?;
    record(&state, &p, "operator.default_plan", &b.key).await?;
    Ok(StatusCode::NO_CONTENT)
}
