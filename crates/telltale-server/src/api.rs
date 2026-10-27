use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use telltale_agent::{ClaudeConfig, Engine};
use telltale_core::model::{IdeaStatus, KpiInput};
use telltale_core::{SqlitePool, db, digest, evaluate, ideas, ingest, kpi};
use tower_http::cors::{Any, CorsLayer};

pub struct AppState {
    pub pool: SqlitePool,
    pub project_id: i64,
    pub admin_token: Option<String>,
    pub claude: Option<ClaudeConfig>,
}

type AppStateRef = Arc<AppState>;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const STYLE_CSS: &str = include_str!("../assets/style.css");
const COLLECTOR_JS: &str = include_str!("../assets/t.js");

pub fn router(state: AppStateRef) -> Router {
    let collector = Router::new()
        .route("/v1/collect", post(collect))
        .route("/t.js", get(|| async { asset("application/javascript", COLLECTOR_JS, "public, max-age=300") }))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST]).allow_headers(Any));

    let api = Router::new()
        .route("/project", get(get_project).put(rename_project))
        .route("/digest", get(get_digest))
        .route("/kpis", get(list_kpis).post(create_kpi))
        .route("/kpis/{id}", axum::routing::put(update_kpi).delete(delete_kpi))
        .route("/ideas", get(list_ideas))
        .route("/ideas/{id}", get(get_idea))
        .route("/ideas/{id}/prompt", get(get_prompt))
        .route("/ideas/{id}/status", post(set_status))
        .route("/ideas/{id}/kpi", post(set_kpi))
        .route("/analyze", post(analyze))
        .route("/evaluate", post(run_evaluate))
        .route("/runs", get(list_runs))
        .layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/assets/app.js", get(|| async { asset("application/javascript", APP_JS, "no-cache") }))
        .route("/assets/style.css", get(|| async { asset("text/css", STYLE_CSS, "no-cache") }))
        .nest("/api", api)
        .merge(collector)
        .with_state(state)
}

fn asset(content_type: &'static str, body: &'static str, cache: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, cache)], body).into_response()
}

async fn require_token(State(state): State<AppStateRef>, req: Request, next: Next) -> Response {
    if let Some(expected) = &state.admin_token {
        let given = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if given != Some(expected.as_str()) {
            return (StatusCode::UNAUTHORIZED, "admin token required").into_response();
        }
    }
    next.run(req).await
}

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn not_found(what: &str) -> Self {
        Self(StatusCode::NOT_FOUND, format!("{what} not found"))
    }
    fn bad_request(e: impl std::fmt::Display) -> Self {
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %format!("{e:#}"), "request failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

async fn collect(State(state): State<AppStateRef>, headers: HeaderMap, body: Bytes) -> ApiResult<StatusCode> {
    // The collector posts JSON as text/plain so browsers skip the CORS preflight.
    let batch: ingest::Batch = serde_json::from_slice(&body).map_err(ApiError::bad_request)?;
    let Some(project_id) = db::project_by_key(&state.pool, &batch.key).await? else {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "unknown write key".into()));
    };
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    ingest::store(&state.pool, project_id, batch, ua).await.map_err(ApiError::bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_project(State(state): State<AppStateRef>) -> ApiResult<Json<Value>> {
    let p = db::project(&state.pool, state.project_id).await?;
    Ok(Json(json!({
        "name": p.name,
        "write_key": p.write_key,
        "claude": state.claude.as_ref().map(|c| json!({ "model": c.model, "effort": c.effort })),
    })))
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

async fn rename_project(State(state): State<AppStateRef>, Json(body): Json<RenameBody>) -> ApiResult<Json<Value>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    db::rename_project(&state.pool, state.project_id, name).await?;
    get_project(State(state)).await
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
    let now = telltale_core::util::now_ms();
    let span = q.days.clamp(1, 365) as i64 * telltale_core::util::DAY_MS;
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

async fn create_kpi(State(state): State<AppStateRef>, Json(input): Json<KpiInput>) -> ApiResult<Json<Value>> {
    let k = kpi::create(&state.pool, state.project_id, &input).await.map_err(ApiError::bad_request)?;
    Ok(Json(serde_json::to_value(k).map_err(anyhow::Error::from)?))
}

async fn update_kpi(
    State(state): State<AppStateRef>,
    Path(id): Path<i64>,
    Json(input): Json<KpiInput>,
) -> ApiResult<Json<Value>> {
    let k = kpi::update(&state.pool, state.project_id, id, &input)
        .await
        .map_err(ApiError::bad_request)?
        .ok_or_else(|| ApiError::not_found("KPI"))?;
    Ok(Json(serde_json::to_value(k).map_err(anyhow::Error::from)?))
}

async fn delete_kpi(State(state): State<AppStateRef>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if kpi::delete(&state.pool, state.project_id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("KPI"))
    }
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

async fn analyze(State(state): State<AppStateRef>, body: Option<Json<AnalyzeBody>>) -> ApiResult<Json<Value>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let engine = match body.engine.as_deref() {
        None | Some("auto") => Engine::Auto,
        Some("claude") => Engine::Claude,
        Some("heuristics") => Engine::Heuristics,
        Some(other) => return Err(ApiError::bad_request(format!("unknown engine {other:?}"))),
    };
    let opts = telltale_agent::Options {
        engine,
        days: body.days.unwrap_or(14).clamp(1, 365),
        max_ideas: body.max_ideas.unwrap_or(5).clamp(1, 20),
    };
    let report = telltale_agent::analyze(&state.pool, state.project_id, state.claude.as_ref(), &opts).await?;
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
