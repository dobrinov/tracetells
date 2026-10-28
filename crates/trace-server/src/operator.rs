//! The operator area: every account with its people, websites, plan and
//! usage, plan templates, and account creation. Access is checked in
//! `auth_api::authorize` (operator rights from the CLI, plus two-step).
//!
//! By design this module reads only account metadata, people, websites,
//! limits and usage: never behaviour data, KPIs, insights or privacy rules.

use axum::extract::{Path, State};
use axum::http::StatusCode;
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
        .map(|u| json!({ "email": u.email, "name": u.name, "role": u.role, "totp_enabled": u.totp_enabled, "last_login_at": u.last_login_at, "is_operator": u.is_operator }))
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
