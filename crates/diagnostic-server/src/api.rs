use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use diagnostic_agent::{ClaudeConfig, Engine};
use diagnostic_core::model::IdeaStatus;
use diagnostic_core::privacy::{self, HostRules, RuleKind};
use diagnostic_core::{SqlitePool, config, db, digest, evaluate, ideas, ingest, kpi};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};

pub struct AppState {
    pub pool: SqlitePool,
    pub project_id: i64,
    /// Legacy shared token; API tokens created in the dashboard replace it.
    pub admin_token: Option<String>,
    pub claude: Option<ClaudeConfig>,
    /// One-time code for creating the owner account; None once one exists.
    pub setup_code: std::sync::Mutex<Option<String>>,
    pub limiter: crate::auth_api::LoginLimiter,
    /// Mark the session cookie Secure (the dashboard is served over HTTPS).
    pub secure_cookies: bool,
    /// The workspace's privacy rules, compiled; see [`reload_rules`].
    pub rules: std::sync::RwLock<Arc<HostRules>>,
    /// The served collector, built lazily with the current rules.
    pub collector: std::sync::RwLock<Option<Arc<Collector>>>,
}

pub type AppStateRef = Arc<AppState>;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const STYLE_CSS: &str = include_str!("../assets/style.css");
const COLLECTOR_JS: &str = include_str!("../assets/d.js");

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
        .route("/ideas", get(list_ideas))
        .route("/ideas/{id}", get(get_idea))
        .route("/ideas/{id}/prompt", get(get_prompt))
        .route("/ideas/{id}/status", post(set_status))
        .route("/ideas/{id}/kpi", post(set_kpi))
        .route("/analyze", post(analyze))
        .route("/evaluate", post(run_evaluate))
        .route("/runs", get(list_runs))
        .route("/privacy", get(get_privacy))
        .route("/privacy/rules", post(add_privacy_rule))
        .route("/privacy/rules/{id}", post(toggle_privacy_rule).delete(delete_privacy_rule))
        .route("/privacy/test", post(test_privacy))
        .merge(crate::auth_api::routes())
        .layer(middleware::from_fn_with_state(state.clone(), crate::auth_api::authorize));

    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/assets/app.js", get(|| async { asset("application/javascript; charset=utf-8", APP_JS, "no-cache") }))
        .route("/assets/style.css", get(|| async { asset("text/css; charset=utf-8", STYLE_CSS, "no-cache") }))
        .nest("/api", api)
        .route("/mcp", post(mcp_http).get(|| async { (StatusCode::METHOD_NOT_ALLOWED, "this MCP endpoint takes POST requests") }))
        .merge(collector)
        .with_state(state)
        // Brotli or gzip, whichever the client accepts: the collector goes from ~32 KB to ~6 KB on the wire.
        .layer(CompressionLayer::new())
}

/// The collector as served: the commented source in the repo, minus whole-line
/// comments, blank lines and indentation. It's ES5 with no multi-line strings,
/// so this is safe without a full minifier; newlines are kept for ASI.
fn strip_js(src: &str) -> String {
    let mut out = String::from("/*! Diagnostic collector | diagnostic.dev | readable source: crates/diagnostic-server/assets/d.js */\n");
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

/// The stripped collector with the workspace's privacy rules written in,
/// compressed once at maximum quality (too slow to do per request), plus a
/// hash for its ETag. Rebuilt when an admin changes a rule.
pub struct Collector {
    plain: Vec<u8>,
    brotli: Vec<u8>,
    gzip: Vec<u8>,
    hash: String,
}

const RULES_PLACEHOLDER: &str = "/*__DIAGNOSTIC_RULES__*/ null";

fn build_collector(rules: &HostRules) -> Collector {
    use std::io::Write;
    let js = strip_js(COLLECTOR_JS);
    debug_assert!(js.contains(RULES_PLACEHOLDER), "d.js lost its rules placeholder");
    let plain = js.replacen(RULES_PLACEHOLDER, &rules.client_json().to_string(), 1).into_bytes();
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

/// Re-reads the privacy rules after a change: ingest uses the new ones at once,
/// and the next request for d.js gets a collector with them built in.
pub async fn reload_rules(state: &AppState) -> anyhow::Result<()> {
    let rules = Arc::new(HostRules::load(&state.pool, state.project_id).await?);
    *state.rules.write().unwrap() = rules;
    *state.collector.write().unwrap() = None;
    Ok(())
}

fn collector(state: &AppState) -> Arc<Collector> {
    if let Some(c) = state.collector.read().unwrap().clone() {
        return c;
    }
    let rules = state.rules.read().unwrap().clone();
    let c = Arc::new(build_collector(&rules));
    *state.collector.write().unwrap() = Some(c.clone());
    c
}

/// Cached for an hour, then revalidated cheaply with If-None-Match, so sites
/// pick up collector updates (and privacy rule changes) without changing their snippet.
async fn collector_js(State(state): State<AppStateRef>, headers: HeaderMap) -> Response {
    let c = collector(&state);
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

async fn collect(State(state): State<AppStateRef>, headers: HeaderMap, body: Bytes) -> ApiResult<StatusCode> {
    // The collector posts JSON as text/plain so browsers skip the CORS preflight.
    let batch: ingest::Batch = serde_json::from_slice(&body).map_err(ApiError::bad_request)?;
    let Some(project_id) = db::project_by_key(&state.pool, &batch.key).await? else {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "unknown write key".into()));
    };
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    let origin = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|v| v.to_str().ok());
    let rules = state.rules.read().unwrap().clone();
    ingest::store(&state.pool, project_id, batch, ua, origin, &rules).await.map_err(ApiError::bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_project(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let p = db::project(&state.pool, state.project_id).await?;
    Ok(Json(json!({
        "name": p.name,
        "write_key": p.write_key,
        "collection": db::collection(&state.pool, state.project_id).await?,
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

async fn get_digest(State(state): State<AppStateRef>, Query(q): Query<DaysQuery>) -> ApiResult<Json<digest::Digest>> {
    Ok(Json(digest::build(&state.pool, state.project_id, q.days.clamp(1, 365)).await?))
}

async fn list_kpis(State(state): State<AppStateRef>, Query(q): Query<DaysQuery>) -> ApiResult<Json<Value>> {
    let now = diagnostic_core::util::now_ms();
    let span = q.days.clamp(1, 365) as i64 * diagnostic_core::util::DAY_MS;
    let mut out = Vec::new();
    for k in kpi::list(&state.pool, state.project_id).await? {
        let current = kpi::measure(&state.pool, state.project_id, &k.definition, now - span, now).await?;
        let previous = kpi::measure(&state.pool, state.project_id, &k.definition, now - 2 * span, now - span).await?;
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

/// Applies a diagnostic.json pushed by `diagnostic push` (or CI). Invalid
/// configs are rejected with every problem listed.
async fn push_config(
    State(state): State<AppStateRef>,
    axum::Extension(who): axum::Extension<crate::auth_api::Principal>,
    headers: HeaderMap,
    body: String,
) -> ApiResult<Json<Value>> {
    let parsed = config::parse(&body).map_err(|errors| ApiError(StatusCode::UNPROCESSABLE_ENTITY, errors.join("\n")))?;
    let source = headers
        .get("x-diagnostic-source")
        .and_then(|v| v.to_str().ok())
        .map(|s| diagnostic_core::util::truncate(s, 200))
        .unwrap_or_else(|| "api".into());
    let report = config::apply(&state.pool, state.project_id, &parsed, &source).await?;
    diagnostic_core::auth::audit(
        &state.pool,
        state.project_id,
        &who.actor(),
        "config.pushed",
        &format!("{source}: {} created, {} updated, {} archived", report.created.len(), report.updated.len(), report.archived.len()),
    )
    .await?;
    Ok(Json(serde_json::to_value(report).map_err(anyhow::Error::from)?))
}

/// Where the config came from, and whether the events it declares are arriving.
async fn get_config(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let health = config::tracking_health(&state.pool, state.project_id, 14).await?;
    Ok(Json(serde_json::to_value(health).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
struct IdeaFilter {
    status: Option<IdeaStatus>,
}

async fn list_ideas(State(state): State<AppStateRef>, Query(f): Query<IdeaFilter>) -> ApiResult<Json<Value>> {
    let list = ideas::list(&state.pool, state.project_id, f.status).await?;
    Ok(Json(serde_json::to_value(list).map_err(anyhow::Error::from)?))
}

async fn get_idea(State(state): State<AppStateRef>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let idea = ideas::get(&state.pool, state.project_id, id).await?.ok_or_else(|| ApiError::not_found("idea"))?;
    Ok(Json(serde_json::to_value(idea).map_err(anyhow::Error::from)?))
}

async fn get_prompt(State(state): State<AppStateRef>, Path(id): Path<i64>) -> ApiResult<Response> {
    let idea = ideas::get(&state.pool, state.project_id, id).await?.ok_or_else(|| ApiError::not_found("idea"))?;
    let project = db::project(&state.pool, state.project_id).await?;
    let text = crate::render_prompt(&state.pool, &project, &idea).await?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], text).into_response())
}

#[derive(Deserialize)]
struct StatusBody {
    status: IdeaStatus,
    #[serde(default)]
    reason: Option<String>,
}

async fn set_status(
    State(state): State<AppStateRef>,
    Path(id): Path<i64>,
    Json(body): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    let idea = ideas::set_status(&state.pool, state.project_id, id, body.status, body.reason.as_deref())
        .await?
        .ok_or_else(|| ApiError::not_found("idea"))?;
    Ok(Json(serde_json::to_value(idea).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize)]
struct KpiLinkBody {
    kpi_id: Option<i64>,
}

async fn set_kpi(
    State(state): State<AppStateRef>,
    Path(id): Path<i64>,
    Json(body): Json<KpiLinkBody>,
) -> ApiResult<Json<Value>> {
    if let Some(k) = body.kpi_id {
        kpi::get(&state.pool, state.project_id, k).await?.ok_or_else(|| ApiError::not_found("KPI"))?;
    }
    let idea = ideas::set_kpi(&state.pool, state.project_id, id, body.kpi_id)
        .await?
        .ok_or_else(|| ApiError::not_found("idea"))?;
    Ok(Json(serde_json::to_value(idea).map_err(anyhow::Error::from)?))
}

#[derive(Deserialize, Default)]
struct AnalyzeBody {
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    days: Option<u32>,
    #[serde(default)]
    max_ideas: Option<usize>,
}

async fn analyze(State(state): State<AppStateRef>, body: Bytes) -> ApiResult<Json<Value>> {
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
    let opts = diagnostic_agent::Options {
        engine,
        days: body.days.unwrap_or(14).clamp(1, 365),
        max_ideas: body.max_ideas.unwrap_or(5).clamp(1, 20),
    };
    let report = diagnostic_agent::analyze(&state.pool, state.project_id, state.claude.as_ref(), &opts).await?;
    Ok(Json(serde_json::to_value(report).map_err(anyhow::Error::from)?))
}

async fn run_evaluate(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let evals = evaluate::run(&state.pool, state.project_id, evaluate::Options::default()).await?;
    Ok(Json(serde_json::to_value(evals).map_err(anyhow::Error::from)?))
}

async fn list_runs(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, String, Option<String>, i64, Option<i64>, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, engine, model, started_at, finished_at, ideas_created, error FROM analysis_runs
         WHERE project_id = ? ORDER BY id DESC LIMIT 20",
    )
    .bind(state.project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(anyhow::Error::from)?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, engine, model, started_at, finished_at, ideas_created, error)| json!({
            "id": id, "engine": engine, "model": model, "started_at": started_at,
            "finished_at": finished_at, "ideas_created": ideas_created, "error": error,
        }))
        .collect::<Vec<_>>())))
}

// ---- MCP over Streamable HTTP ----------------------------------------------------

/// MCP for remote agents: JSON-RPC in, JSON out, authenticated with an API token
/// (`Authorization: Bearer dgt_...`). Stateless, so no session id or event stream.
async fn mcp_http(State(state): State<AppStateRef>, headers: HeaderMap, body: Bytes) -> Response {
    // Browsers send Origin; a page on another site must not reach a local server (DNS rebinding).
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if privacy::referrer_origin(origin).is_none_or(|o| !o.ends_with(&format!("//{host}"))) {
            return (StatusCode::FORBIDDEN, "cross-origin MCP requests are not allowed").into_response();
        }
    }
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let name = match token {
        Some(t) if state.admin_token.as_deref().is_some_and(|a| diagnostic_core::auth::constant_time_eq(a, t)) => {
            Some("DIAGNOSTIC_ADMIN_TOKEN".to_string())
        }
        Some(t) => diagnostic_core::auth::verify_api_token(&state.pool, state.project_id, t).await.ok().flatten().map(|(_, n)| n),
        None => None,
    };
    let Some(name) = name else {
        let mut res = (StatusCode::UNAUTHORIZED, Json(json!({ "error": "send an API token from the dashboard's Team page as `Authorization: Bearer dgt_...`" }))).into_response();
        res.headers_mut().insert(header::WWW_AUTHENTICATE, header::HeaderValue::from_static("Bearer"));
        return res;
    };
    let ctx = crate::mcp::Context {
        pool: &state.pool,
        project_id: state.project_id,
        claude: state.claude.as_ref(),
        actor: format!("mcp: token \"{name}\""),
        role: diagnostic_core::auth::Role::Admin,
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

async fn get_privacy(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "built_in": BUILT_IN,
        "rules": privacy::list_rules(&state.pool, state.project_id).await?,
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
    State(state): State<AppStateRef>,
    axum::Extension(who): axum::Extension<crate::auth_api::Principal>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    let actor = who.actor();
    let id = privacy::add_rule(&state.pool, state.project_id, b.kind, &b.pattern, b.replacement.as_deref(), &b.label, &actor)
        .await
        .map_err(ApiError::bad_request)?;
    reload_rules(&state).await?;
    diagnostic_core::auth::audit(&state.pool, state.project_id, &actor, "privacy.rule_added", &format!("{} {}", b.kind.as_str(), b.pattern.trim())).await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct ToggleBody {
    enabled: bool,
}

async fn toggle_privacy_rule(
    State(state): State<AppStateRef>,
    axum::Extension(who): axum::Extension<crate::auth_api::Principal>,
    Path(id): Path<i64>,
    Json(b): Json<ToggleBody>,
) -> ApiResult<StatusCode> {
    if !privacy::set_rule_enabled(&state.pool, state.project_id, id, b.enabled).await? {
        return Err(ApiError::not_found("rule"));
    }
    reload_rules(&state).await?;
    let action = if b.enabled { "privacy.rule_enabled" } else { "privacy.rule_disabled" };
    diagnostic_core::auth::audit(&state.pool, state.project_id, &who.actor(), action, &format!("rule #{id}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_privacy_rule(
    State(state): State<AppStateRef>,
    axum::Extension(who): axum::Extension<crate::auth_api::Principal>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let rule = privacy::delete_rule(&state.pool, state.project_id, id).await?.ok_or_else(|| ApiError::not_found("rule"))?;
    reload_rules(&state).await?;
    diagnostic_core::auth::audit(&state.pool, state.project_id, &who.actor(), "privacy.rule_removed", &format!("{} {}", rule.kind.as_str(), rule.pattern)).await?;
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
async fn test_privacy(State(state): State<AppStateRef>, Json(b): Json<TestBody>) -> ApiResult<Json<Value>> {
    let rules = state.rules.read().unwrap().clone();
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
