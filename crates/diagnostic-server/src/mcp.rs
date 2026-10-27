//! Model Context Protocol server: the same data and actions as the dashboard,
//! for coding agents. Served over stdio (`diagnostic mcp`) and Streamable HTTP
//! (`POST /mcp` with an API token).
//!
//! Like the dashboard, it only ever exposes aggregated patterns: no individual
//! sessions, visitors or events. Privacy rules and people are managed in the
//! dashboard only, and KPIs in `diagnostic.json`.

use diagnostic_agent::{ClaudeConfig, Engine};
use diagnostic_core::auth::Role;
use diagnostic_core::model::IdeaStatus;
use diagnostic_core::{SqlitePool, config, db, digest, evaluate, ideas, kpi, privacy};
use serde_json::{Value, json};

/// MCP versions this server speaks, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
Diagnostic is privacy-first web analytics for improving a website's UX. It studies aggregated behaviour \
patterns (never individual people) and turns them into improvement ideas tied to the team's KPIs.

A typical loop:
1. get_overview to see KPIs against their targets and how many ideas are waiting.
2. list_ideas with status \"new\" or \"selected\", then get_idea_prompt for the one to work on: it is a complete \
task description with evidence, the KPI to move and acceptance criteria.
3. Implement the change in the codebase. Move the idea to \"in_progress\" while working on it.
4. After the change ships, update_idea_status to \"released\": Diagnostic then measures whether the KPI moved \
(evaluate_releases or get_idea shows the verdict).

KPIs and the custom-event catalogue live in the app's diagnostic.json; use check_config to validate edits to it. \
Use get_behaviour_summary for the data behind ideas, and get_tracking_health to confirm custom events arrive.";

/// Who is calling, and what they may do.
pub struct Context<'a> {
    pub pool: &'a SqlitePool,
    pub project_id: i64,
    pub claude: Option<&'a ClaudeConfig>,
    /// Recorded in the activity log, e.g. `mcp: token "Claude Code"`.
    pub actor: String,
    pub role: Role,
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Handles one JSON-RPC message. Returns None for notifications, which get no reply.
pub async fn handle(ctx: &Context<'_>, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let Some(id) = id else {
        return None; // notifications: initialized, cancelled, ...
    };
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_empty() {
        return Some(rpc_error(&id, -32600, "invalid JSON-RPC request"));
    }
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            rpc_result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "diagnostic", "title": "Diagnostic", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => rpc_result(&id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !tools().iter().any(|t| t["name"] == name) {
                return Some(rpc_error(&id, -32602, &format!("unknown tool {name:?}")));
            }
            let result = match call(ctx, name, &args).await {
                Ok(ToolOutput::Json(v)) => json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                    "structuredContent": if v.is_object() { v } else { json!({ "result": v }) },
                }),
                Ok(ToolOutput::Text(t)) => json!({ "content": [{ "type": "text", "text": t }] }),
                // Tool failures are results the model can read and act on, not protocol errors.
                Err(e) => json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true }),
            };
            rpc_result(&id, result)
        }
        "resources/list" => rpc_result(&id, json!({ "resources": [] })),
        "prompts/list" => rpc_result(&id, json!({ "prompts": [] })),
        _ => rpc_error(&id, -32601, &format!("method {method:?} not found")),
    })
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false },
    })
}

pub fn tools() -> Vec<Value> {
    let days = json!({ "type": "integer", "minimum": 1, "maximum": 365, "description": "Days of behaviour to look at (default 14)" });
    let id = json!({ "type": "integer", "description": "Idea id, e.g. 12" });
    let statuses = json!(["new", "selected", "in_progress", "released", "dismissed"]);
    vec![
        tool(
            "get_overview",
            "Overview",
            "Start here: whether data is being collected, each KPI against its target (last 14 days vs the 14 before), and how many ideas are in each status.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "get_behaviour_summary",
            "Behaviour summary",
            "Aggregated behaviour for a window: top pages with exit rates, scroll and time, navigation paths, rage and dead clicks, errors (exceptions, console errors, failed requests, missing files), form completion and custom events. Aggregates only; no individual sessions.",
            json!({ "days": days }),
            &[],
            true,
        ),
        tool("list_kpis", "KPIs", "The KPIs defined in diagnostic.json with current and previous values and targets.", json!({ "days": days }), &[], true),
        tool(
            "list_ideas",
            "List ideas",
            "Improvement ideas, newest activity first, optionally filtered by status. Each includes the KPI it should move and, once released, the measured impact.",
            json!({ "status": { "type": "string", "enum": statuses } }),
            &[],
            true,
        ),
        tool("get_idea", "Idea", "One idea in full: problem, evidence, proposal, acceptance criteria, pages and evaluation.", json!({ "id": id }), &["id"], true),
        tool(
            "get_idea_prompt",
            "Idea as a task prompt",
            "The idea as a complete, self-contained task for implementing it in the codebase (Markdown): problem, evidence, goal KPI and current value, proposed change, routes involved, acceptance criteria and constraints.",
            json!({ "id": id }),
            &["id"],
            true,
        ),
        tool(
            "update_idea_status",
            "Update idea status",
            "Move an idea through new → selected → in_progress → released, or dismiss it. Mark it released once the change is live: Diagnostic then compares the KPI before and after. When dismissing, give the reason; the analyst learns from it.",
            json!({ "id": id, "status": { "type": "string", "enum": statuses }, "reason": { "type": "string", "description": "Why it was dismissed" } }),
            &["id", "status"],
            false,
        ),
        tool(
            "run_analysis",
            "Run analysis",
            "Analyse recent behaviour and create new improvement ideas. Uses Claude when the server has an API key (this costs API credits and can take a minute), otherwise built-in heuristics. Existing and dismissed ideas aren't repeated.",
            json!({ "days": days, "max_ideas": { "type": "integer", "minimum": 1, "maximum": 20, "description": "Default 5" } }),
            &[],
            false,
        ),
        tool(
            "evaluate_releases",
            "Evaluate releases",
            "Measure every released idea against its KPI now (before vs after the release, with a significance test). Runs hourly anyway.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "get_tracking_health",
            "Tracking health",
            "Custom events declared in diagnostic.json and whether the app is sending them (last 14 days), plus events sent without being declared.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "check_config",
            "Check diagnostic.json",
            "Validate the contents of a diagnostic.json (KPIs and event catalogue) without applying it. Returns every problem with its location, or a summary and warnings. Publishing happens through CI with `diagnostic push`.",
            json!({ "json": { "type": "string", "description": "The full file contents" } }),
            &["json"],
            true,
        ),
        tool(
            "get_privacy_rules",
            "Privacy rules",
            "The privacy protections in force: what is never collected, and the workspace's own rules (masked elements, redacted patterns, ignored pages, dropped properties). Read-only; admins change rules in the dashboard.",
            json!({}),
            &[],
            true,
        ),
    ]
}

enum ToolOutput {
    Json(Value),
    Text(String),
}

fn arg_days(args: &Value) -> u32 {
    args.get("days").and_then(Value::as_u64).map(|d| d.clamp(1, 365) as u32).unwrap_or(14)
}

fn arg_id(args: &Value) -> anyhow::Result<i64> {
    args.get("id").and_then(Value::as_i64).ok_or_else(|| anyhow::anyhow!("`id` is required, e.g. {{\"id\": 12}}"))
}

fn need(ctx: &Context, role: Role) -> anyhow::Result<()> {
    if ctx.role < role {
        anyhow::bail!("this needs the {role} role; ask an admin for a token with more access");
    }
    Ok(())
}

async fn kpi_values(ctx: &Context<'_>, days: u32) -> anyhow::Result<Vec<Value>> {
    let now = diagnostic_core::util::now_ms();
    let span = days as i64 * diagnostic_core::util::DAY_MS;
    let mut out = Vec::new();
    for k in kpi::list(ctx.pool, ctx.project_id).await? {
        let current = kpi::measure(ctx.pool, ctx.project_id, &k.definition, now - span, now).await?;
        let previous = kpi::measure(ctx.pool, ctx.project_id, &k.definition, now - 2 * span, now - span).await?;
        let fmt = |v: Option<f64>| v.map(|v| k.definition.format_value(v));
        out.push(json!({
            "key": k.key,
            "id": k.id,
            "name": k.name,
            "description": if k.description.is_empty() { k.definition.describe() } else { k.description.clone() },
            "goal": k.direction.as_str(),
            "current": fmt(current.value),
            "previous": fmt(previous.value),
            "target": fmt(k.target),
            "target_met": current.value.and_then(|v| kpi::target_met(&k, v)),
            "sessions": current.n(),
        }));
    }
    Ok(out)
}

async fn call(ctx: &Context<'_>, name: &str, args: &Value) -> anyhow::Result<ToolOutput> {
    let (pool, p) = (ctx.pool, ctx.project_id);
    Ok(match name {
        "get_overview" => {
            let project = db::project(pool, p).await?;
            let all = ideas::list(pool, p, None).await?;
            let counts: serde_json::Map<String, Value> = IdeaStatus::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), json!(all.iter().filter(|i| i.status == *s).count())))
                .collect();
            let collection = db::collection(pool, p).await?;
            ToolOutput::Json(json!({
                "product": project.name,
                "collecting": collection.active,
                "last_event_at": collection.last_event_at,
                "sites": collection.sources.iter().map(|s| &s.origin).collect::<Vec<_>>(),
                "kpis": kpi_values(ctx, 14).await?,
                "ideas": counts,
                "analyst": if ctx.claude.is_some() { "claude" } else { "heuristics" },
                "privacy": "Diagnostic only exposes aggregated patterns: no individual sessions or people.",
            }))
        }
        "get_behaviour_summary" => ToolOutput::Json(serde_json::to_value(digest::build(pool, p, arg_days(args)).await?)?),
        "list_kpis" => ToolOutput::Json(json!({ "kpis": kpi_values(ctx, arg_days(args)).await? })),
        "list_ideas" => {
            let status = match args.get("status").and_then(Value::as_str) {
                Some(s) => Some(s.parse::<IdeaStatus>()?),
                None => None,
            };
            let kpis = kpi::list(pool, p).await?;
            let list: Vec<Value> = ideas::list(pool, p, status)
                .await?
                .into_iter()
                .map(|i| {
                    json!({
                        "id": i.id,
                        "title": i.title,
                        "status": i.status,
                        "confidence": i.confidence,
                        "kpi": i.kpi_id.and_then(|id| kpis.iter().find(|k| k.id == id)).map(|k| &k.name),
                        "pages": i.pages,
                        "source": i.source,
                        "impact": i.evaluation.as_ref().map(|e| json!({ "verdict": e.verdict, "summary": e.summary })),
                    })
                })
                .collect();
            ToolOutput::Json(json!({ "ideas": list }))
        }
        "get_idea" => {
            let idea = ideas::get(pool, p, arg_id(args)?).await?.ok_or_else(|| anyhow::anyhow!("no idea with that id"))?;
            ToolOutput::Json(serde_json::to_value(idea)?)
        }
        "get_idea_prompt" => {
            let idea = ideas::get(pool, p, arg_id(args)?).await?.ok_or_else(|| anyhow::anyhow!("no idea with that id"))?;
            let project = db::project(pool, p).await?;
            ToolOutput::Text(crate::render_prompt(pool, &project, &idea).await?)
        }
        "update_idea_status" => {
            need(ctx, Role::Member)?;
            let id = arg_id(args)?;
            let status: IdeaStatus = args
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("`status` is required"))?
                .parse()?;
            let reason = args.get("reason").and_then(Value::as_str);
            let idea = ideas::set_status(pool, p, id, status, reason).await?.ok_or_else(|| anyhow::anyhow!("no idea with that id"))?;
            diagnostic_core::auth::audit(pool, p, &ctx.actor, "idea.status", &format!("#{id} → {status}")).await?;
            let mut note = format!("Idea #{id} is now {status}.");
            if status == IdeaStatus::Released {
                if idea.kpi_id.is_some() {
                    evaluate::run(pool, p, evaluate::Options::default()).await?;
                    note.push_str(" Diagnostic will compare its KPI before and after the release; check back with get_idea in a few days.");
                } else {
                    note.push_str(" It has no KPI linked, so its impact can't be measured.");
                }
            }
            ToolOutput::Json(json!({ "message": note, "idea": { "id": idea.id, "title": idea.title, "status": idea.status } }))
        }
        "run_analysis" => {
            need(ctx, Role::Member)?;
            let opts = diagnostic_agent::Options {
                engine: Engine::Auto,
                days: arg_days(args),
                max_ideas: args.get("max_ideas").and_then(Value::as_u64).map(|n| n.clamp(1, 20) as usize).unwrap_or(5),
            };
            let report = diagnostic_agent::analyze(pool, p, ctx.claude, &opts).await?;
            diagnostic_core::auth::audit(pool, p, &ctx.actor, "analysis.run", &format!("{} new ideas via {}", report.created.len(), report.engine)).await?;
            let mut created = Vec::new();
            for id in &report.created {
                if let Some(i) = ideas::get(pool, p, *id).await? {
                    created.push(json!({ "id": i.id, "title": i.title }));
                }
            }
            ToolOutput::Json(json!({ "engine": report.engine, "model": report.model, "created": created, "already_known": report.duplicates, "warning": report.warning }))
        }
        "evaluate_releases" => {
            need(ctx, Role::Member)?;
            ToolOutput::Json(json!({ "evaluations": evaluate::run(pool, p, evaluate::Options::default()).await? }))
        }
        "get_tracking_health" => ToolOutput::Json(serde_json::to_value(config::tracking_health(pool, p, 14).await?)?),
        "check_config" => {
            let text = args.get("json").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("`json` is required"))?;
            match config::parse(text) {
                Ok(parsed) => ToolOutput::Json(json!({
                    "valid": true,
                    "product": parsed.config.product,
                    "kpis": parsed.config.kpis.iter().map(|k| json!({ "key": k.key, "name": k.name, "measures": k.definition.describe() })).collect::<Vec<_>>(),
                    "events": parsed.config.events,
                    "warnings": parsed.warnings,
                })),
                Err(errors) => ToolOutput::Json(json!({ "valid": false, "errors": errors })),
            }
        }
        "get_privacy_rules" => ToolOutput::Json(json!({
            "always_on": crate::api::BUILT_IN,
            "workspace_rules": privacy::list_rules(pool, p).await?
                .into_iter()
                .filter(|r| r.enabled)
                .map(|r| json!({ "kind": r.kind, "pattern": r.pattern, "replacement": r.replacement, "note": r.label }))
                .collect::<Vec<_>>(),
        })),
        other => anyhow::bail!("unknown tool {other:?}"),
    })
}

/// `diagnostic mcp`: newline-delimited JSON-RPC on stdin/stdout. Logs go to stderr.
pub async fn serve_stdio(ctx: Context<'_>) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let mut replies = Vec::new();
                for m in &batch {
                    replies.extend(handle(&ctx, m).await);
                }
                (!replies.is_empty()).then(|| Value::Array(replies))
            }
            Ok(msg) => handle(&ctx, &msg).await,
            Err(e) => Some(rpc_error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            out.write_all(serde_json::to_string(&r)?.as_bytes()).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handshake_tools_and_calls() {
        let path = std::env::temp_dir().join(format!("diagnostic-mcp-{}.db", diagnostic_core::util::new_key()));
        let pool = db::connect(path.to_str().unwrap()).await.unwrap();
        let project = db::default_project(&pool).await.unwrap();
        let ctx = Context { pool: &pool, project_id: project.id, claude: None, actor: "test".into(), role: Role::Viewer };

        let init = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(handle(&ctx, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());

        let list = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await.unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 10);

        let overview = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "get_overview" } })).await.unwrap();
        assert_eq!(overview["result"]["structuredContent"]["collecting"], false);

        let denied = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "update_idea_status", "arguments": { "id": 1, "status": "selected" } } })).await.unwrap();
        assert_eq!(denied["result"]["isError"], true, "viewers can't change ideas");

        let bad = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "check_config", "arguments": { "json": "{\"kpis\":{\"x\":{\"goal\":\"up\"}}}" } } })).await.unwrap();
        assert_eq!(bad["result"]["structuredContent"]["valid"], false);

        let unknown = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 6, "method": "nope" })).await.unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        let _ = std::fs::remove_file(path);
    }
}
