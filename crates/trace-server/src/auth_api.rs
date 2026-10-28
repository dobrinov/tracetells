//! Sign-in, two-step verification, invites, team management and API tokens,
//! plus the middleware that decides who may call which dashboard endpoint.

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use trace_core::auth::{self, Role, Session, User};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppStateRef};
use trace_core::db;

pub const COOKIE: &str = "trc_session";
const LOCKOUT_MS: i64 = 15 * 60 * 1000;
/// Wrong passwords for one email from one address before it waits 15 minutes.
const MAX_FAILED_LOGINS: u32 = 10;
/// Wrong passwords for one email from anywhere: high enough that a stranger
/// can't easily lock someone out, low enough to stop a slow, spread-out guess.
const MAX_FAILED_LOGINS_ANYWHERE: u32 = 50;
/// Wrong passwords from one address across all emails (password spraying).
const MAX_FAILED_LOGINS_PER_ADDRESS: u32 = 30;
/// Wrong two-step codes for one person.
const MAX_FAILED_CODES: u32 = 10;
/// New accounts from one address.
const MAX_SIGNUPS_PER_ADDRESS: u32 = 10;

type Connect = Option<Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>>;

fn address(connect: &Connect, headers: &HeaderMap) -> String {
    crate::guard::client_ip(connect.as_ref().map(|Extension(c)| c.0), headers)
}

/// Who is calling a dashboard endpoint, always within one account.
#[derive(Clone)]
pub enum Principal {
    User(Session),
    /// A person's API token: acts with their current role.
    Token(auth::TokenAuth),
    /// The deprecated shared TRACE_ADMIN_TOKEN, which belongs to nobody;
    /// it acts as an admin of the server's first account.
    LegacyToken { account_id: i64 },
}

impl Principal {
    pub fn account(&self) -> i64 {
        match self {
            Self::User(s) => s.user.account_id,
            Self::Token(t) => t.user.account_id,
            Self::LegacyToken { account_id } => *account_id,
        }
    }
    pub fn role(&self) -> Role {
        match self {
            Self::User(s) => s.user.role,
            Self::Token(t) => t.role(),
            Self::LegacyToken { .. } => Role::Admin,
        }
    }
    /// Read-only tokens can read what their owner can, and change nothing.
    pub fn read_only(&self) -> bool {
        matches!(self, Self::Token(t) if t.read_only)
    }
    pub fn actor(&self) -> String {
        match self {
            Self::User(s) => s.user.email.clone(),
            Self::Token(t) => format!("{} via token \"{}\"", t.user.email, t.name),
            Self::LegacyToken { .. } => "TRACE_ADMIN_TOKEN".into(),
        }
    }
    /// The signed-in person, when the caller is one (not a token).
    pub fn session(&self) -> Option<&Session> {
        self.user()
    }
    fn user(&self) -> Option<&Session> {
        match self {
            Self::User(s) => Some(s),
            _ => None,
        }
    }
}

/// The website a request is about, resolved within the caller's account.
#[derive(Clone)]
pub struct CurrentSite(pub trace_core::model::Project);

/// Resolves a bearer token: a person's API token, or the legacy shared one.
pub async fn token_principal(state: &AppStateRef, token: &str) -> Option<Principal> {
    if state.admin_token.as_deref().is_some_and(|t| auth::constant_time_eq(t, token)) {
        let first = db::accounts(&state.pool).await.ok()?.into_iter().next()?;
        return Some(Principal::LegacyToken { account_id: first.id });
    }
    auth::verify_api_token(&state.pool, token).await.ok().flatten().map(Principal::Token)
}

/// The header that names the website. `/api/sites/{site}/...` paths are
/// rewritten to set it (see `api::route_site_paths`); API clients may also send it.
pub const SITE_HEADER: &str = "x-trace-site";

/// Picks the website for a request: the one named, or the account's only one.
/// Another account's website is "not found", so accounts can't probe each other.
pub async fn resolve_site(pool: &trace_core::Pool, account_id: i64, slug: Option<&str>) -> Result<(trace_core::model::Project, bool), Response> {
    let sites = db::sites(pool, account_id).await.map_err(|e| ApiError::from(e).into_response())?;
    let list = || sites.iter().map(|s| json!({ "slug": s.slug, "name": s.name })).collect::<Vec<_>>();
    match slug.filter(|s| !s.is_empty()) {
        Some(slug) => match sites.iter().find(|s| s.slug == slug) {
            Some(site) => Ok((site.clone(), false)),
            None => Err((StatusCode::NOT_FOUND, Json(json!({ "error": format!("no website {slug:?}"), "code": "no_site", "sites": list() }))).into_response()),
        },
        None => match sites.as_slice() {
            [only] => Ok((only.clone(), true)),
            [] => Err((StatusCode::NOT_FOUND, Json(json!({ "error": "this account has no websites yet; add one first", "code": "no_site", "sites": [] }))).into_response()),
            _ => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "this account has several websites; say which one", "code": "site_required", "sites": list() }))).into_response()),
        },
    }
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

/// Paths about one website; everything else is about the account.
fn is_site_path(path: &str) -> bool {
    ["/project", "/config", "/digest", "/kpis", "/insights", "/analyze", "/analysis", "/releases", "/publish", "/query", "/evaluate", "/runs", "/privacy", "/onboarding"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// The least role an endpoint needs. Anything not listed that changes state
/// needs an admin, so a forgotten entry fails closed.
fn required_role(method: &Method, path: &str) -> Role {
    let read = matches!(*method, Method::GET | Method::HEAD);
    let user_actions = path == "/analyze"
        || path == "/evaluate"
        || path == "/releases"
        || path == "/query"
        || (path.starts_with("/insights/") && (path.ends_with("/status") || path.ends_with("/kpi") || path.ends_with("/refresh")));
    match () {
        _ if path.starts_with("/team") || path.starts_with("/audit") => Role::Admin,
        _ if path.starts_with("/account") => Role::User,
        _ if read => Role::User,
        _ if user_actions => Role::User,
        _ => Role::Admin,
    }
}

/// Endpoints only people (not tokens) may use.
fn people_only(path: &str) -> bool {
    ["/team", "/account", "/audit", "/operator", "/sites"].iter().any(|p| path.starts_with(p))
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
        match auth::session(&state.pool, &cookie).await {
            Ok(Some(s)) => {
                // Cookie-authenticated writes must be JSON: browsers can't send that cross-site without CORS.
                let json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
                if !matches!(*req.method(), Method::GET | Method::HEAD) && !json {
                    return deny(StatusCode::FORBIDDEN, "csrf", "requests must be sent as JSON");
                }
                let require_mfa = auth::require_mfa(&state.pool, s.user.account_id).await.unwrap_or(true);
                // While enrolling, a person can still switch to an account that doesn't require it.
                let enrolling = path.starts_with("/account/mfa/") || path == "/account" || path == "/account/switch";
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
        return deny(StatusCode::FORBIDDEN, "people_only", "API tokens can't manage people, tokens, websites or accounts");
    }
    // A question is a POST with a body, but changes nothing.
    let asks = path == "/query" && *req.method() == Method::POST;
    if principal.read_only() && !asks && !matches!(*req.method(), Method::GET | Method::HEAD) {
        return deny(StatusCode::FORBIDDEN, "read_only", "this token is read-only");
    }
    if path.starts_with("/operator") {
        // Operator rights come only from the CLI, and need two-step verification.
        let Some(s) = principal.user().filter(|s| s.user.is_operator) else {
            return deny(StatusCode::FORBIDDEN, "not_operator", "this is only for the server's operators");
        };
        if !s.user.totp_enabled {
            return deny(StatusCode::FORBIDDEN, "operator_mfa", "set up two-step verification under Account to use the operator area");
        }
    } else {
        let needed = required_role(req.method(), &path);
        if principal.role() < needed {
            return deny(StatusCode::FORBIDDEN, "role", &format!("this needs the {needed} role"));
        }
    }
    let mut implicit_site = false;
    if is_site_path(&path) {
        let slug = req.headers().get(SITE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
        match resolve_site(&state.pool, principal.account(), slug.as_deref()).await {
            Ok((site, implicit)) => {
                implicit_site = implicit;
                req.extensions_mut().insert(CurrentSite(site));
            }
            Err(res) => return res,
        }
    }
    req.extensions_mut().insert(principal);
    let mut res = next.run(req).await;
    if implicit_site {
        // Paths without a website still work for one-website accounts, for now.
        res.headers_mut().insert("deprecation", HeaderValue::from_static("true"));
    }
    res
}

pub fn routes() -> Router<AppStateRef> {
    Router::new()
        .route("/auth/state", get(auth_state))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/mfa", post(verify_mfa))
        .route("/auth/logout", post(logout))
        .route("/auth/signup", post(signup))
        .route("/auth/invite/{token}", get(invite_info).post(accept_invite))
        .route("/account", get(account))
        .route("/account/switch", post(switch_account))
        .route("/account/new", post(create_own_account))
        .route("/account/leave", post(leave_account))
        .route("/account/join/{token}", post(join_account))
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
        .route("/sites", get(list_sites).post(add_site))
        .route("/sites/{slug}", delete(remove_site))
        .route("/plan", get(plan_and_usage))
}

fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(e)
}

async fn audit(state: &AppStateRef, account: i64, actor: &str, action: &str, detail: &str) {
    if let Err(e) = auth::audit(&state.pool, account, actor, action, detail).await {
        tracing::error!(error = %e, "writing audit log");
    }
}

async fn current_session(state: &AppStateRef, headers: &HeaderMap) -> ApiResult<Option<Session>> {
    let Some(cookie) = session_cookie(headers) else { return Ok(None) };
    Ok(auth::session(&state.pool, &cookie).await?)
}

async fn state_json(state: &AppStateRef, s: Option<&Session>) -> ApiResult<Value> {
    let (require_mfa, account, accounts) = match s {
        Some(s) => (
            auth::require_mfa(&state.pool, s.user.account_id).await?,
            Some(db::account(&state.pool, s.user.account_id).await?),
            auth::memberships(&state.pool, s.user.id).await?,
        ),
        None => (false, None, Vec::new()),
    };
    Ok(json!({
        "needs_setup": auth::total_users(&state.pool).await? == 0,
        "allow_signup": state.allow_signup,
        "terms_url": state.terms_url,
        "privacy_url": state.privacy_url,
        "user": s.map(|s| &s.user),
        "account": account,
        // Every account the person belongs to, for switching between them.
        "accounts": accounts,
        "can_create_account": s.is_some_and(|s| may_create_account(state, s)),
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
    let token = auth::create_session(&state.pool, &user, mfa_ok, ua).await?;
    let s = auth::session(&state.pool, &token).await?.ok_or_else(|| bad("session was just created"))?;
    let mut res = Json(state_json(state, Some(&s)).await?).into_response();
    set_cookie(&mut res, Some(&token), state.secure_cookies);
    Ok(res)
}

#[derive(Deserialize)]
struct SetupBody {
    /// The one-time setup code (first account only; sign-up has none).
    #[serde(default)]
    code: String,
    /// The company or project the account is for.
    #[serde(default)]
    account: String,
    email: String,
    #[serde(default)]
    name: String,
    password: String,
}

/// Creates the first (owner) account. Needs the one-time code the server
/// printed at startup, so whoever reaches a fresh server first can't claim it.
async fn setup(State(state): State<AppStateRef>, headers: HeaderMap, Json(b): Json<SetupBody>) -> ApiResult<Response> {
    if auth::total_users(&state.pool).await? > 0 {
        return Err(ApiError::conflict("this server already has an owner; sign in instead"));
    }
    let valid = state.setup_code.lock().unwrap().as_deref().is_some_and(|c| auth::constant_time_eq(c, b.code.trim()));
    if !valid {
        return Err(ApiError::forbidden("that setup link isn't valid; use the one the server printed when it started"));
    }
    let user = new_account(&state, &b.account, &b.email, &b.name, &b.password).await?;
    *state.setup_code.lock().unwrap() = None;
    signed_in(&state, user, true, &headers).await
}

/// An account on the default plan with its first website and its owner.
async fn new_account(state: &AppStateRef, account: &str, email: &str, name: &str, password: &str) -> ApiResult<User> {
    auth::check_new_password(password).map_err(bad)?;
    if auth::find_user(&state.pool, email).await?.is_some() {
        return Err(bad(format!("{} is already in use; sign in instead, then add another account from the website menu", email.trim())));
    }
    let account = account.trim();
    let account = if account.is_empty() { email.split('@').nth(1).unwrap_or("My account") } else { account };
    let (acct, _) = db::create_account_with_site(&state.pool, account, None).await.map_err(bad)?;
    let user = auth::create_user(&state.pool, acct.id, email, name, password, Role::Owner).await.map_err(bad)?;
    audit(state, acct.id, &user.email, "account.created", &format!("owner {}", user.email)).await;
    Ok(user)
}

/// Self-service sign-up, when the operator has enabled it.
async fn signup(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Json(b): Json<SetupBody>) -> ApiResult<Response> {
    if !state.allow_signup {
        return Err(ApiError::forbidden("accounts on this server are created by invitation; ask its operator for an invite"));
    }
    let key = format!("signup:{}", address(&connect, &headers));
    if !state.limiter.hit(&key, MAX_SIGNUPS_PER_ADDRESS, LOCKOUT_MS) {
        return Err(ApiError::too_many("too many sign-ups from this address; try again later"));
    }
    let user = new_account(&state, &b.account, &b.email, &b.name, &b.password).await?;
    signed_in(&state, user, true, &headers).await
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

async fn login(State(state): State<AppStateRef>, connect: Connect, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<Response> {
    let email = b.email.trim().to_lowercase();
    let ip = address(&connect, &headers);
    let limits = [
        (format!("login:{email}|{ip}"), MAX_FAILED_LOGINS),
        (format!("login:{email}"), MAX_FAILED_LOGINS_ANYWHERE),
        (format!("login-from:{ip}"), MAX_FAILED_LOGINS_PER_ADDRESS),
    ];
    if limits.iter().any(|(key, limit)| state.limiter.over(key, *limit)) {
        return Err(ApiError::too_many("too many failed attempts; try again in 15 minutes"));
    }
    match auth::authenticate(&state.pool, &b.email, &b.password).await? {
        Some(user) => {
            state.limiter.clear(&limits[0].0);
            let needs_code = user.totp_enabled;
            signed_in(&state, user, !needs_code, &headers).await
        }
        None => {
            for (key, _) in &limits {
                state.limiter.add(key, LOCKOUT_MS);
            }
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
    if state.limiter.over(&key, MAX_FAILED_CODES) {
        return Err(ApiError::too_many("too many wrong codes; try again in 15 minutes"));
    }
    match auth::verify_second_factor(&state.pool, s.user.id, &b.code).await? {
        Some(kind) => {
            state.limiter.clear(&key);
            auth::mark_mfa_passed(&state.pool, &s).await?;
            let s = Session { mfa_ok: true, ..s };
            let mut out = state_json(&state, Some(&s)).await?;
            if kind == auth::SecondFactor::RecoveryCode {
                out["recovery_codes_left"] = json!(auth::recovery_codes_left(&state.pool, s.user.id).await?);
                audit(&state, s.user.account_id, &s.user.email, "mfa.recovery_code_used", "").await;
            }
            Ok(Json(out))
        }
        None => {
            state.limiter.add(&key, LOCKOUT_MS);
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
    let (account, invite) = auth::find_invite(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::not_found_msg("this invite link has expired or was already used"))?;
    let workspace = db::account(&state.pool, account).await?.name;
    // Someone who already has a login joins by signing in, never by choosing a password.
    let has_login = auth::has_login(&state.pool, &invite.email).await?;
    Ok(Json(json!({ "email": invite.email, "role": invite.role, "workspace": workspace, "has_login": has_login,
        "require_mfa": auth::require_mfa(&state.pool, account).await? })))
}

#[derive(Deserialize)]
struct AcceptBody {
    #[serde(default)]
    name: String,
    password: String,
}

async fn accept_invite(State(state): State<AppStateRef>, Path(token): Path<String>, headers: HeaderMap, Json(b): Json<AcceptBody>) -> ApiResult<Response> {
    let user = auth::accept_invite(&state.pool, &token, &b.name, &b.password).await.map_err(bad)?;
    audit(&state, user.account_id, &user.email, "member.joined", &format!("as {}", user.role)).await;
    signed_in(&state, user, true, &headers).await
}

fn me(p: &Principal) -> ApiResult<&Session> {
    p.user().ok_or_else(|| ApiError::forbidden("sign in as a person to do this"))
}

// ---- several accounts ------------------------------------------------------------
//
// One login can belong to several accounts (see `memberships`). The session
// acts in one of them at a time; these move it, add to it and leave one.

/// The auth state once the session acts as `user` (in their new account).
async fn state_as(state: &AppStateRef, s: &Session, user: User) -> ApiResult<Json<Value>> {
    Ok(Json(state_json(state, Some(&Session { user, ..s.clone() })).await?))
}

/// New accounts come from sign-up, so a signed-in person may add one when
/// sign-up is open (and an operator always may).
fn may_create_account(state: &AppStateRef, s: &Session) -> bool {
    state.allow_signup || s.user.is_operator
}

#[derive(Deserialize)]
struct SwitchBody {
    account_id: i64,
}

async fn switch_account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<SwitchBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    // Not a member is "not found", like any other account's data.
    let user = auth::switch_account(&state.pool, s, b.account_id).await.map_err(|e| ApiError::not_found_msg(&e.to_string()))?;
    state_as(&state, s, user).await
}

#[derive(Deserialize)]
struct NewAccountBody {
    /// The company or project the account is for.
    name: String,
}

/// Another account for the signed-in person, which they own, with its first
/// website. The session moves to it.
async fn create_own_account(
    State(state): State<AppStateRef>,
    Extension(p): Extension<Principal>,
    connect: Connect,
    headers: HeaderMap,
    Json(b): Json<NewAccountBody>,
) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    if !may_create_account(&state, s) {
        return Err(ApiError::forbidden("new accounts on this server are created by its operator; ask them for one"));
    }
    if !state.limiter.hit(&format!("signup:{}", address(&connect, &headers)), MAX_SIGNUPS_PER_ADDRESS, LOCKOUT_MS) {
        return Err(ApiError::too_many("too many new accounts from this address; try again later"));
    }
    let (acct, _) = db::create_account_with_site(&state.pool, &b.name, None).await.map_err(bad)?;
    auth::add_member(&state.pool, acct.id, s.user.id, Role::Owner).await?;
    audit(&state, acct.id, &s.user.email, "account.created", &format!("owner {}", s.user.email)).await;
    let user = auth::switch_account(&state.pool, s, acct.id).await?;
    state_as(&state, s, user).await
}

/// Leaves the account the session is in, for one of the person's others.
async fn leave_account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    if s.user.role == Role::Owner {
        return Err(bad("the owner can't leave their own account"));
    }
    if auth::memberships(&state.pool, s.user.id).await?.len() < 2 {
        return Err(bad("this is your only account; ask an admin to remove you instead"));
    }
    let account = s.user.account_id;
    auth::remove_user(&state.pool, account, s.user.id).await.map_err(bad)?;
    audit(&state, account, &s.user.email, "member.left", "").await;
    let home = auth::home_user(&state.pool, s.user.id).await?.ok_or_else(|| ApiError::unauthorized("sign in again"))?;
    let user = auth::switch_account(&state.pool, s, home.account_id).await?;
    state_as(&state, s, user).await
}

/// Accepts an invite as the person signed in, when it was sent to their email.
async fn join_account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let user = auth::join_with_invite(&state.pool, &token, &s.user).await.map_err(bad)?;
    audit(&state, user.account_id, &user.email, "member.joined", &format!("as {}, with an existing login", user.role)).await;
    let user = auth::switch_account(&state.pool, s, user.account_id).await?;
    state_as(&state, s, user).await
}

async fn account(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    Ok(Json(json!({
        "user": s.user,
        "recovery_codes_left": auth::recovery_codes_left(&state.pool, s.user.id).await?,
        "require_mfa": auth::require_mfa(&state.pool, s.user.account_id).await?,
        "account": db::account(&state.pool, s.user.account_id).await?,
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
    audit(&state, s.user.account_id, &s.user.email, "account.password_changed", "other sessions signed out").await;
    Ok(StatusCode::NO_CONTENT)
}

async fn mfa_start(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    if s.user.totp_enabled {
        return Err(bad("two-step verification is already on; turn it off first to move to a new device"));
    }
    let workspace = db::account(&state.pool, s.user.account_id).await?.name;
    let e = auth::begin_totp(&state.pool, &s.user, &format!("Trace ({workspace})")).await?;
    Ok(Json(json!({ "secret": e.secret, "uri": e.uri, "qr_svg": e.qr_svg })))
}

async fn mfa_confirm(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let codes = auth::confirm_totp(&state.pool, s.user.id, &b.code).await.map_err(bad)?;
    auth::mark_mfa_passed(&state.pool, s).await?;
    audit(&state, s.user.account_id, &s.user.email, "mfa.enabled", "").await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(Deserialize)]
struct ConfirmPassword {
    password: String,
}

async fn mfa_disable(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<ConfirmPassword>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if auth::require_mfa(&state.pool, s.user.account_id).await? {
        return Err(bad("your account requires two-step verification, so it can't be turned off"));
    }
    if !auth::password_matches(&state.pool, s.user.id, &b.password).await? {
        return Err(ApiError::unauthorized("wrong password"));
    }
    auth::disable_totp(&state.pool, s.user.id).await?;
    audit(&state, s.user.account_id, &s.user.email, "mfa.disabled", "").await;
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
    audit(&state, s.user.account_id, &s.user.email, "mfa.recovery_codes_replaced", "").await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

async fn team(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let a = p.account();
    Ok(Json(json!({
        "users": auth::list_users(&state.pool, a).await?,
        "invites": auth::pending_invites(&state.pool, a).await?,
        "require_mfa": auth::require_mfa(&state.pool, a).await?,
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
    let (inv, token) = auth::create_invite(&state.pool, s.user.account_id, &b.email, role, Some(s.user.id)).await.map_err(bad)?;
    audit(&state, s.user.account_id, &s.user.email, "member.invited", &format!("{} as {}", inv.email, inv.role)).await;
    Ok(Json(json!({ "invite": inv, "token": token })))
}

async fn revoke_invite(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !auth::revoke_invite(&state.pool, p.account(), id).await? {
        return Err(ApiError::not_found_msg("no such invite"));
    }
    audit(&state, p.account(), &p.actor(), "member.invite_revoked", &format!("invite #{id}")).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RoleBody {
    role: String,
}

/// The person a team action is about. Nobody acts on themselves this way, and
/// only the owner can act on the owner: an admin can't reset the owner's
/// two-step verification or sign them out.
async fn target(state: &AppStateRef, p: &Principal, id: i64) -> ApiResult<User> {
    let s = me(p)?;
    if s.user.id == id {
        return Err(bad("you can't change your own access"));
    }
    let user = auth::get_user(&state.pool, p.account(), id).await?.ok_or_else(|| ApiError::not_found_msg("no such user"))?;
    if user.role == Role::Owner && s.user.role != Role::Owner {
        return Err(ApiError::forbidden("only the owner can change the owner's access"));
    }
    Ok(user)
}

async fn change_role(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>, Json(b): Json<RoleBody>) -> ApiResult<StatusCode> {
    let user = target(&state, &p, id).await?;
    let role: Role = b.role.parse().map_err(bad)?;
    auth::set_role(&state.pool, p.account(), id, role).await.map_err(bad)?;
    audit(&state, p.account(), &p.actor(), "member.role_changed", &format!("{}: {} → {role}", user.email, user.role)).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_user(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    target(&state, &p, id).await?;
    let user = auth::remove_user(&state.pool, p.account(), id).await.map_err(bad)?;
    audit(&state, p.account(), &p.actor(), "member.removed", &user.email).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_mfa(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let user = target(&state, &p, id).await?;
    auth::disable_totp(&state.pool, id).await?;
    auth::end_user_sessions(&state.pool, id, None).await?;
    audit(&state, p.account(), &p.actor(), "mfa.reset", &format!("{} (signed out; sets it up again at next sign-in)", user.email)).await;
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
    auth::set_require_mfa(&state.pool, s.user.account_id, b.require_mfa).await?;
    audit(&state, s.user.account_id, &s.user.email, if b.require_mfa { "security.mfa_required" } else { "security.mfa_optional" }, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// Tokens are personal: everyone manages their own under Account; admins can
// see and revoke anyone's from Team, but not create tokens for other people.

async fn my_tokens(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    Ok(Json(json!(auth::list_api_tokens(&state.pool, s.user.account_id, Some(s.user.id)).await?)))
}

#[derive(Deserialize)]
struct TokenBody {
    name: String,
    #[serde(default)]
    read_only: bool,
}

async fn create_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<TokenBody>) -> ApiResult<Json<Value>> {
    let s = me(&p)?;
    let (t, secret) = auth::create_api_token(&state.pool, s.user.account_id, &s.user, &b.name, b.read_only).await.map_err(bad)?;
    let access = if t.read_only { "read-only".to_string() } else { format!("acts as {}", s.user.role) };
    audit(&state, s.user.account_id, &s.user.email, "token.created", &format!("{} ({access})", t.name)).await;
    Ok(Json(json!({ "token": t, "secret": secret })))
}

async fn revoke_my_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let s = me(&p)?;
    if !auth::revoke_api_token(&state.pool, s.user.account_id, id, Some(s.user.id)).await? {
        return Err(ApiError::not_found_msg("no such token"));
    }
    audit(&state, s.user.account_id, &s.user.email, "token.revoked", &format!("own token #{id}")).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn all_tokens(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(auth::list_api_tokens(&state.pool, p.account(), None).await?)))
}

async fn revoke_any_token(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let owner = auth::list_api_tokens(&state.pool, p.account(), None).await?.into_iter().find(|t| t.id == id).map(|t| t.owner_email);
    if !auth::revoke_api_token(&state.pool, p.account(), id, None).await? {
        return Err(ApiError::not_found_msg("no such token"));
    }
    audit(&state, p.account(), &p.actor(), "token.revoked", &format!("token #{id} of {}", owner.unwrap_or_default())).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn audit_log(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(auth::audit_log(&state.pool, p.account(), 200).await?)))
}

// ---- websites ------------------------------------------------------------------

async fn list_sites(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let mut out = Vec::new();
    for site in db::sites(&state.pool, p.account()).await? {
        let collection = db::collection(&state.pool, site.id).await?;
        out.push(json!({ "name": site.name, "slug": site.slug, "write_key": site.write_key, "collection": collection }));
    }
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
struct SiteBody {
    name: String,
}

async fn add_site(State(state): State<AppStateRef>, Extension(p): Extension<Principal>, Json(b): Json<SiteBody>) -> ApiResult<Json<Value>> {
    let site = db::create_site(&state.pool, p.account(), &b.name).await.map_err(bad)?;
    audit(&state, p.account(), &p.actor(), "site.added", &format!("{} ({})", site.name, site.slug)).await;
    Ok(Json(json!({ "name": site.name, "slug": site.slug, "write_key": site.write_key })))
}

#[derive(Deserialize)]
struct RemoveSiteBody {
    /// The website's name, typed to confirm.
    confirm: String,
}

async fn remove_site(
    State(state): State<AppStateRef>,
    Extension(p): Extension<Principal>,
    Path(slug): Path<String>,
    Json(b): Json<RemoveSiteBody>,
) -> ApiResult<StatusCode> {
    let site = db::site_by_slug(&state.pool, p.account(), &slug).await?.ok_or_else(|| ApiError::not_found_msg("no such website"))?;
    if b.confirm.trim() != site.name {
        return Err(bad(format!("type the website's name, {:?}, to confirm", site.name)));
    }
    db::delete_site(&state.pool, p.account(), site.id).await?;
    crate::api::forget_site(&state, site.id);
    audit(&state, p.account(), &p.actor(), "site.removed", &format!("{} ({}) and all its data", site.name, site.slug)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---- plan & usage (no plan names: those are for operators) ------------------------

async fn plan_and_usage(State(state): State<AppStateRef>, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let a = p.account();
    let plan = trace_core::plans::for_account(&state.pool, a).await?;
    let usage = trace_core::usage::get(&state.pool, a, &trace_core::usage::this_month()).await?;
    let used = json!({
        "retention_days": null,
        "max_sites": db::sites(&state.pool, a).await?.len(),
        "monthly_events": usage.events,
        "max_members": auth::user_count(&state.pool, a).await? + auth::pending_invites(&state.pool, a).await?.len() as i64,
        "monthly_ai_runs": usage.ai_runs,
    });
    Ok(Json(json!({
        // Claude analyses only count on a server that runs Claude itself.
        "limits": trace_core::plans::LIMITS.iter().filter(|(key, _)| *key != "monthly_ai_runs" || state.claude.is_some()).map(|(key, label)| json!({
            "key": key, "label": label, "limit": plan.effective.get(key), "used": used[*key],
        })).collect::<Vec<_>>(),
        "month": usage.month,
        "events_dropped": usage.events_dropped,
        "server_retention_days": (state.retention_days > 0).then_some(state.retention_days),
        "contact_url": state.contact_url,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_per_endpoint() {
        assert_eq!(required_role(&Method::GET, "/insights"), Role::User);
        assert_eq!(required_role(&Method::POST, "/insights/3/status"), Role::User);
        assert_eq!(required_role(&Method::POST, "/analyze"), Role::User);
        assert_eq!(required_role(&Method::POST, "/config"), Role::Admin);
        assert_eq!(required_role(&Method::POST, "/sites"), Role::Admin);
        assert_eq!(required_role(&Method::GET, "/team"), Role::Admin);
        assert_eq!(required_role(&Method::POST, "/account/password"), Role::User);
        assert_eq!(required_role(&Method::DELETE, "/something/new"), Role::Admin, "unknown writes fail closed");
        assert!(is_site_path("/insights/3/prompt") && is_site_path("/privacy") && !is_site_path("/plan") && !is_site_path("/sites"));
    }
}
