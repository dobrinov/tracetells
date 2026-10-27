//! Sign-in, two-step verification, invites, team management and API tokens,
//! plus the middleware that decides who may call which dashboard endpoint.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use diagnostic_core::auth::{self, Role, Session, User};
use diagnostic_core::util::now_ms;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppStateRef};

pub const COOKIE: &str = "dg_session";
const MAX_FAILED_LOGINS: u32 = 10;
const LOCKOUT_MS: i64 = 15 * 60 * 1000;

/// Failed sign-in attempts per email, to slow down password guessing.
#[derive(Default)]
pub struct LoginLimiter(Mutex<HashMap<String, (u32, i64)>>);

impl LoginLimiter {
    fn locked(&self, key: &str) -> bool {
        let map = self.0.lock().unwrap();
        map.get(key).is_some_and(|(n, since)| *n >= MAX_FAILED_LOGINS && now_ms() - since < LOCKOUT_MS)
    }
    fn failed(&self, key: &str) {
        let mut map = self.0.lock().unwrap();
        let now = now_ms();
        let e = map.entry(key.to_string()).or_insert((0, now));
        if now - e.1 >= LOCKOUT_MS {
            *e = (0, now);
        }
        e.0 += 1;
    }
    fn succeeded(&self, key: &str) {
        self.0.lock().unwrap().remove(key);
    }
}

/// Who is calling a dashboard endpoint.
#[derive(Clone)]
pub enum Principal {
    User(Session),
    /// A person's API token: acts with their current role, or as a viewer if read-only.
    Token(auth::TokenAuth),
    /// The deprecated shared DIAGNOSTIC_ADMIN_TOKEN, which belongs to nobody.
    LegacyToken,
}

impl Principal {
    pub fn role(&self) -> Role {
        match self {
            Self::User(s) => s.user.role,
            Self::Token(t) => t.role(),
            Self::LegacyToken => Role::Admin,
        }
    }
    pub fn actor(&self) -> String {
        match self {
            Self::User(s) => s.user.email.clone(),
            Self::Token(t) => format!("{} via token \"{}\"", t.user.email, t.name),
            Self::LegacyToken => "DIAGNOSTIC_ADMIN_TOKEN".into(),
        }
    }
    fn user(&self) -> Option<&Session> {
        match self {
            Self::User(s) => Some(s),
            _ => None,
        }
    }
}

/// Resolves a bearer token: a person's API token, or the legacy shared one.
pub async fn token_principal(state: &AppStateRef, token: &str) -> Option<Principal> {
    if state.admin_token.as_deref().is_some_and(|t| auth::constant_time_eq(t, token)) {
        return Some(Principal::LegacyToken);
    }
    auth::verify_api_token(&state.pool, state.project_id, token).await.ok().flatten().map(Principal::Token)
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn set_cookie(res: &mut Response, token: Option<&str>, secure: bool) {
    let value = match token {
        Some(t) => format!("{COOKIE}={t}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}", auth::SESSION_DAYS * 86400),
        None => format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    };
    let value = if secure { format!("{value}; Secure") } else { value };
    res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&value).expect("cookie is ASCII"));
}

/// The least role an endpoint needs. Anything not listed that changes state
/// needs an admin, so a forgotten entry fails closed.
fn required_role(method: &Method, path: &str) -> Role {
    let read = matches!(*method, Method::GET | Method::HEAD);
    let member_actions = path == "/analyze"
        || path == "/evaluate"
        || (path.starts_with("/ideas/") && (path.ends_with("/status") || path.ends_with("/kpi")));
    match () {
        _ if path.starts_with("/team") || path.starts_with("/audit") => Role::Admin,
        _ if path.starts_with("/account") => Role::Viewer,
        _ if read => Role::Viewer,
        _ if member_actions => Role::Member,
        _ => Role::Admin,
    }
}

/// Endpoints only people (not tokens) may use.
fn people_only(path: &str) -> bool {
    path.starts_with("/team") || path.starts_with("/account") || path.starts_with("/audit")
}

/// What a signed-in person still has to do before using the dashboard.
fn pending_step(s: &Session, require_mfa: bool) -> Option<&'static str> {
    if !s.mfa_ok {
        Some("verify")
    } else if require_mfa && !s.user.totp_enabled {
        Some("enroll")
    } else {
        None
    }
}

fn deny(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": message, "code": code }))).into_response()
}

pub async fn authorize(State(state): State<AppStateRef>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/auth/") {
        return next.run(req).await; // sign-in endpoints check credentials themselves
    }
    let headers = req.headers();
    let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let principal = if let Some(token) = bearer {
        match token_principal(&state, token).await {
            Some(p) => p,
            None => return deny(StatusCode::UNAUTHORIZED, "bad_token", "invalid or revoked API token"),
        }
    } else {
        let Some(cookie) = session_cookie(headers) else {
            return deny(StatusCode::UNAUTHORIZED, "signed_out", "sign in to continue");
        };
        match auth::session(&state.pool, state.project_id, &cookie).await {
            Ok(Some(s)) => {
                // Cookie-authenticated writes must be JSON: browsers can't send that cross-site without CORS.
                let json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
                if !matches!(*req.method(), Method::GET | Method::HEAD) && !json {
                    return deny(StatusCode::FORBIDDEN, "csrf", "requests must be sent as JSON");
                }
                let require_mfa = auth::require_mfa(&state.pool, state.project_id).await.unwrap_or(true);
                let enrolling = path.starts_with("/account/mfa/") || path == "/account";
                if let Some(step) = pending_step(&s, require_mfa)
                    && !(step == "enroll" && enrolling)
                {
                    return deny(StatusCode::UNAUTHORIZED, step, "finish two-step verification to continue");
                }
                Principal::User(s)
            }
            _ => return deny(StatusCode::UNAUTHORIZED, "signed_out", "your session has ended; sign in again"),
        }
    };
    if principal.user().is_none() && people_only(&path) {
        return deny(StatusCode::FORBIDDEN, "people_only", "API tokens can't manage people, tokens or accounts");
    }
    let needed = required_role(req.method(), &path);
    if principal.role() < needed {
        return deny(StatusCode::FORBIDDEN, "role", &format!("this needs the {needed} role"));
    }
    req.extensions_mut().insert(principal);
    next.run(req).await
}

pub fn routes() -> Router<AppStateRef> {
    Router::new()
        .route("/auth/state", get(auth_state))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/mfa", post(verify_mfa))
        .route("/auth/logout", post(logout))
        .route("/auth/invite/{token}", get(invite_info).post(accept_invite))
        .route("/account", get(account))
        .route("/account/password", post(change_password))
        .route("/account/mfa/start", post(mfa_start))
        .route("/account/mfa/confirm", post(mfa_confirm))
        .route("/account/mfa/disable", post(mfa_disable))
        .route("/account/recovery-codes", post(new_recovery_codes))
        .route("/team", get(team))
        .route("/team/invites", post(invite))
        .route("/team/invites/{id}", delete(revoke_invite))
        .route("/team/users/{id}/role", post(change_role))
        .route("/team/users/{id}", delete(remove_user))
        .route("/team/users/{id}/reset-mfa", post(reset_mfa))
        .route("/team/settings", post(team_settings))
        .route("/account/tokens", get(my_tokens).post(create_token))
        .route("/account/tokens/{id}", delete(revoke_my_token))
        .route("/team/tokens", get(all_tokens))
        .route("/team/tokens/{id}", delete(revoke_any_token))
        .route("/audit", get(audit_log))
}

fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(e)
}

async fn audit(state: &AppStateRef, actor: &str, action: &str, detail: &str) {
    if let Err(e) = auth::audit(&state.pool, state.project_id, actor, action, detail).await {
        tracing::error!(error = %e, "writing audit log");
    }
}

async fn current_session(state: &AppStateRef, headers: &HeaderMap) -> ApiResult<Option<Session>> {
    let Some(cookie) = session_cookie(headers) else { return Ok(None) };
    Ok(auth::session(&state.pool, state.project_id, &cookie).await?)
}

async fn state_json(state: &AppStateRef, s: Option<&Session>) -> ApiResult<Value> {
    let require_mfa = auth::require_mfa(&state.pool, state.project_id).await?;
    Ok(json!({
        "needs_setup": auth::user_count(&state.pool, state.project_id).await? == 0,
        "user": s.map(|s| &s.user),
        "step": s.and_then(|s| pending_step(s, require_mfa)),
        "require_mfa": require_mfa,
    }))
}

async fn auth_state(State(state): State<AppStateRef>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let s = current_session(&state, &headers).await?;
    Ok(Json(state_json(&state, s.as_ref()).await?))
}

async fn signed_in(state: &AppStateRef, user: User, mfa_ok: bool, headers: &HeaderMap) -> ApiResult<Response> {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    let token = auth::create_session(&state.pool, user.id, mfa_ok, ua).await?;
    let s = auth::session(&state.pool, state.project_id, &token).await?.ok_or_else(|| bad("session was just created"))?;
    let mut res = Json(state_json(state, Some(&s)).await?).into_response();
    set_cookie(&mut res, Some(&token), state.secure_cookies);
    Ok(res)
}

#[derive(Deserialize)]
struct SetupBody {
    code: String,
    email: String,
    #[serde(default)]
    name: String,
    password: String,
}

/// Creates the first (owner) account. Needs the one-time code the server
/// printed at startup, so whoever reaches a fresh server first can't claim it.
async fn setup(State(state): State<AppStateRef>, headers: HeaderMap, Json(b): Json<SetupBody>) -> ApiResult<Response> {
    if auth::user_count(&state.pool, state.project_id).await? > 0 {
        return Err(ApiError::conflict("this workspace already has an owner; sign in instead"));
    }
    let valid = state.setup_code.lock().unwrap().as_deref().is_some_and(|c| auth::constant_time_eq(c, b.code.trim()));
    if !valid {
        return Err(ApiError::forbidden("that setup link isn't valid; use the one the server printed when it started"));
    }
    let user = auth::create_user(&state.pool, state.project_id, &b.email, &b.name, &b.password, Role::Owner).await.map_err(bad)?;
    *state.setup_code.lock().unwrap() = None;
    audit(&state, &user.email, "workspace.created", "owner account created").await;
    signed_in(&state, user, true, &headers).await
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

async fn login(State(state): State<AppStateRef>, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<Response> {
    let key = b.email.trim().to_lowercase();
    if state.limiter.locked(&key) {
        return Err(ApiError::too_many("too many failed attempts; try again in 15 minutes"));
    }
    match auth::authenticate(&state.pool, state.project_id, &b.email, &b.password).await? {
        Some(user) => {
            state.limiter.succeeded(&key);
            let needs_code = user.totp_enabled;
            signed_in(&state, user, !needs_code, &headers).await
        }
        None => {
            state.limiter.failed(&key);
            Err(ApiError::unauthorized("wrong email or password"))
        }
    }
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

async fn verify_mfa(State(state): State<AppStateRef>, headers: HeaderMap, Json(b): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let s = current_session(&state, &headers).await?.ok_or_else(|| ApiError::unauthorized("sign in first"))?;
    let key = format!("mfa:{}", s.user.id);
    if state.limiter.locked(&key) {
        return Err(ApiError::too_many("too many wrong codes; try again in 15 minutes"));
    }
    match auth::verify_second_factor(&state.pool, s.user.id, &b.code).await? {
        Some(kind) => {
            state.limiter.succeeded(&key);
            auth::mark_mfa_passed(&state.pool, &s).await?;
            let s = Session { mfa_ok: true, ..s };
            let mut out = state_json(&state, Some(&s)).await?;
            if kind == auth::SecondFactor::RecoveryCode {
                out["recovery_codes_left"] = json!(auth::recovery_codes_left(&state.pool, s.user.id).await?);
                audit(&state, &s.user.email, "mfa.recovery_code_used", "").await;
            }
            Ok(Json(out))
        }
        None => {
            state.limiter.failed(&key);
            Err(ApiError::unauthorized("that code didn't work; use the newest code from your app, or a recovery code"))
        }
    }
}

async fn logout(State(state): State<AppStateRef>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(c) = session_cookie(&headers) {
        auth::end_session(&state.pool, &c).await?;
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    set_cookie(&mut res, None, state.secure_cookies);
    Ok(res)
}

async fn invite_info(State(state): State<AppStateRef>, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let invite = auth::find_invite(&state.pool, state.project_id, &token)
        .await?
        .ok_or_else(|| ApiError::not_found_msg("this invite link has expired or was already used"))?;
    let workspace = diagnostic_core::db::project(&state.pool, state.project_id).await?.name;
    Ok(Json(json!({ "email": invite.email, "role": invite.role, "workspace": workspace,
        "require_mfa": auth::require_mfa(&state.pool, state.project_id).await? })))
}

#[derive(Deserialize)]
struct AcceptBody {
    #[serde(default)]
    name: String,
    password: String,
}

async fn accept_invite(State(state): State<AppStateRef>, Path(token): Path<String>, headers: HeaderMap, Json(b): Json<AcceptBody>) -> ApiResult<Response> {
    let user = auth::accept_invite(&state.pool, state.project_id, &token, &b.name, &b.password).await.map_err(bad)?;
    audit(&state, &user.email, "member.joined", &format!("as {}", user.role)).await;
    signed_in(&state, user, true, &headers).await
}

fn me(p: &Principal) -> ApiResult<&Session> {
    p.user().ok_or_else(|| ApiError::forbidden("sign in as a person to do this"))
}

async fn account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    Ok(Json(json!({
        "user": s.user,
        "recovery_codes_left": auth::recovery_codes_left(&state.pool, s.user.id).await?,
        "require_mfa": auth::require_mfa(&state.pool, state.project_id).await?,
    })))
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

async fn change_password(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<PasswordBody>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if !auth::password_matches(&state.pool, s.user.id, &b.current).await? {
        return Err(ApiError::unauthorized("your current password is wrong"));
    }
    auth::set_password(&state.pool, s.user.id, &b.new).await.map_err(bad)?;
    auth::end_user_sessions(&state.pool, s.user.id, Some(&s.token_hash)).await?;
    audit(&state, &s.user.email, "account.password_changed", "other sessions signed out").await;
    Ok(StatusCode::NO_CONTENT)
}

async fn mfa_start(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    if s.user.totp_enabled {
        return Err(bad("two-step verification is already on; turn it off first to move to a new device"));
    }
    let workspace = diagnostic_core::db::project(&state.pool, state.project_id).await?.name;
    let e = auth::begin_totp(&state.pool, &s.user, &format!("Diagnostic ({workspace})")).await?;
    Ok(Json(json!({ "secret": e.secret, "uri": e.uri, "qr_svg": e.qr_svg })))
}

async fn mfa_confirm(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let codes = auth::confirm_totp(&state.pool, s.user.id, &b.code).await.map_err(bad)?;
    auth::mark_mfa_passed(&state.pool, s).await?;
    audit(&state, &s.user.email, "mfa.enabled", "").await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(Deserialize)]
struct ConfirmPassword {
    password: String,
}

async fn mfa_disable(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<ConfirmPassword>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if auth::require_mfa(&state.pool, state.project_id).await? {
        return Err(bad("your workspace requires two-step verification, so it can't be turned off"));
    }
    if !auth::password_matches(&state.pool, s.user.id, &b.password).await? {
        return Err(ApiError::unauthorized("wrong password"));
    }
    auth::disable_totp(&state.pool, s.user.id).await?;
    audit(&state, &s.user.email, "mfa.disabled", "").await;
    Ok(StatusCode::NO_CONTENT)
}

async fn new_recovery_codes(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<ConfirmPassword>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    if !s.user.totp_enabled {
        return Err(bad("turn on two-step verification first"));
    }
    if !auth::password_matches(&state.pool, s.user.id, &b.password).await? {
        return Err(ApiError::unauthorized("wrong password"));
    }
    let codes = auth::replace_recovery_codes(&state.pool, s.user.id).await?;
    audit(&state, &s.user.email, "mfa.recovery_codes_replaced", "").await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

async fn team(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "users": auth::list_users(&state.pool, state.project_id).await?,
        "invites": auth::pending_invites(&state.pool, state.project_id).await?,
        "require_mfa": auth::require_mfa(&state.pool, state.project_id).await?,
    })))
}

#[derive(Deserialize)]
struct InviteBody {
    email: String,
    role: String,
}

async fn invite(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<InviteBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let role: Role = b.role.parse().map_err(bad)?;
    let (inv, token) = auth::create_invite(&state.pool, state.project_id, &b.email, role, s.user.id).await.map_err(bad)?;
    audit(&state, &s.user.email, "member.invited", &format!("{} as {}", inv.email, inv.role)).await;
    Ok(Json(json!({ "invite": inv, "token": token })))
}

async fn revoke_invite(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !auth::revoke_invite(&state.pool, state.project_id, id).await? {
        return Err(ApiError::not_found_msg("no such invite"));
    }
    audit(&state, &p.actor(), "member.invite_revoked", &format!("invite #{id}")).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RoleBody {
    role: String,
}

async fn target(state: &AppStateRef, p: &Principal, id: i64) -> ApiResult<User> {
    let s = me(p)?;
    if s.user.id == id {
        return Err(bad("you can't change your own access"));
    }
    auth::get_user(&state.pool, state.project_id, id).await?.ok_or_else(|| ApiError::not_found_msg("no such user"))
}

async fn change_role(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>, Json(b): Json<RoleBody>) -> ApiResult<StatusCode> {
    let user = target(&state, &p, id).await?;
    let role: Role = b.role.parse().map_err(bad)?;
    auth::set_role(&state.pool, state.project_id, id, role).await.map_err(bad)?;
    audit(&state, &p.actor(), "member.role_changed", &format!("{}: {} → {role}", user.email, user.role)).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_user(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    target(&state, &p, id).await?;
    let user = auth::remove_user(&state.pool, state.project_id, id).await.map_err(bad)?;
    audit(&state, &p.actor(), "member.removed", &user.email).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_mfa(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let user = target(&state, &p, id).await?;
    auth::disable_totp(&state.pool, id).await?;
    auth::end_user_sessions(&state.pool, id, None).await?;
    audit(&state, &p.actor(), "mfa.reset", &format!("{} (signed out; sets it up again at next sign-in)", user.email)).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SettingsBody {
    require_mfa: bool,
}

async fn team_settings(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<SettingsBody>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if b.require_mfa && !s.user.totp_enabled {
        return Err(bad("turn on two-step verification for your own account first, so you can't lock yourself out"));
    }
    auth::set_require_mfa(&state.pool, state.project_id, b.require_mfa).await?;
    audit(&state, &s.user.email, if b.require_mfa { "security.mfa_required" } else { "security.mfa_optional" }, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// Tokens are personal: everyone manages their own under Account; admins can
// see and revoke anyone's from Team, but not create tokens for other people.

async fn my_tokens(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    Ok(Json(json!(auth::list_api_tokens(&state.pool, state.project_id, Some(s.user.id)).await?)))
}

#[derive(Deserialize)]
struct TokenBody {
    name: String,
    #[serde(default)]
    read_only: bool,
}

async fn create_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<TokenBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let (t, secret) = auth::create_api_token(&state.pool, state.project_id, &s.user, &b.name, b.read_only).await.map_err(bad)?;
    let access = if t.read_only { "read-only".to_string() } else { format!("acts as {}", s.user.role) };
    audit(&state, &s.user.email, "token.created", &format!("{} ({access})", t.name)).await;
    Ok(Json(json!({ "token": t, "secret": secret })))
}

async fn revoke_my_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if !auth::revoke_api_token(&state.pool, state.project_id, id, Some(s.user.id)).await? {
        return Err(ApiError::not_found_msg("no such token"));
    }
    audit(&state, &s.user.email, "token.revoked", &format!("own token #{id}")).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn all_tokens(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(auth::list_api_tokens(&state.pool, state.project_id, None).await?)))
}

async fn revoke_any_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let owner = auth::list_api_tokens(&state.pool, state.project_id, None).await?.into_iter().find(|t| t.id == id).map(|t| t.owner_email);
    if !auth::revoke_api_token(&state.pool, state.project_id, id, None).await? {
        return Err(ApiError::not_found_msg("no such token"));
    }
    audit(&state, &p.actor(), "token.revoked", &format!("token #{id} of {}", owner.unwrap_or_default())).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn audit_log(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(auth::audit_log(&state.pool, state.project_id, 200).await?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_per_endpoint() {
        assert_eq!(required_role(&Method::GET, "/ideas"), Role::Viewer);
        assert_eq!(required_role(&Method::POST, "/ideas/3/status"), Role::Member);
        assert_eq!(required_role(&Method::POST, "/analyze"), Role::Member);
        assert_eq!(required_role(&Method::POST, "/config"), Role::Admin);
        assert_eq!(required_role(&Method::GET, "/team"), Role::Admin);
        assert_eq!(required_role(&Method::POST, "/account/password"), Role::Viewer);
        assert_eq!(required_role(&Method::DELETE, "/something/new"), Role::Admin, "unknown writes fail closed");
    }
}
