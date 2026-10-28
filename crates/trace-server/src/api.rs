use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use trace_agent::{ClaudeConfig, Engine};
use trace_core::model::InsightStatus;
use trace_core::privacy::{self, HostRules, RuleKind};
use trace_core::{Pool, config, db, digest, evaluate, insights, ingest, kpi};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};

use crate::auth_api::{CurrentSite, SITE_HEADER};

pub struct AppState {
    pub pool: Pool,
    /// Deprecated shared token; personal API tokens replace it.
    pub admin_token: Option<String>,
    pub claude: Option<ClaudeConfig>,
    /// One-time code for creating the first account; None once one exists.
    pub setup_code: Mutex<Option<String>>,
    /// Sign-in, two-step and sign-up attempts.
    pub limiter: crate::guard::Counter,
    /// Collector batches per visitor address and per website.
    pub collect_limits: crate::guard::Counter,
    /// The address people and MCP clients reach this server at (`TRACE_PUBLIC_URL`).
    pub public_url: Option<String>,
    /// Mark the session cookie Secure (the dashboard is served over HTTPS).
    pub secure_cookies: bool,
    /// Self-service sign-up creates accounts on the default plan.
    pub allow_signup: bool,
    /// Where people ask for more (Plan & usage).
    pub contact_url: Option<String>,
    /// Terms and privacy policy accepted at sign-up, if the operator has them.
    pub terms_url: Option<String>,
    pub privacy_url: Option<String>,
    /// The server-wide retention cap in days (0 = none).
    pub retention_days: u32,
    /// Each website's privacy rules, compiled on first use; see [`reload_rules`].
    pub rules: RwLock<HashMap<i64, Arc<HostRules>>>,
    /// Served collectors by website (0 = the rule-free `/d.js`), built lazily.
    pub collectors: RwLock<HashMap<i64, Arc<Collector>>>,
    /// Monthly event quota per account, refreshed from the database each minute.
    pub quotas: Mutex<HashMap<i64, Quota>>,
}

pub type AppStateRef = Arc<AppState>;

impl AppState {
    pub fn new(pool: Pool, claude: Option<ClaudeConfig>) -> Self {
        Self {
            pool,
            admin_token: None,
            claude,
            setup_code: Mutex::new(None),
            limiter: Default::default(),
            collect_limits: Default::default(),
            public_url: None,
            secure_cookies: false,
            allow_signup: false,
            contact_url: None,
            terms_url: None,
            privacy_url: None,
            retention_days: 90,
            rules: Default::default(),
            collectors: Default::default(),
            quotas: Default::default(),
        }
    }
}

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const STYLE_CSS: &str = include_str!("../assets/style.css");
const COLLECTOR_JS: &str = include_str!("../assets/d.js");
static DASHBOARD_CSP: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| crate::guard::dashboard_csp(INDEX_HTML));

pub fn router(state: AppStateRef) -> Router {
    let collector = Router::new()
        .route("/v1/collect", post(collect))
        .route("/d.js", get(collector_js))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST]).allow_headers(Any));

    let api = Router::new()
        .route("/project", get(get_project))
        .route("/config", get(get_config).post(push_config))
        .route("/digest", get(get_digest))
        .route("/kpis", get(list_kpis))
        .route("/insights", get(list_insights))
        .route("/insights/{id}", get(get_insight))
        .route("/insights/{id}/prompt", get(get_prompt))
        .route("/insights/{id}/status", post(set_status))
        .route("/insights/{id}/kpi", post(set_kpi))
        .route("/releases", post(record_release))
        .route("/analyze", post(analyze))
        .route("/evaluate", post(run_evaluate))
        .route("/runs", get(list_runs))
        .route("/analysis", get(get_analysis))
        .route("/privacy", get(get_privacy))
        .route("/privacy/rules", post(add_privacy_rule))
        .route("/privacy/rules/{id}", post(toggle_privacy_rule).delete(delete_privacy_rule))
        .route("/privacy/test", post(test_privacy))
        .route("/onboarding", get(get_onboarding))
        .route("/onboarding/privacy-reviewed", post(confirm_privacy_review))
        .merge(crate::auth_api::routes())
        .merge(crate::oauth::api_routes())
        .merge(crate::operator::routes())
        .layer(middleware::from_fn_with_state(state.clone(), crate::auth_api::authorize));

    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/assets/app.js", get(|| async { asset("application/javascript; charset=utf-8", APP_JS, "no-cache") }))
        .route("/assets/style.css", get(|| async { asset("text/css; charset=utf-8", STYLE_CSS, "no-cache") }))
        .nest("/api", api)
        .route("/mcp", post(mcp_http).get(|| async { (StatusCode::METHOD_NOT_ALLOWED, "this MCP endpoint takes POST requests") }))
        .merge(collector)
        .merge(crate::oauth::routes())
        .with_state(state)
        .layer(middleware::from_fn(|req, next| crate::guard::security_headers(&DASHBOARD_CSP, req, next)))
        // Brotli or gzip, whichever the client accepts: the collector goes from ~32 KB to ~6 KB on the wire.
        .layer(CompressionLayer::new())
}

/// Runs before routing (it wraps the whole router): `/api/sites/{site}/rest`
/// becomes `/api/rest` with the website in the `X-Trace-Site` header, so
/// every website endpoint has one handler whether or not the path names the website.
pub fn route_site_paths(mut req: Request) -> Request {
    let path = req.uri().path();
    if let Some(rest) = path.strip_prefix("/api/sites/")
        && let Some((slug, tail)) = rest.split_once('/')
        && !slug.is_empty()
        && !tail.is_empty()
    {
        let slug = slug.to_string();
        let new_path = format!("/api/{tail}");
        let pq = match req.uri().query() {
            Some(q) => format!("{new_path}?{q}"),
            None => new_path,
        };
        if let (Ok(uri), Ok(value)) = (pq.parse::<Uri>(), HeaderValue::from_str(&slug)) {
            *req.uri_mut() = uri;
            req.headers_mut().insert(SITE_HEADER, value);
        }
    }
    req
}

/// The collector as served: the commented source in the repo, minus whole-line
/// comments, blank lines and indentation. It's ES5 with no multi-line strings,
/// so this is safe without a full minifier; newlines are kept for ASI.
fn strip_js(src: &str) -> String {
    let mut out = String::from("/*! Trace collector | MIT License | https://github.com/dobrinov/tracetells/blob/main/crates/trace-server/assets/d.js */\n");
    let mut in_block = false;
    for line in src.lines() {
        let t = line.trim();
        if in_block {
            in_block = !t.ends_with("*/");
            continue;
        }
        if t.starts_with("/*") {
            in_block = !t.ends_with("*/");
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        out.push_str(t);
        out.push('\n');
    }
    out
}

/// The stripped collector with one website's privacy rules written in,
/// compressed once at maximum quality (too slow to do per request), plus a
/// hash for its ETag. Rebuilt when an admin changes that website's rules.
pub struct Collector {
    plain: Vec<u8>,
    brotli: Vec<u8>,
    gzip: Vec<u8>,
    hash: String,
}

const RULES_PLACEHOLDER: &str = "/*__TRACE_RULES__*/ null";

fn build_collector(rules: Option<&HostRules>) -> Collector {
    use std::io::Write;
    let js = strip_js(COLLECTOR_JS);
    debug_assert!(js.contains(RULES_PLACEHOLDER), "d.js lost its rules placeholder");
    let plain = match rules {
        Some(r) => js.replacen(RULES_PLACEHOLDER, &r.client_json().to_string(), 1),
        None => js,
    }
    .into_bytes();
    let mut brotli = Vec::new();
    {
        let mut w = brotli::CompressorWriter::new(&mut brotli, 4096, 11, 22);
        w.write_all(&plain).expect("in-memory write");
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&plain).expect("in-memory write");
    let gzip = gz.finish().expect("in-memory write");
    // FNV-1a: only needs to change when the file does.
    let hash = plain.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3));
    Collector { plain, brotli, gzip, hash: format!("{hash:016x}") }
}

/// A website's compiled privacy rules, loaded on first use.
pub async fn site_rules(state: &AppState, site: i64) -> anyhow::Result<Arc<HostRules>> {
    if let Some(r) = state.rules.read().unwrap().get(&site).cloned() {
        return Ok(r);
    }
    let rules = Arc::new(HostRules::load(&state.pool, site).await?);
    state.rules.write().unwrap().insert(site, rules.clone());
    Ok(rules)
}

/// After a rule change: ingest uses the new rules at once, and the next
/// request for that website's d.js gets a collector with them built in.
pub async fn reload_rules(state: &AppState, site: i64) -> anyhow::Result<()> {
    forget_site(state, site);
    site_rules(state, site).await?;
    Ok(())
}

/// Drops a website's cached rules and collector (after a change, or when it's removed).
pub fn forget_site(state: &AppState, site: i64) {
    state.rules.write().unwrap().remove(&site);
    state.collectors.write().unwrap().remove(&site);
}

#[derive(Deserialize)]
struct CollectorQuery {
    /// The website's write key: its privacy rules are built in.
    #[serde(default)]
    k: Option<String>,
}

/// Cached for an hour, then revalidated cheaply with If-None-Match, so sites
/// pick up collector updates (and privacy rule changes) without changing their snippet.
async fn collector_js(State(state): State<AppStateRef>, Query(q): Query<CollectorQuery>, headers: HeaderMap) -> Response {
    let site = match q.k.as_deref() {
        Some(k) => db::project_by_key(&state.pool, k).await.ok().flatten().map(|(id, _)| id),
        None => None,
    };
    let key = site.unwrap_or(0);
    let cached = state.collectors.read().unwrap().get(&key).cloned();
    let c = match cached {
        Some(c) => c,
        None => {
            let rules = match site {
                Some(id) => site_rules(&state, id).await.ok(),
                None => None,
            };
            let c = Arc::new(build_collector(rules.as_deref()));
            state.collectors.write().unwrap().insert(key, c.clone());
            c
        }
    };
    let accepts = headers.get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or("");
    let (body, encoding, tag) = if accepts.contains("br") {
        (&c.brotli, Some("br"), "br")
    } else if accepts.contains("gzip") {
        (&c.gzip, Some("gzip"), "gz")
    } else {
        (&c.plain, None, "id")
    };
    let etag = format!("\"{}-{tag}\"", c.hash);
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag));
    let mut res = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], body.clone()).into_response()
    };
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("public, max-age=3600, stale-while-revalidate=86400"));
    h.insert(header::VARY, header::HeaderValue::from_static("Accept-Encoding"));
    h.insert(header::ETAG, header::HeaderValue::from_str(&etag).expect("hex etag"));
    if let (Some(enc), false) = (encoding, fresh) {
        h.insert(header::CONTENT_ENCODING, header::HeaderValue::from_static(enc));
    }
    res
}

fn asset(content_type: &'static str, body: &'static str, cache: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, cache)], body).into_response()
}

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn not_found(what: &str) -> Self {
        Self(StatusCode::NOT_FOUND, format!("{what} not found"))
    }
    pub fn not_found_msg(msg: &str) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
    }
    pub fn bad_request(e: impl std::fmt::Display) -> Self {
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
    pub fn unauthorized(msg: &str) -> Self {
        Self(StatusCode::UNAUTHORIZED, msg.into())
    }
    pub fn forbidden(msg: &str) -> Self {
        Self(StatusCode::FORBIDDEN, msg.into())
    }
    pub fn conflict(msg: &str) -> Self {
        Self(StatusCode::CONFLICT, msg.into())
    }
    pub fn too_many(msg: &str) -> Self {
        Self(StatusCode::TOO_MANY_REQUESTS, msg.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        // Details go to the server log only: some endpoints are reachable before signing in.
        tracing::error!(error = %format!("{e:#}"), "request failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong on the server; the details are in its log".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// An account's monthly event quota, kept in memory so ingest needs no extra
/// reads. It can be overshot by at most a minute of traffic per server process.
#[derive(Clone)]
pub struct Quota {
    month: String,
    used: i64,
    limit: Option<i64>,
    refreshed: i64,
}

/// How many of `n` events an account may still record this month.
async fn take_events(state: &AppState, account: i64, n: i64) -> anyhow::Result<i64> {
    let now = trace_core::util::now_ms();
    let month = trace_core::usage::month_of(now);
    let cached = state.quotas.lock().unwrap().get(&account).cloned();
    let mut q = match cached {
        Some(q) if q.month == month && now - q.refreshed < 60_000 => q,
        _ => Quota {
            used: trace_core::usage::get(&state.pool, account, &month).await?.events,
            limit: trace_core::plans::for_account(&state.pool, account).await?.effective.monthly_events,
            month,
            refreshed: now,
        },
    };
    let allowed = match q.limit {
        Some(limit) => n.min((limit - q.used).max(0)),
        None => n,
    };
    q.used += allowed;
    state.quotas.lock().unwrap().insert(account, q);
    Ok(allowed)
}

/// Collector batches one visitor address may send per minute. A visitor's
/// browser sends a few; a whole office behind one address, a few hundred.
const BATCHES_PER_ADDRESS: u32 = 600;
/// Collector batches one website may receive per minute (100 a second).
const BATCHES_PER_SITE: u32 = 6000;

async fn collect(
    State(state): State<AppStateRef>,
    connect: Option<axum::Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    // Flooding from one address is refused before any work, even parsing.
    let ip = crate::guard::client_ip(connect.map(|axum::Extension(c)| c.0), &headers);
    if !state.collect_limits.hit(&format!("from:{ip}"), BATCHES_PER_ADDRESS, 60_000) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many batches from this address".into()));
    }
    // The collector posts JSON as text/plain so browsers skip the CORS preflight.
    let mut batch: ingest::Batch = serde_json::from_slice(&body).map_err(ApiError::bad_request)?;
    let Some((site, account)) = db::project_by_key(&state.pool, &batch.key).await? else {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "unknown write key".into()));
    };
    if !state.collect_limits.hit(&format!("site:{site}"), BATCHES_PER_SITE, 60_000) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many batches for this website".into()));
    }
    // Over the monthly limit: drop the rest, count it, and still answer 204 so
    // visitors' browsers do no extra work and never retry.
    let n = batch.events.len() as i64;
    let allowed = take_events(&state, account, n).await?;
    batch.events.truncate(allowed as usize);
    if batch.events.is_empty() {
        trace_core::usage::add_events(&state.pool, account, 0, n).await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    let origin = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|v| v.to_str().ok());
    let rules = site_rules(&state, site).await?;
    if let Err(e) = ingest::store(&state.pool, site, batch, ua, origin, &rules).await {
        // A rejected batch uses none of the account's quota.
        give_back_events(&state, account, allowed);
        return Err(ApiError::bad_request(e));
    }
    trace_core::usage::add_events(&state.pool, account, allowed, n - allowed).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns events taken by [`take_events`] that weren't stored after all.
fn give_back_events(state: &AppState, account: i64, n: i64) {
    if let Some(q) = state.quotas.lock().unwrap().get_mut(&account) {
        q.used = (q.used - n).max(0);
    }
}

async fn get_project(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "name": site.name,
        "slug": site.slug,
        "write_key": site.write_key,
        "collection": db::collection(&state.pool, site.id).await?,
        "claude": state.claude.as_ref().map(|c| json!({ "model": c.model, "effort": c.effort })),
    })))
}

#[derive(Deserialize)]
struct DaysQuery {
    #[serde(default = "default_days")]
    days: u32,
}

fn default_days() -> u32 {
    14
}

async fn get_digest(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Query(q): Query<DaysQuery>) -> ApiResult<Json<digest::Digest>> {
    Ok(Json(digest::build(&state.pool, site.id, q.days.clamp(1, 365)).await?))
}

async fn list_kpis(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Query(q): Query<DaysQuery>) -> ApiResult<Json<Value>> {
    let now = trace_core::util::now_ms();
    let span = q.days.clamp(1, 365) as i64 * trace_core::util::DAY_MS;
    let mut out = Vec::new();
    for k in kpi::list(&state.pool, site.id).await? {
        let current = kpi::measure(&state.pool, site.id, &k.definition, now - span, now).await?;
        let previous = kpi::measure(&state.pool, site.id, &k.definition, now - 2 * span, now - span).await?;
        let mut v = serde_json::to_value(&k).map_err(anyhow::Error::from)?;
        v["summary"] = json!(k.definition.describe());
        v["current"] = json!(current.value);
        v["previous"] = json!(previous.value);
        v["sessions"] = json!(current.n());
        v["target_met"] = json!(current.value.and_then(|c| kpi::target_met(&k, c)));
        out.push(v);
    }
    Ok(Json(json!(out)))
}

/// Applies a trace.json pushed by `trace push` (or CI). Invalid
/// configs are rejected with every problem listed.
async fn push_config(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
    headers: HeaderMap,
    body: String,
) -> ApiResult<Json<Value>> {
    let parsed = config::parse(&body).map_err(|errors| ApiError(StatusCode::UNPROCESSABLE_ENTITY, errors.join("\n")))?;
    let source = headers
        .get("x-trace-source")
        .and_then(|v| v.to_str().ok())
        .map(|s| trace_core::util::truncate(s, 200))
        .unwrap_or_else(|| "api".into());
    let report = config::apply(&state.pool, site.id, &parsed, &source).await?;
    trace_core::auth::audit_site(
        &state.pool,
        site.account_id,
        Some(site.id),
        &who.actor(),
        "config.pushed",
        &format!("{source}: {} created, {} updated, {} archived", report.created.len(), report.updated.len(), report.archived.len()),
    )
    .await?;
    Ok(Json(serde_json::to_value(report).map_err(anyhow::Error::from)?))
}

/// Where the config came from, and whether the events it declares are arriving.
async fn get_config(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    let health = config::tracking_health(&state.pool, site.id, 14).await?;
    Ok(Json(serde_json::to_value(health).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
struct InsightFilter {
    status: Option<InsightStatus>,
}

async fn list_insights(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Query(f): Query<InsightFilter>) -> ApiResult<Json<Value>> {
    let list = insights::list(&state.pool, site.id, f.status).await?;
    Ok(Json(serde_json::to_value(list).map_err(anyhow::Error::from)?))
}

async fn get_insight(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let insight = insights::get(&state.pool, site.id, id).await?.ok_or_else(|| ApiError::not_found("insight"))?;
    Ok(Json(serde_json::to_value(insight).map_err(anyhow::Error::from)?))
}

async fn get_prompt(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Path(id): Path<i64>) -> ApiResult<Response> {
    let insight = insights::get(&state.pool, site.id, id).await?.ok_or_else(|| ApiError::not_found("insight"))?;
    let project = db::project(&state.pool, site.id).await?;
    let text = crate::render_prompt(&state.pool, &project, &insight).await?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], text).into_response())
}

#[derive(Deserialize)]
struct StatusBody {
    status: InsightStatus,
    #[serde(default)]
    reason: Option<String>,
    /// When the change went live, if not now: epoch ms or ISO 8601.
    #[serde(default)]
    released_at: Option<Value>,
}

/// A moment given as epoch milliseconds or an ISO 8601 string.
pub(crate) fn time_arg(v: &Value) -> anyhow::Result<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(trace_core::util::parse_time))
        .ok_or_else(|| anyhow::anyhow!("times are epoch milliseconds or ISO 8601, e.g. \"2026-10-28T14:05:00Z\""))
}

async fn set_status(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Path(id): Path<i64>,
    Json(body): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    let at = body.released_at.as_ref().filter(|v| !v.is_null()).map(time_arg).transpose().map_err(ApiError::bad_request)?;
    let insight = insights::set_status(&state.pool, site.id, id, body.status, body.reason.as_deref(), at)
        .await
        .map_err(ApiError::bad_request)?
        .ok_or_else(|| ApiError::not_found("insight"))?;
    Ok(Json(serde_json::to_value(insight).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
struct ReleaseBody {
    /// The insights this deploy ships.
    insights: Vec<i64>,
    /// When it went live, if not now.
    #[serde(default)]
    at: Option<Value>,
    /// The commit or version deployed, for the activity log.
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
}

/// For deploy pipelines: marks the insights a deploy ships as released at the
/// deploy's time, so the before/after split is when the change went live.
async fn record_release(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
    Json(body): Json<ReleaseBody>,
) -> ApiResult<Json<Value>> {
    if body.insights.is_empty() || body.insights.len() > 50 {
        return Err(ApiError::bad_request("list between 1 and 50 insight ids in `insights`"));
    }
    let at = match body.at.as_ref().filter(|v| !v.is_null()) {
        Some(v) => time_arg(v).map_err(ApiError::bad_request)?,
        None => trace_core::util::now_ms(),
    };
    let (mut released, mut skipped) = (Vec::new(), Vec::new());
    for &id in &body.insights {
        let problem = match insights::get(&state.pool, site.id, id).await? {
            None => Some("no such insight".to_string()),
            Some(i) if i.status == InsightStatus::Dismissed => Some("dismissed".into()),
            Some(i) if i.status == InsightStatus::Released && i.released_at.is_some_and(|t| t <= at) => {
                Some(format!("already released on {}", trace_core::util::date(i.released_at.unwrap_or(at))))
            }
            Some(_) => insights::set_status(&state.pool, site.id, id, InsightStatus::Released, None, Some(at)).await.err().map(|e| e.to_string()),
        };
        match problem {
            None => released.push(id),
            Some(reason) => skipped.push(json!({ "id": id, "reason": reason })),
        }
    }
    if !released.is_empty() {
        let git_ref = body.git_ref.as_deref().map(|r| format!(" ({})", trace_core::util::truncate(&trace_core::privacy::one_line(r), 80))).unwrap_or_default();
        let ids = released.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ");
        trace_core::auth::audit_site(&state.pool, site.account_id, Some(site.id), &who.actor(), "insights.released", &format!("{ids}{git_ref}")).await?;
    }
    Ok(Json(json!({ "released": released, "skipped": skipped, "released_at": at })))
}

#[derive(Deserialize)]
struct KpiLinkBody {
    kpi_id: Option<i64>,
}

async fn set_kpi(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Path(id): Path<i64>,
    Json(body): Json<KpiLinkBody>,
) -> ApiResult<Json<Value>> {
    if let Some(k) = body.kpi_id {
        kpi::get(&state.pool, site.id, k).await?.ok_or_else(|| ApiError::not_found("KPI"))?;
    }
    let insight = insights::set_kpi(&state.pool, site.id, id, body.kpi_id)
        .await?
        .ok_or_else(|| ApiError::not_found("insight"))?;
    Ok(Json(serde_json::to_value(insight).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize, Default)]
struct AnalyzeBody {
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    days: Option<u32>,
    #[serde(default)]
    max_insights: Option<usize>,
}

async fn analyze(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, body: Bytes) -> ApiResult<Json<Value>> {
    // Options are optional: an empty body means the defaults.
    let body: AnalyzeBody = if body.iter().all(u8::is_ascii_whitespace) {
        AnalyzeBody::default()
    } else {
        serde_json::from_slice(&body).map_err(ApiError::bad_request)?
    };
    let engine = match body.engine.as_deref() {
        None | Some("auto") => Engine::Auto,
        Some("claude") => Engine::Claude,
        Some("heuristics") => Engine::Heuristics,
        Some(other) => return Err(ApiError::bad_request(format!("unknown engine {other:?}"))),
    };
    let opts = trace_agent::Options {
        engine,
        days: body.days.unwrap_or(14).clamp(1, 365),
        max_insights: body.max_insights.unwrap_or(5).clamp(1, 20),
    };
    let report = crate::analysis::run(&state.pool, &site, state.claude.as_ref(), &opts).await.map_err(ApiError::bad_request)?;
    Ok(Json(serde_json::to_value(report).map_err(anyhow::Error::from)?))
}

async fn run_evaluate(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    let evals = evaluate::run(&state.pool, site.id, evaluate::Options::default()).await?;
    Ok(Json(serde_json::to_value(evals).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
struct FormatQuery {
    format: Option<String>,
}

/// Whether it's worth analysing now. For hooks, which should stay quiet
/// otherwise: `?format=text` gives the one-line message when it is and an
/// empty body when it isn't; `?format=claude-hook` the same as Claude Code's
/// hook output (`{"systemMessage": ...}`), which it shows to the person.
async fn get_analysis(
    State(state): State<AppStateRef>,
    Extension(CurrentSite(site)): Extension<CurrentSite>,
    Query(q): Query<FormatQuery>,
) -> ApiResult<Response> {
    let status = trace_core::readiness::status(&state.pool, &site).await?;
    match q.format.as_deref() {
        Some("text") => {
            let body = if status.ready { format!("Trace: {}\n", status.message) } else { String::new() };
            return Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response());
        }
        Some("claude-hook") => {
            let body = if status.ready { json!({ "systemMessage": format!("Trace: {}", status.message) }).to_string() } else { String::new() };
            return Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response());
        }
        _ => {}
    }
    Ok(Json(serde_json::to_value(status).map_err(anyhow::Error::from)?).into_response())
}

async fn list_runs(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, String, Option<String>, i64, Option<i64>, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, engine, model, started_at, finished_at, insights_created, error FROM analysis_runs
         WHERE project_id = $1 ORDER BY id DESC LIMIT 20",
    )
    .bind(site.id)
    .fetch_all(&state.pool)
    .await
    .map_err(anyhow::Error::from)?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, engine, model, started_at, finished_at, insights_created, error)| json!({
            "id": id, "engine": engine, "model": model, "started_at": started_at,
            "finished_at": finished_at, "insights_created": insights_created, "error": error,
        }))
        .collect::<Vec<_>>())))
}

// ---- MCP over Streamable HTTP ----------------------------------------------------

/// MCP for remote agents: JSON-RPC in, JSON out, authenticated with an API token
/// (`Authorization: Bearer trc_...`). Stateless, so no session id or event stream.
async fn mcp_http(State(state): State<AppStateRef>, headers: HeaderMap, body: Bytes) -> Response {
    // Browsers send Origin; a page on another site must not reach a local server (DNS rebinding).
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if privacy::referrer_origin(origin).is_none_or(|o| !o.ends_with(&format!("//{host}"))) {
            return (StatusCode::FORBIDDEN, "cross-origin MCP requests are not allowed").into_response();
        }
    }
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let principal = match token {
        Some(t) => crate::auth_api::token_principal(&state, t).await,
        None => None,
    };
    let Some(principal) = principal else {
        // Points the client at the sign-in, so it can open it for the person.
        return crate::oauth::challenge(&state, &headers, token.is_some());
    };
    let ctx = crate::mcp::Context {
        pool: &state.pool,
        account_id: principal.account(),
        claude: state.claude.as_ref(),
        actor: format!("mcp: {}", principal.actor()),
        role: principal.role(),
        read_only: principal.read_only(),
        origin: crate::oauth::origin(&state, &headers),
        state: Some(&state),
    };
    let replies = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Array(batch)) => {
            let mut out = Vec::new();
            for m in &batch {
                out.extend(crate::mcp::handle(&ctx, m).await);
            }
            (!out.is_empty()).then(|| Value::Array(out))
        }
        Ok(msg) => crate::mcp::handle(&ctx, &msg).await,
        Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })),
    };
    match replies {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(), // only notifications
    }
}

// ---- privacy rules ------------------------------------------------------------

/// What always applies, whatever rules a workspace adds. Shown on the Privacy page.
pub const BUILT_IN: &[&str] = &[
    "No session recordings or page snapshots: only discrete interactions are recorded",
    "Nothing people type is ever read, and page titles aren't recorded",
    "Text is read only from buttons, links and labels, never from inside forms with password or payment fields",
    "Every digit in page text is masked, so amounts, balances and account numbers never leave the browser",
    "Emails, tokens and API keys, card numbers, IBANs and phone numbers are replaced in everything sent",
    "URL query strings are dropped; ids, emails and tokens in paths are grouped",
    "Only the referrer's site is kept, never the full link",
    "Custom-event properties named like passwords, tokens, cards, emails, addresses or balances are dropped",
    "Everything is scrubbed in the browser and again on the server",
    "Raw events are deleted after the retention period; the analyst sees only aggregates",
];

async fn get_onboarding(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(trace_core::onboarding::checklist(&state.pool, &site).await?)))
}

async fn confirm_privacy_review(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
) -> ApiResult<StatusCode> {
    let actor = who.actor();
    trace_core::onboarding::confirm_privacy_review(&state.pool, site.id, &actor).await?;
    trace_core::auth::audit_site(&state.pool, site.account_id, Some(site.id), &actor, "onboarding.privacy_reviewed", &site.name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_privacy(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "built_in": BUILT_IN,
        "rules": privacy::list_rules(&state.pool, site.id).await?,
    })))
}

#[derive(Deserialize)]
struct RuleBody {
    kind: RuleKind,
    pattern: String,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    label: String,
}

async fn add_privacy_rule(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    let actor = who.actor();
    let id = privacy::add_rule(&state.pool, site.id, b.kind, &b.pattern, b.replacement.as_deref(), &b.label, &actor)
        .await
        .map_err(ApiError::bad_request)?;
    reload_rules(&state, site.id).await?;
    trace_core::auth::audit_site(&state.pool, site.account_id, Some(site.id), &actor, "privacy.rule_added", &format!("{} {}", b.kind.as_str(), b.pattern.trim())).await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct ToggleBody {
    enabled: bool,
}

async fn toggle_privacy_rule(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
    Path(id): Path<i64>,
    Json(b): Json<ToggleBody>,
) -> ApiResult<StatusCode> {
    if !privacy::set_rule_enabled(&state.pool, site.id, id, b.enabled).await? {
        return Err(ApiError::not_found("rule"));
    }
    reload_rules(&state, site.id).await?;
    let action = if b.enabled { "privacy.rule_enabled" } else { "privacy.rule_disabled" };
    trace_core::auth::audit_site(&state.pool, site.account_id, Some(site.id), &who.actor(), action, &format!("rule #{id}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_privacy_rule(
    State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>,
    Extension(who): Extension<crate::auth_api::Principal>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let rule = privacy::delete_rule(&state.pool, site.id, id).await?.ok_or_else(|| ApiError::not_found("rule"))?;
    reload_rules(&state, site.id).await?;
    trace_core::auth::audit_site(&state.pool, site.account_id, Some(site.id), &who.actor(), "privacy.rule_removed", &format!("{} {}", rule.kind.as_str(), rule.pattern)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TestBody {
    #[serde(default)]
    text: String,
    #[serde(default)]
    path: String,
}

/// Shows what the server would store for some sample text and a page path,
/// with the built-in protections and the current rules.
async fn test_privacy(State(state): State<AppStateRef>, Extension(CurrentSite(site)): Extension<CurrentSite>, Json(b): Json<TestBody>) -> ApiResult<Json<Value>> {
    let rules = site_rules(&state, site.id).await?;
    let path = b.path.trim();
    Ok(Json(json!({
        "page_text": privacy::redact_with(&rules, &b.text, true),
        "error_text": privacy::redact_with(&rules, &b.text, false),
        "path": (!path.is_empty()).then(|| privacy::redact_path(path)),
        "path_ignored": !path.is_empty() && (rules.ignores(path) || rules.ignores(&privacy::redact_path(path))),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripping_keeps_all_code_lines() {
        let src = "/*! header\n * more\n */\n(function () {\n  // a comment\n\n  var url = \"https://x\"; // trailing stays\n  /* one-line block */\n  return 1;\n})();\n";
        assert_eq!(
            strip_js(src).lines().skip(1).collect::<Vec<_>>(),
            ["(function () {", "var url = \"https://x\"; // trailing stays", "return 1;", "})();"]
        );
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;
    use trace_core::auth::{self, Role};
    use tower::ServiceExt;

    /// Every endpoint about one website, as (method, path below the website).
    const SITE_ROUTES: &[(&str, &str)] = &[
        ("GET", "project"), ("GET", "config"), ("POST", "config"), ("GET", "digest"), ("GET", "kpis"),
        ("GET", "insights"), ("GET", "insights/1"), ("GET", "insights/1/prompt"), ("POST", "insights/1/status"), ("POST", "insights/1/kpi"), ("POST", "releases"),
        ("POST", "analyze"), ("POST", "evaluate"), ("GET", "runs"), ("GET", "privacy"), ("POST", "privacy/rules"),
        ("POST", "privacy/rules/1"), ("DELETE", "privacy/rules/1"), ("POST", "privacy/test"),
        ("GET", "onboarding"), ("POST", "onboarding/privacy-reviewed"), ("GET", "analysis"),
    ];

    async fn app() -> (tower::util::MapRequest<Router, fn(Request) -> Request>, AppStateRef, String) {
        let path = trace_core::db::test_target("api").await;
        let pool = db::connect(&path).await.unwrap();
        let state = Arc::new(AppState::new(pool, None));
        let router = router(state.clone());
        (tower::Layer::layer(&tower::util::MapRequestLayer::new(route_site_paths as fn(Request) -> Request), router), state, path)
    }

    async fn person(state: &AppState, account: i64, email: &str, role: Role) -> String {
        let user = auth::create_user(&state.pool, account, email, "", "a long enough password", role).await.unwrap();
        format!("{}={}", crate::auth_api::COOKIE, auth::create_session(&state.pool, user.id, true, None).await.unwrap())
    }

    async fn call<S>(app: &S, method: &str, path: &str, cookie: &str) -> (StatusCode, HeaderMap)
    where
        S: tower::Service<Request, Response = Response, Error = std::convert::Infallible> + Clone,
    {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(r#"{"status":"selected","kpi_id":null,"kind":"drop_prop","pattern":"x","enabled":false}"#))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        (res.status(), res.headers().clone())
    }

    #[tokio::test]
    async fn no_account_can_reach_another_accounts_website() {
        let (app, state, path) = app().await;
        let (acme, acme_site) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let (bolt, bolt_site) = db::create_account_with_site(&state.pool, "Bolt", None).await.unwrap();
        let _ = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let bolt_admin = person(&state, bolt.id, "b@bolt.example", Role::Owner).await;

        for (method, route) in SITE_ROUTES {
            let (status, _) = call(&app, method, &format!("/api/sites/{}/{route}", acme_site.slug), &bolt_admin).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {route} on another account's website");
        }
        // Their own website works under the same paths.
        let (status, _) = call(&app, "GET", &format!("/api/sites/{}/project", bolt_site.slug), &bolt_admin).await;
        assert_eq!(status, StatusCode::OK);
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn paths_without_a_website() {
        let (app, state, path) = app().await;
        let (acme, _) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let owner = person(&state, acme.id, "a@acme.example", Role::Owner).await;

        let (status, headers) = call(&app, "GET", "/api/kpis", &owner).await;
        assert_eq!(status, StatusCode::OK, "one website: the old path still works");
        assert_eq!(headers.get("deprecation").map(|v| v.to_str().unwrap()), Some("true"));
        let (status, headers) = call(&app, "GET", "/api/sites/acme/kpis", &owner).await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("deprecation").is_none(), "naming the website isn't deprecated");

        db::create_site(&state.pool, acme.id, "Docs").await.unwrap();
        let (status, _) = call(&app, "GET", "/api/kpis", &owner).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "two websites: say which");
        let (status, _) = call(&app, "GET", "/api/sites/docs/kpis", &owner).await;
        assert_eq!(status, StatusCode::OK);
        trace_core::db::drop_test_target(&path).await;
    }

    async fn send<S>(app: &S, req: Request) -> (StatusCode, Value)
    where
        S: tower::Service<Request, Response = Response, Error = std::convert::Infallible> + Clone,
    {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn json_req(method: &str, path: &str, auth: (&str, &str), body: Value) -> Request {
        Request::builder()
            .method(method)
            .uri(path)
            .header(auth.0, auth.1)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn read_only_tokens_change_nothing() {
        let (app, state, path) = app().await;
        let (acme, _) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let _ = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let owner = auth::find_user(&state.pool, "a@acme.example").await.unwrap().unwrap();
        let (_, token) = auth::create_api_token(&state.pool, acme.id, &owner, "agent", true).await.unwrap();
        let bearer = format!("Bearer {token}");
        for (method, route) in SITE_ROUTES.iter().filter(|(m, _)| *m != "GET") {
            let (status, body) = send(&app, json_req(method, &format!("/api/sites/acme/{route}"), ("authorization", &bearer), json!({}))).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {route}");
            assert_eq!(body["code"], "read_only");
        }
        let (status, _) = send(&app, json_req("GET", "/api/sites/acme/kpis", ("authorization", &bearer), json!({}))).await;
        assert_eq!(status, StatusCode::OK, "reads still work");
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn sign_up_only_when_enabled() {
        let signup = json!({ "account": "Newco", "email": "new@newco.example", "name": "N", "password": "a long enough password" });
        let (app, state, path) = app().await;
        let (status, body) = send(&app, json_req("POST", "/api/auth/signup", ("x-none", ""), signup.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body["error"].as_str().unwrap().contains("by invitation"));
        assert!(db::accounts(&state.pool).await.unwrap().is_empty());
        trace_core::db::drop_test_target(&path).await;

        let path = trace_core::db::test_target("signup").await;
        let pool = db::connect(&path).await.unwrap();
        trace_core::plans::set_default(&pool, "free").await.unwrap();
        let mut st = AppState::new(pool, None);
        st.allow_signup = true;
        let state = Arc::new(st);
        let app = tower::Layer::layer(&tower::util::MapRequestLayer::new(route_site_paths as fn(Request) -> Request), router(state.clone()));
        let (status, body) = send(&app, json_req("POST", "/api/auth/signup", ("x-none", ""), signup)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["user"]["role"], "owner");
        let account = body["account"]["id"].as_i64().unwrap();
        assert_eq!(trace_core::plans::for_account(&state.pool, account).await.unwrap().plan.key, "free");
        assert_eq!(db::sites(&state.pool, account).await.unwrap()[0].slug, "newco", "with a first website");
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn removing_a_website_keeps_the_others() {
        let (app, state, path) = app().await;
        let (acme, shop) = db::create_account_with_site(&state.pool, "Shop", None).await.unwrap();
        let docs = db::create_site(&state.pool, acme.id, "Docs").await.unwrap();
        for site in [shop.id, docs.id] {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type) VALUES ($1, 's', 1, 'pageview')").bind(site).execute(&state.pool).await.unwrap();
        }
        let admin = person(&state, acme.id, "a@acme.example", Role::Admin).await;
        let (status, _) = send(&app, json_req("DELETE", "/api/sites/docs", ("cookie", &admin), json!({ "confirm": "nope" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "needs the name to confirm");
        let (status, _) = send(&app, json_req("DELETE", "/api/sites/docs", ("cookie", &admin), json!({ "confirm": "Docs" }))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let left: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT project_id FROM events").fetch_all(&state.pool).await.unwrap();
        assert_eq!(left, [shop.id]);
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn each_website_gets_its_own_collector() {
        let (app, state, path) = app().await;
        let (acme, shop) = db::create_account_with_site(&state.pool, "Shop", None).await.unwrap();
        let docs = db::create_site(&state.pool, acme.id, "Docs").await.unwrap();
        privacy::add_rule(&state.pool, shop.id, RuleKind::IgnorePath, "/admin/*", None, "", "test").await.unwrap();
        let get = |k: String| {
            let app = app.clone();
            async move {
                let res = app.oneshot(Request::builder().uri(format!("/d.js?k={k}")).body(axum::body::Body::empty()).unwrap()).await.unwrap();
                let etag = res.headers()[header::ETAG].to_str().unwrap().to_string();
                (String::from_utf8(axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap().to_vec()).unwrap(), etag)
            }
        };
        let (shop_js, shop_etag) = get(shop.write_key.clone()).await;
        let (docs_js, docs_etag) = get(docs.write_key.clone()).await;
        assert!(shop_js.contains(r#""ignore":["/admin/*"]"#));
        assert!(!docs_js.contains("/admin/*") && docs_js.contains(r#""ignore":[]"#));
        assert_ne!(shop_etag, docs_etag);
        for (name, js) in [("shop", &shop_js), ("docs", &docs_js)] {
            let f = std::env::temp_dir().join(format!("collector-{name}-{}.js", trace_core::util::new_key()));
            std::fs::write(&f, js).unwrap();
            if let Ok(out) = std::process::Command::new("node").arg("--check").arg(&f).output() {
                assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
            }
            let _ = std::fs::remove_file(f);
        }
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn events_over_the_monthly_limit_are_dropped_but_answered() {
        let (app, state, path) = app().await;
        let (acme, shop) = db::create_account_with_site(&state.pool, "Shop", None).await.unwrap();
        trace_core::plans::set_overrides(&state.pool, acme.id, &[("monthly_events".to_string(), Some(5))].into_iter().collect()).await.unwrap();
        let batch = |n: usize| {
            let events: Vec<Value> = (0..n).map(|_| json!({ "type": "pageview", "path": "/" })).collect();
            json!({ "key": shop.write_key, "sid": "s1", "events": events })
        };
        for n in [3, 4, 2] {
            let req = Request::builder().method("POST").uri("/v1/collect").body(axum::body::Body::from(batch(n).to_string())).unwrap();
            assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::NO_CONTENT);
        }
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE project_id = $1").bind(shop.id).fetch_one(&state.pool).await.unwrap();
        assert_eq!(stored, 5);
        let u = trace_core::usage::get(&state.pool, acme.id, &trace_core::usage::this_month()).await.unwrap();
        assert_eq!((u.events, u.events_dropped), (5, 4));
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn mcp_clients_sign_people_in() {
        let (app, state, path) = app().await;
        let (acme, _) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let owner = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let raw = |req: Request| {
            let app = app.clone();
            async move { app.oneshot(req).await.unwrap() }
        };
        let mcp = |auth: Option<String>| {
            let mut b = Request::builder().method("POST").uri("/mcp").header(header::CONTENT_TYPE, "application/json");
            if let Some(a) = auth {
                b = b.header(header::AUTHORIZATION, a);
            }
            b.body(axum::body::Body::from(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "list_sites", "arguments": {} } }).to_string())).unwrap()
        };

        // Without a token, /mcp says where to sign in.
        let res = raw(mcp(None)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let challenge = res.headers()[header::WWW_AUTHENTICATE].to_str().unwrap().to_string();
        assert!(challenge.contains("resource_metadata=\"http://localhost/.well-known/oauth-protected-resource\""), "{challenge}");
        let get = |uri: &str| Request::builder().uri(uri).header(header::HOST, "localhost").body(axum::body::Body::empty()).unwrap();
        let (_, meta) = send(&app, get("/.well-known/oauth-authorization-server")).await;
        assert_eq!((meta["token_endpoint"].as_str(), meta["code_challenge_methods_supported"][0].as_str()), (Some("http://localhost/oauth/token"), Some("S256")));

        // The client registers itself.
        let cb = "http://127.0.0.1:53412/callback";
        let (status, reg) = send(&app, json_req("POST", "/oauth/register", ("x-none", "1"), json!({ "client_name": "Claude Code", "redirect_uris": [cb] }))).await;
        assert_eq!(status, StatusCode::CREATED, "{reg}");
        let client_id = reg["client_id"].as_str().unwrap().to_string();
        let (status, _) = send(&app, json_req("POST", "/oauth/register", ("x-none", "1"), json!({ "redirect_uris": ["http://evil.example/cb"] }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Authorizing hands over to the dashboard's Connect screen; a wrong redirect is never followed.
        let (verifier, challenge) = ("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let q = format!("response_type=code&client_id={client_id}&redirect_uri=http%3A%2F%2F127.0.0.1%3A53412%2Fcallback&code_challenge={challenge}&code_challenge_method=S256&state=xyz");
        let res = raw(get(&format!("/oauth/authorize?{q}"))).await;
        assert_eq!(res.headers()[header::LOCATION].to_str().unwrap(), format!("/#/connect?{q}"));
        let res = raw(get(&format!("/oauth/authorize?{}", q.replace("53412", "9999")))).await;
        assert!(res.status() == StatusCode::BAD_REQUEST && res.headers().get(header::LOCATION).is_none());

        // Only a signed-in person approves; a token can't.
        let approval = json!({ "client_id": client_id, "redirect_uri": cb, "code_challenge": challenge, "code_challenge_method": "S256", "state": "xyz", "approve": true });
        let (_, described) = send(&app, json_req("GET", &format!("/api/account/connect?client_id={client_id}&redirect_uri={}", "http%3A%2F%2F127.0.0.1%3A53412%2Fcallback"), ("cookie", &owner), json!({}))).await;
        assert_eq!((described["client_name"].as_str(), described["back_to"].as_str()), (Some("Claude Code"), Some("127.0.0.1:53412")));
        let (status, approved) = send(&app, json_req("POST", "/api/account/connect", ("cookie", &owner), approval.clone())).await;
        assert_eq!(status, StatusCode::OK, "{approved}");
        let redirect = approved["redirect"].as_str().unwrap();
        assert!(redirect.starts_with("http://127.0.0.1:53412/callback?code=") && redirect.ends_with("&state=xyz"), "{redirect}");
        let code = redirect.split("code=").nth(1).unwrap().split('&').next().unwrap().to_string();

        // The code becomes a token once, with the verifier.
        let exchange = |verifier: &str| {
            Request::builder().method("POST").uri("/oauth/token").header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(axum::body::Body::from(format!("grant_type=authorization_code&code={code}&client_id={client_id}&redirect_uri=http%3A%2F%2F127.0.0.1%3A53412%2Fcallback&code_verifier={verifier}")))
                .unwrap()
        };
        let (status, tok) = send(&app, exchange(verifier)).await;
        assert_eq!(status, StatusCode::OK, "{tok}");
        let access = tok["access_token"].as_str().unwrap().to_string();
        let (status, again) = send(&app, exchange(verifier)).await;
        assert_eq!((status, again["error"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_grant")));

        // The token works on /mcp and shows under Account.
        let res = raw(mcp(Some(format!("Bearer {access}")))).await;
        assert_eq!(res.status(), StatusCode::OK);
        let (status, _) = send(&app, json_req("POST", "/api/account/connect", ("authorization", &format!("Bearer {access}")), approval)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "a token can't approve another connection");
        let tokens = auth::list_api_tokens(&state.pool, acme.id, None).await.unwrap();
        assert!(tokens.iter().any(|t| t.name.starts_with("Claude Code (signed in ")), "{:?}", tokens.iter().map(|t| &t.name).collect::<Vec<_>>());
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn deploys_mark_insights_released_at_their_time() {
        let (app, state, path) = app().await;
        let (acme, shop) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let owner = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let mut ids = Vec::new();
        for (title, status) in [("Ship me", "in_progress"), ("Not this", "dismissed")] {
            let id: i64 = sqlx::query_scalar("INSERT INTO insights (project_id, title, problem, proposal, source, status, created_at, updated_at) VALUES ($1, $2, 'p', 'x', 'test', $3, 1, 1) RETURNING id")
                .bind(shop.id).bind(title).bind(status).fetch_one(&state.pool).await.unwrap();
            ids.push(id);
        }
        let url = format!("/api/sites/{}/releases", shop.slug);
        let (status, body) = send(&app, json_req("POST", &url, ("cookie", &owner), json!({ "insights": [ids[0], ids[1], 999], "at": "2026-10-01T09:30:00Z", "ref": "abc123" }))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["released"], json!([ids[0]]));
        let reasons: Vec<&str> = body["skipped"].as_array().unwrap().iter().map(|s| s["reason"].as_str().unwrap()).collect();
        assert_eq!(reasons, ["dismissed", "no such insight"]);
        let shipped = insights::get(&state.pool, shop.id, ids[0]).await.unwrap().unwrap();
        assert_eq!((shipped.status, shipped.released_at), (InsightStatus::Released, trace_core::util::parse_time("2026-10-01T09:30:00Z")));

        // The same deploy reported again changes nothing; a bad time is refused.
        let (_, again) = send(&app, json_req("POST", &url, ("cookie", &owner), json!({ "insights": [ids[0]], "at": "2026-10-01T09:30:00Z" }))).await;
        assert!(again["skipped"][0]["reason"].as_str().unwrap().starts_with("already released"), "{again}");
        let (status, _) = send(&app, json_req("POST", &url, ("cookie", &owner), json!({ "insights": [ids[0]], "at": "last tuesday" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn operator_area_is_for_operators_with_two_step() {
        let (app, state, path) = app().await;
        let (acme, shop) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        sqlx::query("INSERT INTO insights (project_id, title, problem, proposal, source, created_at, updated_at) VALUES ($1, 'SECRET INSIGHT', 'p', 'x', 'test', 1, 1)")
            .bind(shop.id)
            .execute(&state.pool)
            .await
            .unwrap();
        let owner = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let (status, body) = send(&app, json_req("GET", "/api/operator/overview", ("cookie", &owner), json!({}))).await;
        assert_eq!((status, body["code"].as_str()), (StatusCode::FORBIDDEN, Some("not_operator")), "an owner isn't an operator");

        auth::set_operator(&state.pool, "a@acme.example", true).await.unwrap();
        let (status, body) = send(&app, json_req("GET", "/api/operator/overview", ("cookie", &owner), json!({}))).await;
        assert_eq!((status, body["code"].as_str()), (StatusCode::FORBIDDEN, Some("operator_mfa")), "operators need two-step first");

        sqlx::query("UPDATE users SET totp_enabled_at = 1, totp_secret = 'AAAA' WHERE email = 'a@acme.example'").execute(&state.pool).await.unwrap();
        let (status, overview) = send(&app, json_req("GET", "/api/operator/overview", ("cookie", &owner), json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(overview["accounts"][0]["plan"], "unlimited");
        let (status, detail) = send(&app, json_req("GET", &format!("/api/operator/accounts/{}", acme.id), ("cookie", &owner), json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(detail["sites"][0]["slug"], "acme");
        let everything = format!("{overview}{detail}");
        assert!(!everything.contains("SECRET INSIGHT") && !everything.contains("kpis") && !everything.contains("events\":["), "no analytics in the operator area");

        let user = auth::find_user(&state.pool, "a@acme.example").await.unwrap().unwrap();
        let (_, token) = auth::create_api_token(&state.pool, acme.id, &user, "t", false).await.unwrap();
        let (status, _) = send(&app, json_req("GET", "/api/operator/overview", ("authorization", &format!("Bearer {token}")), json!({}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "never with a token");
        trace_core::db::drop_test_target(&path).await;
    }

    /// Another account's token, and its owner's session, against every route
    /// and MCP tool: nothing of Acme's is reachable, changeable or visible.
    #[tokio::test]
    async fn another_accounts_token_reaches_nothing() {
        let (app, state, path) = app().await;
        let (acme, acme_site) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let (bolt, bolt_site) = db::create_account_with_site(&state.pool, "Bolt", None).await.unwrap();
        let _ = person(&state, acme.id, "a@acme.example", Role::Owner).await;
        let acme_owner = auth::find_user(&state.pool, "a@acme.example").await.unwrap().unwrap();
        let (acme_token, _) = auth::create_api_token(&state.pool, acme.id, &acme_owner, "acme agent", false).await.unwrap();
        let (acme_invite, _) = auth::create_invite(&state.pool, acme.id, "new@acme.example", Role::User, Some(acme_owner.id)).await.unwrap();
        let acme_insight: i64 = sqlx::query_scalar(
            "INSERT INTO insights (project_id, title, problem, proposal, source, created_at, updated_at) VALUES ($1, 'ACME SECRET', 'p', 'x', 'test', 1, 1) RETURNING id",
        )
        .bind(acme_site.id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        let acme_rule = privacy::add_rule(&state.pool, acme_site.id, RuleKind::IgnorePath, "/acme-only/*", None, "", "test").await.unwrap();
        auth::audit(&state.pool, acme.id, "a@acme.example", "test", "ACME SECRET").await.unwrap();

        let bolt_cookie = person(&state, bolt.id, "b@bolt.example", Role::Owner).await;
        let bolt_owner = auth::find_user(&state.pool, "b@bolt.example").await.unwrap().unwrap();
        let (_, token) = auth::create_api_token(&state.pool, bolt.id, &bolt_owner, "bolt agent", false).await.unwrap();
        let bearer = format!("Bearer {token}");
        let body = json!({ "status": "selected", "kpi_id": null, "enabled": false, "role": "admin", "confirm": "Acme", "kind": "drop_prop", "pattern": "x" });
        let unchanged = |what: &str| format!("{what} of Acme's changed");

        // Acme's website, by path or by header.
        for (method, route) in SITE_ROUTES {
            let route = route.replace("insights/1", &format!("insights/{acme_insight}")).replace("rules/1", &format!("rules/{acme_rule}"));
            for auth in [("authorization", bearer.as_str()), ("cookie", bolt_cookie.as_str())] {
                let (status, _) = send(&app, json_req(method, &format!("/api/sites/{}/{route}", acme_site.slug), auth, body.clone())).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{method} {route} with {}", auth.0);
                let mut req = json_req(method, &format!("/api/{route}"), auth, body.clone());
                req.headers_mut().insert(crate::auth_api::SITE_HEADER, acme_site.slug.parse().unwrap());
                assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND, "{method} {route} with the header");
            }
        }
        // Acme's ids through Bolt's own website.
        for (method, route) in SITE_ROUTES.iter().filter(|(_, r)| r.contains('/')) {
            let route = route.replace("insights/1", &format!("insights/{acme_insight}")).replace("rules/1", &format!("rules/{acme_rule}"));
            let (status, body_out) = send(&app, json_req(method, &format!("/api/sites/{}/{route}", bolt_site.slug), ("authorization", &bearer), body.clone())).await;
            assert!(!status.is_success() || !body_out.to_string().contains("ACME"), "{method} {route} through Bolt's website: {status} {body_out}");
        }

        // Account-wide routes (people-only ones with the session).
        for route in ["sites", "plan", "account", "account/tokens", "team", "team/tokens", "audit"] {
            for auth in [("authorization", bearer.as_str()), ("cookie", bolt_cookie.as_str())] {
                let (_, out) = send(&app, json_req("GET", &format!("/api/{route}"), auth, json!({}))).await;
                let text = out.to_string();
                assert!(!text.contains("acme") && !text.contains("ACME"), "GET {route} with {}: {text}", auth.0);
            }
        }
        let attempts = [
            ("DELETE", format!("/api/sites/{}", acme_site.slug)),
            ("DELETE", format!("/api/team/users/{}", acme_owner.id)),
            ("POST", format!("/api/team/users/{}/role", acme_owner.id)),
            ("POST", format!("/api/team/users/{}/reset-mfa", acme_owner.id)),
            ("DELETE", format!("/api/team/invites/{}", acme_invite.id)),
            ("DELETE", format!("/api/team/tokens/{}", acme_token.id)),
            ("DELETE", format!("/api/account/tokens/{}", acme_token.id)),
        ];
        for (method, uri) in &attempts {
            let (status, _) = send(&app, json_req(method, uri, ("cookie", &bolt_cookie), body.clone())).await;
            assert!(!status.is_success(), "{method} {uri}: {status}");
        }
        assert_eq!(db::sites(&state.pool, acme.id).await.unwrap().len(), 1, "{}", unchanged("websites"));
        assert_eq!(auth::list_users(&state.pool, acme.id).await.unwrap()[0].role, Role::Owner, "{}", unchanged("people"));
        assert_eq!(auth::pending_invites(&state.pool, acme.id).await.unwrap().len(), 1, "{}", unchanged("invites"));
        let tokens: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_tokens WHERE account_id = $1 AND revoked_at IS NULL").bind(acme.id).fetch_one(&state.pool).await.unwrap();
        assert_eq!(tokens, 1, "{}", unchanged("tokens"));
        let insight: String = sqlx::query_scalar("SELECT status FROM insights WHERE id = $1").bind(acme_insight).fetch_one(&state.pool).await.unwrap();
        assert_eq!(insight, "new", "{}", unchanged("insights"));
        let rule: db::Flag = sqlx::query_scalar("SELECT enabled FROM privacy_rules WHERE id = $1").bind(acme_rule).fetch_one(&state.pool).await.unwrap();
        assert!(rule.0, "{}", unchanged("privacy rules"));

        // Every MCP tool, pointed at Acme's website and ids.
        let rpc = |id: i64, method: &str, params: Value| {
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, &bearer)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()))
                .unwrap()
        };
        let (_, list) = send(&app, rpc(1, "tools/list", json!({}))).await;
        let tools = list["result"]["tools"].as_array().unwrap().clone();
        assert!(tools.len() >= 12);
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            let takes_site = tool["inputSchema"]["properties"]["site"].is_object();
            let args = json!({ "site": acme_site.slug, "id": acme_insight, "status": "selected", "json": "{}" });
            let (_, out) = send(&app, rpc(2, "tools/call", json!({ "name": name, "arguments": args }))).await;
            let text = out.to_string();
            assert!(!text.contains("ACME") && !text.contains("acme-only"), "{name}: {text}");
            if takes_site {
                assert_eq!(out["result"]["isError"], true, "{name} reached Acme's website: {text}");
            }
            // And Acme's ids through Bolt's own website.
            let args = json!({ "site": bolt_site.slug, "id": acme_insight, "status": "selected" });
            let (_, out) = send(&app, rpc(3, "tools/call", json!({ "name": name, "arguments": args }))).await;
            assert!(!out.to_string().contains("ACME"), "{name} through Bolt's website: {out}");
        }
        let insight: String = sqlx::query_scalar("SELECT status FROM insights WHERE id = $1").bind(acme_insight).fetch_one(&state.pool).await.unwrap();
        assert_eq!(insight, "new", "{}", unchanged("insights (MCP)"));

        // The collector: Acme's rules never reach Bolt's script, and a write key only writes its own website.
        let js = |k: String| {
            let app = app.clone();
            async move {
                let res = app.oneshot(Request::builder().uri(format!("/d.js?k={k}")).body(axum::body::Body::empty()).unwrap()).await.unwrap();
                String::from_utf8(axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap().to_vec()).unwrap()
            }
        };
        assert!(js(acme_site.write_key.clone()).await.contains("/acme-only/*"));
        assert!(!js(bolt_site.write_key.clone()).await.contains("acme-only"));
        let batch = json!({ "key": bolt_site.write_key, "project_id": acme_site.id, "sid": "s", "events": [{ "type": "pageview", "path": "/" }] });
        let req = Request::builder().method("POST").uri("/v1/collect").body(axum::body::Body::from(batch.to_string())).unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::NO_CONTENT);
        let acme_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE project_id = $1").bind(acme_site.id).fetch_one(&state.pool).await.unwrap();
        assert_eq!(acme_events, 0);
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn only_admins_confirm_the_privacy_review() {
        let (app, state, path) = app().await;
        let (acme, site) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let user = person(&state, acme.id, "u@acme.example", Role::User).await;
        let admin = person(&state, acme.id, "a@acme.example", Role::Admin).await;
        let checklist = |cookie: String| {
            let app = app.clone();
            async move { send(&app, json_req("GET", "/api/sites/acme/onboarding", ("cookie", &cookie), json!({}))).await }
        };
        let (status, before) = checklist(user.clone()).await;
        assert_eq!(status, StatusCode::OK, "users can read the checklist");
        assert_eq!(before["steps"][3]["key"], "privacy");
        assert_eq!(before["steps"][3]["done"], false);

        let (status, _) = send(&app, json_req("POST", "/api/sites/acme/onboarding/privacy-reviewed", ("cookie", &user), json!({}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(&app, json_req("POST", "/api/sites/acme/onboarding/privacy-reviewed", ("cookie", &admin), json!({}))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, after) = checklist(user).await;
        assert_eq!(after["steps"][3]["done"], true);
        let logged: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'onboarding.privacy_reviewed' AND project_id = $1")
            .bind(site.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(logged, 1);
        trace_core::db::drop_test_target(&path).await;
    }

    #[test]
    fn only_the_cli_grants_operator_rights() {
        // No HTTP or MCP code may call set_operator or write the is_operator column.
        for file in ["api.rs", "auth_api.rs", "operator.rs", "mcp.rs"] {
            let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap();
            let code = src.split("#[cfg(test)]").next().unwrap();
            assert!(!code.contains("set_operator") && !code.contains("is_operator ="), "{file} must not grant operator rights");
        }
    }

    async fn post_json<S>(app: &S, path: &str, body: Value, cookie: &str) -> StatusCode
    where
        S: tower::Service<Request, Response = Response, Error = std::convert::Infallible> + Clone,
    {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn only_the_owner_acts_on_the_owner() {
        let (app, state, path) = app().await;
        let (acme, _) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        let owner = person(&state, acme.id, "o@acme.example", Role::Owner).await;
        let admin = person(&state, acme.id, "a@acme.example", Role::Admin).await;
        let owner_id = auth::find_user(&state.pool, "o@acme.example").await.unwrap().unwrap().id;
        let admin_id = auth::find_user(&state.pool, "a@acme.example").await.unwrap().unwrap().id;
        let reset = |id: i64| format!("/api/team/users/{id}/reset-mfa");
        assert_eq!(post_json(&app, &reset(owner_id), json!({}), &admin).await, StatusCode::FORBIDDEN, "an admin can't reset the owner");
        assert_eq!(post_json(&app, &reset(admin_id), json!({}), &owner).await, StatusCode::NO_CONTENT, "the owner can reset an admin");
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn password_guessing_is_limited_per_email_and_per_address() {
        let (app, state, path) = app().await;
        let (acme, _) = db::create_account_with_site(&state.pool, "Acme", None).await.unwrap();
        for e in ["vic@acme.example", "eve@acme.example"] {
            auth::create_user(&state.pool, acme.id, e, "", "the right password", Role::User).await.unwrap();
        }
        let login = |email: &str, password: &str| post_json(&app, "/api/auth/login", json!({ "email": email, "password": password }), "");
        for _ in 0..10 {
            assert_eq!(login("vic@acme.example", "wrong").await, StatusCode::UNAUTHORIZED);
        }
        assert_eq!(login("vic@acme.example", "the right password").await, StatusCode::TOO_MANY_REQUESTS, "vic waits 15 minutes");
        assert_eq!(login("eve@acme.example", "the right password").await, StatusCode::OK, "other people aren't affected");
        // Spraying many emails from one address is stopped too (30 failures in all).
        for i in 0..20 {
            login(&format!("nobody{i}@acme.example"), "wrong").await;
        }
        assert_eq!(login("eve@acme.example", "the right password").await, StatusCode::TOO_MANY_REQUESTS);
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn the_collector_refuses_floods_from_one_address() {
        let (app, _state, path) = app().await;
        let mut statuses = Vec::new();
        for _ in 0..=BATCHES_PER_ADDRESS {
            let req = Request::builder().method("POST").uri("/v1/collect").body(axum::body::Body::from("not json")).unwrap();
            statuses.push(app.clone().oneshot(req).await.unwrap().status());
        }
        assert!(statuses[..BATCHES_PER_ADDRESS as usize].iter().all(|s| *s == StatusCode::BAD_REQUEST));
        assert_eq!(statuses.last(), Some(&StatusCode::TOO_MANY_REQUESTS), "refused before parsing");
        trace_core::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn security_headers() {
        let (app, _state, path) = app().await;
        let get = |uri: &'static str| {
            let app = app.clone();
            async move { app.oneshot(Request::builder().uri(uri).body(axum::body::Body::empty()).unwrap()).await.unwrap() }
        };
        let page = get("/").await;
        let csp = page.headers().get(header::CONTENT_SECURITY_POLICY).unwrap().to_str().unwrap().to_string();
        assert!(csp.contains("frame-ancestors 'none'") && csp.contains("'sha256-"), "{csp}");
        assert_eq!(page.headers().get("x-frame-options").unwrap(), "DENY");
        assert_eq!(page.headers().get("x-content-type-options").unwrap(), "nosniff");
        let collector = get("/d.js").await;
        assert_eq!(collector.headers().get("x-content-type-options").unwrap(), "nosniff");
        assert!(collector.headers().get("x-frame-options").is_none(), "the collector runs on other people's sites");
        trace_core::db::drop_test_target(&path).await;
    }
}
