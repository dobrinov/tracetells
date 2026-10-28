//! Model Context Protocol server: the same data and actions as the dashboard,
//! for coding agents. Served over stdio (`trace mcp`) and Streamable HTTP
//! (`POST /mcp` with an API token).
//!
//! Like the dashboard, it only ever exposes aggregated patterns: no individual
//! sessions, visitors or events. Privacy rules and people are managed in the
//! dashboard only, and KPIs in `trace.json`.

use trace_agent::{ClaudeConfig, Engine};
use trace_core::auth::Role;
use trace_core::model::InsightStatus;
use trace_core::model::Project;
use trace_core::{SqlitePool, config, db, digest, evaluate, insights, kpi, privacy};
use serde_json::{Value, json};

/// MCP versions this server speaks, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
Trace is privacy-first web analytics for improving a website's UX. It studies aggregated behaviour \
patterns (never individual people) and turns them into improvement insights tied to the team's KPIs.

A typical loop:
0. If the account has several websites, list_sites and pass `site` to the other tools.
1. get_overview to see KPIs against their targets and how many insights are waiting.
2. list_insights with status \"new\" or \"selected\", then get_insight_prompt for the one to work on: it is a complete \
task description with evidence, the KPI to move and acceptance criteria.
3. Implement the change in the codebase. Move the insight to \"in_progress\" while working on it.
4. After the change ships, update_insight_status to \"released\": Trace then measures whether the KPI moved \
(evaluate_releases or get_insight shows the verdict).

KPIs and the custom-event catalogue live in the app's trace.json; use check_config to validate edits to it. \
Use get_behaviour_summary for the data behind insights, and get_tracking_health to confirm custom events arrive.";

/// Who is calling, and what they may do.
pub struct Context<'a> {
    pub pool: &'a SqlitePool,
    /// The caller's account: tools only ever see its websites.
    pub account_id: i64,
    pub claude: Option<&'a ClaudeConfig>,
    /// Recorded in the activity log, e.g. `mcp: ana@acme.example via token "Claude Code"`.
    pub actor: String,
    pub role: Role,
    /// A read-only token: reads work, changes are refused.
    pub read_only: bool,
}

/// The website a tool call is about: the `site` argument, or the account's only website.
async fn site_for(ctx: &Context<'_>, args: &Value) -> anyhow::Result<Project> {
    let sites = db::sites(ctx.pool, ctx.account_id).await?;
    let names = || sites.iter().map(|s| format!("{} ({})", s.slug, s.name)).collect::<Vec<_>>().join(", ");
    match args.get("site").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        Some(slug) => sites
            .iter()
            .find(|s| s.slug == slug)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no website {slug:?} in this account; websites: {}", names())),
        None => match sites.as_slice() {
            [only] => Ok(only.clone()),
            [] => anyhow::bail!("this account has no websites yet"),
            _ => anyhow::bail!("this account has several websites; pass `site` as one of: {}", names()),
        },
    }
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
                    "serverInfo": { "name": "trace", "title": "Trace", "version": env!("CARGO_PKG_VERSION") },
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

fn tool(name: &str, title: &str, description: &str, mut properties: Value, required: &[&str], read_only: bool) -> Value {
    if !matches!(name, "list_sites" | "check_config") {
        properties["site"] = json!({
            "type": "string",
            "description": "The website's identifier (from list_sites); can be left out when the account has one website",
        });
    }
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
    let id = json!({ "type": "integer", "description": "Insight id, e.g. 12" });
    let statuses = json!(["new", "selected", "in_progress", "released", "dismissed"]);
    vec![
        tool(
            "list_sites",
            "Websites",
            "The account's websites, with the identifier to pass as `site` to the other tools, the addresses each collects from and whether it is collecting.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "get_overview",
            "Overview",
            "Start here: whether data is being collected, each KPI against its target (last 14 days vs the 14 before), and how many insights are in each status.",
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
        tool("list_kpis", "KPIs", "The KPIs defined in trace.json with current and previous values and targets.", json!({ "days": days }), &[], true),
        tool(
            "list_insights",
            "List insights",
            "Improvement insights, newest activity first, optionally filtered by status. Each includes the KPI it should move and, once released, the measured impact.",
            json!({ "status": { "type": "string", "enum": statuses } }),
            &[],
            true,
        ),
        tool("get_insight", "Insight", "One insight in full: problem, evidence, proposal, acceptance criteria, pages and evaluation.", json!({ "id": id }), &["id"], true),
        tool(
            "get_insight_prompt",
            "Insight as a task prompt",
            "The insight as a complete, self-contained task for implementing it in the codebase (Markdown): problem, evidence, goal KPI and current value, proposed change, routes involved, acceptance criteria and constraints.",
            json!({ "id": id }),
            &["id"],
            true,
        ),
        tool(
            "update_insight_status",
            "Update insight status",
            "Move an insight through new → selected → in_progress → released, or dismiss it. Mark it released once the change is live: Trace then compares the KPI before and after. When dismissing, give the reason; the analyst learns from it.",
            json!({ "id": id, "status": { "type": "string", "enum": statuses }, "reason": { "type": "string", "description": "Why it was dismissed" } }),
            &["id", "status"],
            false,
        ),
        tool(
            "run_analysis",
            "Run analysis",
            "Analyse recent behaviour and create new improvement insights. Uses Claude when the server has an API key (this costs API credits and can take a minute), otherwise built-in heuristics. Existing and dismissed insights aren't repeated.",
            json!({ "days": days, "max_insights": { "type": "integer", "minimum": 1, "maximum": 20, "description": "Default 5" } }),
            &[],
            false,
        ),
        tool(
            "evaluate_releases",
            "Evaluate releases",
            "Measure every released insight against its KPI now (before vs after the release, with a significance test). Runs hourly anyway.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "get_tracking_health",
            "Tracking health",
            "Custom events declared in trace.json and whether the app is sending them (last 14 days), plus events sent without being declared.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "check_config",
            "Check trace.json",
            "Validate the contents of a trace.json (KPIs and event catalogue) without applying it. Returns every problem with its location, or a summary and warnings. Publishing happens through CI with `trace push`.",
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
    if ctx.read_only {
        anyhow::bail!("this token is read-only, so it can't change anything; use a token that isn't read-only");
    }
    if ctx.role < role {
        anyhow::bail!(
            "this needs the {role} role, and this token acts as {}. Tokens act with their owner's role (or read-only): \
             use a token that isn't read-only, or ask an admin to change your role",
            ctx.role
        );
    }
    Ok(())
}

async fn kpi_values(ctx: &Context<'_>, p: i64, days: u32) -> anyhow::Result<Vec<Value>> {
    let now = trace_core::util::now_ms();
    let span = days as i64 * trace_core::util::DAY_MS;
    let mut out = Vec::new();
    for k in kpi::list(ctx.pool, p).await? {
        let current = kpi::measure(ctx.pool, p, &k.definition, now - span, now).await?;
        let previous = kpi::measure(ctx.pool, p, &k.definition, now - 2 * span, now - span).await?;
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
    let pool = ctx.pool;
    if name == "list_sites" {
        let mut out = Vec::new();
        for s in db::sites(pool, ctx.account_id).await? {
            let c = db::collection(pool, s.id).await?;
            out.push(json!({ "site": s.slug, "name": s.name, "collecting": c.active, "sites": c.sources.iter().map(|o| &o.origin).collect::<Vec<_>>() }));
        }
        return Ok(ToolOutput::Json(json!({ "websites": out })));
    }
    let site = if name == "check_config" { None } else { Some(site_for(ctx, args).await?) };
    let p = site.as_ref().map(|s| s.id).unwrap_or_default();
    Ok(match name {
        "get_overview" => {
            let project = site.clone().expect("a website tool");
            let all = insights::list(pool, p, None).await?;
            let counts: serde_json::Map<String, Value> = InsightStatus::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), json!(all.iter().filter(|i| i.status == *s).count())))
                .collect();
            let collection = db::collection(pool, p).await?;
            ToolOutput::Json(json!({
                "website": { "site": project.slug, "name": project.name },
                "collecting": collection.active,
                "last_event_at": collection.last_event_at,
                "sites": collection.sources.iter().map(|s| &s.origin).collect::<Vec<_>>(),
                "kpis": kpi_values(ctx, p, 14).await?,
                "insights": counts,
                "analyst": if ctx.claude.is_some() { "claude" } else { "heuristics" },
                "privacy": "Trace only exposes aggregated patterns: no individual sessions or people.",
            }))
        }
        "get_behaviour_summary" => ToolOutput::Json(serde_json::to_value(digest::build(pool, p, arg_days(args)).await?)?),
        "list_kpis" => ToolOutput::Json(json!({ "kpis": kpi_values(ctx, p, arg_days(args)).await? })),
        "list_insights" => {
            let status = match args.get("status").and_then(Value::as_str) {
                Some(s) => Some(s.parse::<InsightStatus>()?),
                None => None,
            };
            let kpis = kpi::list(pool, p).await?;
            let list: Vec<Value> = insights::list(pool, p, status)
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
            ToolOutput::Json(json!({ "insights": list }))
        }
        "get_insight" => {
            let insight = insights::get(pool, p, arg_id(args)?).await?.ok_or_else(|| anyhow::anyhow!("no insight with that id"))?;
            ToolOutput::Json(serde_json::to_value(insight)?)
        }
        "get_insight_prompt" => {
            let insight = insights::get(pool, p, arg_id(args)?).await?.ok_or_else(|| anyhow::anyhow!("no insight with that id"))?;
            let project = site.clone().expect("a website tool");
            ToolOutput::Text(crate::render_prompt(pool, &project, &insight).await?)
        }
        "update_insight_status" => {
            need(ctx, Role::User)?;
            let id = arg_id(args)?;
            let status: InsightStatus = args
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("`status` is required"))?
                .parse()?;
            let reason = args.get("reason").and_then(Value::as_str);
            let insight = insights::set_status(pool, p, id, status, reason).await?.ok_or_else(|| anyhow::anyhow!("no insight with that id"))?;
            trace_core::auth::audit_site(pool, ctx.account_id, Some(p), &ctx.actor, "insight.status", &format!("#{id} → {status}")).await?;
            let mut note = format!("Insight #{id} is now {status}.");
            if status == InsightStatus::Released {
                if insight.kpi_id.is_some() {
                    evaluate::run(pool, p, evaluate::Options::default()).await?;
                    note.push_str(" Trace will compare its KPI before and after the release; check back with get_insight in a few days.");
                } else {
                    note.push_str(" It has no KPI linked, so its impact can't be measured.");
                }
            }
            ToolOutput::Json(json!({ "message": note, "insight": { "id": insight.id, "title": insight.title, "status": insight.status } }))
        }
        "run_analysis" => {
            need(ctx, Role::User)?;
            let opts = trace_agent::Options {
                engine: Engine::Auto,
                days: arg_days(args),
                max_insights: args.get("max_insights").and_then(Value::as_u64).map(|n| n.clamp(1, 20) as usize).unwrap_or(5),
            };
            let report = crate::analysis::run(pool, site.as_ref().expect("a website tool"), ctx.claude, &opts).await?;
            trace_core::auth::audit_site(pool, ctx.account_id, Some(p), &ctx.actor, "analysis.run", &format!("{} new insights via {}", report.created.len(), report.engine)).await?;
            let mut created = Vec::new();
            for id in &report.created {
                if let Some(i) = insights::get(pool, p, *id).await? {
                    created.push(json!({ "id": i.id, "title": i.title }));
                }
            }
            ToolOutput::Json(json!({ "engine": report.engine, "model": report.model, "created": created, "already_known": report.duplicates, "warning": report.warning }))
        }
        "evaluate_releases" => {
            need(ctx, Role::User)?;
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

/// `trace mcp`: newline-delimited JSON-RPC on stdin/stdout. Logs go to stderr.
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
        let path = std::env::temp_dir().join(format!("trace-mcp-{}.db", trace_core::util::new_key()));
        let pool = db::connect(path.to_str().unwrap()).await.unwrap();
        let (acme, _) = db::create_account_with_site(&pool, "Acme", None).await.unwrap();
        let (bolt, _) = db::create_account_with_site(&pool, "Bolt", None).await.unwrap();
        let ctx = Context { pool: &pool, account_id: acme.id, claude: None, actor: "test".into(), role: Role::User, read_only: true };
        let call = |name: &str, args: Value| json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": name, "arguments": args } });

        let init = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(handle(&ctx, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());
        let list = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await.unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 11);
        assert!(tools.iter().find(|t| t["name"] == "get_overview").unwrap()["inputSchema"]["properties"]["site"].is_object());

        // One website: tools use it without being told.
        let overview = handle(&ctx, &call("get_overview", json!({}))).await.unwrap();
        assert_eq!(overview["result"]["structuredContent"]["website"]["site"], "acme");

        // Two websites: tools ask which, and list them.
        db::create_site(&pool, acme.id, "Docs").await.unwrap();
        let ambiguous = handle(&ctx, &call("get_overview", json!({}))).await.unwrap();
        assert_eq!(ambiguous["result"]["isError"], true);
        let msg = ambiguous["result"]["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains("acme") && msg.contains("docs"), "{msg}");
        let docs = handle(&ctx, &call("get_overview", json!({ "site": "docs" }))).await.unwrap();
        assert_eq!(docs["result"]["structuredContent"]["website"]["site"], "docs");

        // Only the caller's account: Bolt's website is invisible and unreachable.
        let sites = handle(&ctx, &call("list_sites", json!({}))).await.unwrap();
        let names: Vec<&str> = sites["result"]["structuredContent"]["websites"].as_array().unwrap().iter().map(|w| w["site"].as_str().unwrap()).collect();
        assert_eq!(names, ["acme", "docs"]);
        let other = handle(&ctx, &call("list_insights", json!({ "site": "bolt" }))).await.unwrap();
        assert_eq!(other["result"]["isError"], true, "account {} can't see {}", acme.id, bolt.id);

        let denied = handle(&ctx, &call("update_insight_status", json!({ "site": "acme", "id": 1, "status": "selected" }))).await.unwrap();
        assert!(denied["result"]["content"][0]["text"].as_str().unwrap().contains("read-only"));

        let bad = handle(&ctx, &call("check_config", json!({ "json": "{\"kpis\":{\"x\":{\"goal\":\"up\"}}}" }))).await.unwrap();
        assert_eq!(bad["result"]["structuredContent"]["valid"], false);
        let unknown = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 6, "method": "nope" })).await.unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        let _ = std::fs::remove_file(path);
    }
}
