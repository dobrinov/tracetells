//! `trace.json`: the product name, custom-event catalogue and KPIs,
//! defined in the host application's repository and synced to the server.
//!
//! ```json
//! {
//!   "product": "Stride",
//!   "events": { "purchase": "Payment succeeded" },
//!   "kpis": {
//!     "checkout_conversion": {
//!       "name": "Checkout conversion",
//!       "conversion": { "from": { "pageview": "/checkout" }, "to": { "event": "purchase" } },
//!       "goal": "increase",
//!       "target": "55%"
//!     },
//!     "js_errors": { "session_rate": { "error": "*" }, "goal": "decrease", "target": "1%" }
//!   }
//! }
//! ```
//!
//! An event matcher is either the full form `{ "type", "path", "selector", "name" }`
//! or a shorthand with one event type as the key: `{ "pageview": "/checkout" }`
//! (a path), `{ "click": "button#pay" }` (a selector), `{ "event": "purchase" }`
//! (a custom event name). Shorthands can add `path` or `selector`. Patterns
//! ending in `*` match by prefix; `"*"` matches anything.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use crate::Pool;

use crate::ingest::EVENT_TYPES;
use crate::model::{Direction, EventMatch, KpiDefinition};
use crate::util::now_ms;

pub const DEFAULT_FILE: &str = "trace.json";

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub product: Option<String>,
    /// Custom event name → what it means.
    pub events: BTreeMap<String, String>,
    pub kpis: Vec<KpiSpec>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KpiSpec {
    pub key: String,
    pub name: String,
    pub description: String,
    #[serde(flatten)]
    pub definition: KpiDefinition,
    pub direction: Direction,
    pub target: Option<f64>,
}

#[derive(Debug)]
pub struct Parsed {
    pub config: Config,
    /// Problems that don't stop the config from working, e.g. a KPI that
    /// counts a custom event the catalogue doesn't declare.
    pub warnings: Vec<String>,
}

/// Parses and validates a config. On failure, returns every problem found,
/// each prefixed with where it is in the file.
pub fn parse(text: &str) -> Result<Parsed, Vec<String>> {
    let root: Value = serde_json::from_str(text).map_err(|e| vec![format!("invalid JSON: {e}")])?;
    let mut p = Parser { errors: Vec::new(), warnings: Vec::new() };
    let config = p.root(&root);
    if p.errors.is_empty() {
        Ok(Parsed { config, warnings: p.warnings })
    } else {
        Err(p.errors)
    }
}

struct Parser {
    errors: Vec<String>,
    warnings: Vec<String>,
}

fn valid_name(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
}

impl Parser {
    fn err(&mut self, at: &str, msg: impl std::fmt::Display) {
        self.errors.push(format!("{at}: {msg}"));
    }

    fn object<'a>(&mut self, v: &'a Value, at: &str) -> Option<&'a Map<String, Value>> {
        let o = v.as_object();
        if o.is_none() {
            self.err(at, "expected an object");
        }
        o
    }

    fn root(&mut self, root: &Value) -> Config {
        let mut config = Config { product: None, events: BTreeMap::new(), kpis: Vec::new() };
        let Some(obj) = self.object(root, "(root)") else { return config };
        for key in obj.keys() {
            if !matches!(key.as_str(), "$schema" | "product" | "events" | "kpis") {
                self.err(key, "unknown key (expected product, events or kpis)");
            }
        }
        match obj.get("product") {
            None => {}
            Some(Value::String(s)) if !s.trim().is_empty() => config.product = Some(s.trim().to_string()),
            Some(_) => self.err("product", "expected a non-empty string"),
        }
        if let Some(events) = obj.get("events").and_then(|v| self.object(v, "events")) {
            for (name, v) in events {
                let at = format!("events.{name}");
                if !valid_name(name, 128) {
                    self.err(&at, "event names may only use letters, digits and _ - . :");
                    continue;
                }
                let description = match v {
                    Value::String(s) => s.clone(),
                    Value::Object(o) => o.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                    _ => {
                        self.err(&at, "expected a description string");
                        continue;
                    }
                };
                config.events.insert(name.clone(), description);
            }
        }
        if let Some(kpis) = obj.get("kpis").and_then(|v| self.object(v, "kpis")) {
            for (key, v) in kpis {
                if let Some(spec) = self.kpi(key, v, &config.events) {
                    config.kpis.push(spec);
                }
            }
        }
        if config.kpis.is_empty() && self.errors.is_empty() {
            self.warnings.push("no KPIs defined: the analyst works best when it knows what to improve".into());
        }
        config
    }

    fn kpi(&mut self, key: &str, v: &Value, events: &BTreeMap<String, String>) -> Option<KpiSpec> {
        let at = format!("kpis.{key}");
        if !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') || key.is_empty() {
            self.err(&at, "KPI keys may only use lowercase letters, digits, _ and -");
            return None;
        }
        let obj = self.object(v, &at)?;
        for k in obj.keys() {
            if !matches!(
                k.as_str(),
                "name" | "description" | "goal" | "target" | "conversion" | "session_rate" | "per_session"
            ) {
                self.err(&format!("{at}.{k}"), "unknown key");
            }
        }
        let measures: Vec<&str> = ["conversion", "session_rate", "per_session"]
            .into_iter()
            .filter(|m| obj.contains_key(*m))
            .collect();
        // Every part is checked even when an earlier one failed, so one run reports all problems.
        let definition = match measures.as_slice() {
            ["conversion"] => self.object(&obj["conversion"], &format!("{at}.conversion")).and_then(|c| {
                let from = self.matcher(c.get("from"), &format!("{at}.conversion.from"), events);
                let to = self.matcher(c.get("to"), &format!("{at}.conversion.to"), events);
                Some(KpiDefinition::Conversion { from: from?, to: to? })
            }),
            ["session_rate"] => self
                .matcher(obj.get("session_rate"), &format!("{at}.session_rate"), events)
                .map(|matcher| KpiDefinition::SessionRate { matcher }),
            ["per_session"] => self
                .matcher(obj.get("per_session"), &format!("{at}.per_session"), events)
                .map(|matcher| KpiDefinition::PerSession { matcher }),
            [] => {
                self.err(&at, "needs one of conversion, session_rate or per_session");
                None
            }
            _ => {
                self.err(&at, "has more than one of conversion, session_rate and per_session");
                None
            }
        };
        let direction = match obj.get("goal").and_then(Value::as_str) {
            Some("increase") => Some(Direction::Increase),
            Some("decrease") => Some(Direction::Decrease),
            _ => {
                self.err(&format!("{at}.goal"), "required: \"increase\" or \"decrease\"");
                None
            }
        };
        let bad_target = "expected a number or a percentage like \"55%\"";
        let target = match obj.get("target") {
            None | Some(Value::Null) => Some(None),
            Some(Value::Number(n)) => Some(n.as_f64()),
            Some(Value::String(s)) => match s.trim().strip_suffix('%').map(|p| p.trim().parse::<f64>()) {
                Some(Ok(p)) => Some(Some(p / 100.0)),
                _ => {
                    self.err(&format!("{at}.target"), bad_target);
                    None
                }
            },
            Some(_) => {
                self.err(&format!("{at}.target"), bad_target);
                None
            }
        };
        let (definition, direction, target) = (definition?, direction?, target?);
        if let Some(t) = target
            && definition.is_rate()
            && !(0.0..=1.0).contains(&t)
        {
            self.err(&format!("{at}.target"), format!("{t} is not a rate; write \"55%\" or 0.55"));
            return None;
        }
        Some(KpiSpec {
            key: key.to_string(),
            name: obj.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(key).to_string(),
            description: obj.get("description").and_then(Value::as_str).unwrap_or_default().trim().to_string(),
            definition,
            direction,
            target,
        })
    }

    fn matcher(&mut self, v: Option<&Value>, at: &str, events: &BTreeMap<String, String>) -> Option<EventMatch> {
        let Some(v) = v else {
            self.err(at, "missing event matcher");
            return None;
        };
        let obj = self.object(v, at)?;
        let text = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_string);
        let m = if let Some(kind) = text("type") {
            EventMatch { kind, path: text("path"), selector: text("selector"), name: text("name") }
        } else {
            let shorthand: Vec<&String> = obj.keys().filter(|k| !matches!(k.as_str(), "path" | "selector")).collect();
            let [key] = shorthand.as_slice() else {
                self.err(at, "expected one event type, e.g. { \"pageview\": \"/checkout\" } or { \"event\": \"purchase\" }");
                return None;
            };
            let Some(value) = obj[key.as_str()].as_str().map(str::to_string) else {
                self.err(&format!("{at}.{key}"), "expected a string pattern (\"*\" matches anything)");
                return None;
            };
            let value = Some(value).filter(|v| v != "*");
            let kind = if key.as_str() == "event" { "custom".to_string() } else { key.to_string() };
            match kind.as_str() {
                "custom" => EventMatch { kind, name: value, path: text("path"), selector: text("selector") },
                "click" | "rage_click" | "dead_click" => {
                    EventMatch { kind, selector: value.or(text("selector")), path: text("path"), name: None }
                }
                _ => EventMatch { kind, path: value.or(text("path")), selector: text("selector"), name: None },
            }
        };
        if !EVENT_TYPES.contains(&m.kind.as_str()) {
            self.err(
                at,
                format!("unknown event type {:?} (expected one of {}, or \"event\" for custom events)", m.kind, EVENT_TYPES.join(", ")),
            );
            return None;
        }
        if m.kind == "custom"
            && let Some(name) = &m.name
            && !name.ends_with('*')
            && !events.contains_key(name)
        {
            self.warnings.push(format!("{at}: custom event {name:?} is not declared under \"events\""));
        }
        Some(m)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub archived: Vec<String>,
    pub events: usize,
    pub warnings: Vec<String>,
}

type StoredKpi = (i64, Option<String>, String, String, String, String, Option<f64>, Option<i64>);

/// Applies a config: upserts KPIs by key, archives KPIs no longer in it,
/// replaces the event catalogue and records the sync.
pub async fn apply(pool: &Pool, project_id: i64, parsed: &Parsed, source: &str) -> Result<SyncReport> {
    let config = &parsed.config;
    let mut report = SyncReport { warnings: parsed.warnings.clone(), events: config.events.len(), ..Default::default() };
    let now = now_ms();
    let mut tx = pool.begin().await?;

    let stored: Vec<StoredKpi> = sqlx::query_as(
        "SELECT id, key, name, description, definition, direction, target, archived_at FROM kpis WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    let by_key: HashMap<&str, &StoredKpi> = stored.iter().filter_map(|k| k.1.as_deref().map(|key| (key, k))).collect();

    for spec in &config.kpis {
        let definition = serde_json::to_string(&spec.definition)?;
        match by_key.get(spec.key.as_str()) {
            Some((id, _, name, description, def, direction, target, archived_at)) => {
                let same = name == &spec.name
                    && description == &spec.description
                    && serde_json::from_str::<KpiDefinition>(def).ok().as_ref() == Some(&spec.definition)
                    && direction == spec.direction.as_str()
                    && *target == spec.target
                    && archived_at.is_none();
                if same {
                    report.unchanged.push(spec.key.clone());
                    continue;
                }
                sqlx::query(
                    "UPDATE kpis SET name = $1, description = $2, definition = $3, direction = $4, target = $5, archived_at = NULL WHERE id = $6",
                )
                .bind(&spec.name)
                .bind(&spec.description)
                .bind(&definition)
                .bind(spec.direction.as_str())
                .bind(crate::db::Real(spec.target))
                .bind(id)
                .execute(&mut *tx)
                .await?;
                report.updated.push(spec.key.clone());
            }
            None => {
                sqlx::query(
                    "INSERT INTO kpis (project_id, key, name, description, definition, direction, target, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(project_id)
                .bind(&spec.key)
                .bind(&spec.name)
                .bind(&spec.description)
                .bind(&definition)
                .bind(spec.direction.as_str())
                .bind(crate::db::Real(spec.target))
                .bind(now)
                .execute(&mut *tx)
                .await?;
                report.created.push(spec.key.clone());
            }
        }
    }

    // Anything not in the file (including KPIs from before KPIs moved to code) is archived.
    for (id, key, name, .., archived_at) in &stored {
        let in_config = key.as_deref().is_some_and(|k| config.kpis.iter().any(|s| s.key == k));
        if !in_config && archived_at.is_none() {
            sqlx::query("UPDATE kpis SET archived_at = $1 WHERE id = $2").bind(now).bind(id).execute(&mut *tx).await?;
            report.archived.push(key.clone().unwrap_or_else(|| name.clone()));
        }
    }

    sqlx::query("DELETE FROM event_definitions WHERE project_id = $1").bind(project_id).execute(&mut *tx).await?;
    for (name, description) in &config.events {
        sqlx::query("INSERT INTO event_definitions (project_id, name, description) VALUES ($1, $2, $3)")
            .bind(project_id)
            .bind(name)
            .bind(description)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query(
        "UPDATE projects SET name = COALESCE($1, name), config = $2, config_synced_at = $3, config_source = $4 WHERE id = $5",
    )
    .bind(config.product.as_deref())
    .bind(serde_json::to_string(config)?)
    .bind(now)
    .bind(source)
    .bind(project_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(report)
}

/// When and from where the config was last synced.
pub async fn last_sync(pool: &Pool, project_id: i64) -> Result<Option<(i64, String)>> {
    let row: Option<(Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT config_synced_at, config_source FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(at, source)| Some((at?, source.unwrap_or_default()))))
}

/// The declared event catalogue.
pub async fn events(pool: &Pool, project_id: i64) -> Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, description FROM event_definitions WHERE project_id = $1 ORDER BY name")
            .bind(project_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// One custom event: whether the config declares it and whether it's arriving.
#[derive(Debug, Serialize)]
pub struct EventHealth {
    pub name: String,
    /// From the catalogue; None for events the app sends without declaring them.
    pub description: Option<String>,
    pub count: i64,
    pub sessions: i64,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TrackingHealth {
    pub synced_at: Option<i64>,
    pub source: Option<String>,
    /// Declared events, including ones not seen in the window.
    pub events: Vec<EventHealth>,
    pub undeclared_events: Vec<EventHealth>,
}

/// Declared custom events and whether they arrived in the last `days`, plus
/// events that arrive without being declared.
pub async fn tracking_health(pool: &Pool, project_id: i64, days: i64) -> Result<TrackingHealth> {
    let since = now_ms() - days * crate::util::DAY_MS;
    let seen: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT name, COUNT(*), COUNT(DISTINCT session_id), MAX(ts) FROM events
         WHERE project_id = $1 AND type = 'custom' AND name IS NOT NULL AND ts >= $2 GROUP BY name ORDER BY 2 DESC",
    )
    .bind(project_id)
    .bind(since)
    .fetch_all(pool)
    .await?;
    let mut declared = events(pool, project_id).await?;
    let mut health = Vec::new();
    let mut undeclared = Vec::new();
    for (name, count, sessions, last_seen) in seen {
        match declared.remove(&name) {
            Some(d) => health.push(EventHealth { name, description: Some(d), count, sessions, last_seen: Some(last_seen) }),
            None => undeclared.push(EventHealth { name, description: None, count, sessions, last_seen: Some(last_seen) }),
        }
    }
    health.extend(declared.into_iter().map(|(name, d)| EventHealth { name, description: Some(d), count: 0, sessions: 0, last_seen: None }));
    let sync = last_sync(pool, project_id).await?;
    Ok(TrackingHealth {
        synced_at: sync.as_ref().map(|s| s.0),
        source: sync.map(|s| s.1),
        events: health,
        undeclared_events: undeclared,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIDE: &str = r#"{
      "product": "Stride",
      "events": { "purchase": "Payment succeeded", "signup_completed": { "description": "Account created" } },
      "kpis": {
        "checkout_conversion": {
          "name": "Checkout conversion",
          "conversion": { "from": { "pageview": "/checkout" }, "to": { "pageview": "/checkout/success" } },
          "goal": "increase", "target": "55%"
        },
        "signup_completion": {
          "conversion": { "from": { "form_start": "/signup" }, "to": { "event": "signup_completed" } },
          "goal": "increase"
        },
        "js_errors": { "session_rate": { "error": "*" }, "goal": "decrease", "target": 0.01 },
        "pay_rage": { "per_session": { "rage_click": "button#pay", "path": "/checkout" }, "goal": "decrease" }
      }
    }"#;

    #[test]
    fn parses_shorthand_and_full_forms() {
        let p = parse(STRIDE).unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        let c = &p.config;
        assert_eq!(c.product.as_deref(), Some("Stride"));
        assert_eq!(c.events["signup_completed"], "Account created");
        let by = |k: &str| c.kpis.iter().find(|s| s.key == k).unwrap();
        assert_eq!(by("checkout_conversion").target, Some(0.55));
        assert_eq!(by("signup_completion").name, "signup_completion");
        let KpiDefinition::Conversion { to, .. } = &by("signup_completion").definition else { panic!() };
        assert_eq!((to.kind.as_str(), to.name.as_deref()), ("custom", Some("signup_completed")));
        let KpiDefinition::SessionRate { matcher } = &by("js_errors").definition else { panic!() };
        assert_eq!((matcher.kind.as_str(), matcher.path.as_deref()), ("error", None));
        let KpiDefinition::PerSession { matcher } = &by("pay_rage").definition else { panic!() };
        assert_eq!(matcher.selector.as_deref(), Some("button#pay"));
        assert_eq!(matcher.path.as_deref(), Some("/checkout"));
    }

    #[test]
    fn reports_every_error_with_its_location() {
        let errors = parse(
            r#"{ "kpis": {
                 "Bad Key": { "goal": "increase", "session_rate": { "error": "*" } },
                 "a": { "conversion": { "from": { "pagview": "/x" } }, "goal": "up" },
                 "b": { "session_rate": { "error": "*" }, "goal": "increase", "target": 55 }
               }, "kpi": {} }"#,
        )
        .unwrap_err();
        let all = errors.join("\n");
        assert!(all.contains("kpi: unknown key"), "{all}");
        assert!(all.contains("kpis.Bad Key: KPI keys may only use"), "{all}");
        assert!(all.contains("kpis.a.conversion.from: unknown event type \"pagview\""), "{all}");
        assert!(all.contains("kpis.a.conversion.to: missing event matcher"), "{all}");
        assert!(all.contains("kpis.a.goal: required"), "{all}");
        assert!(all.contains("kpis.b.target: 55 is not a rate"), "{all}");
    }

    #[test]
    fn warns_about_undeclared_custom_events() {
        let p = parse(r#"{ "kpis": { "x": { "session_rate": { "event": "purchase" }, "goal": "increase" } } }"#).unwrap();
        assert_eq!(p.warnings, vec!["kpis.x.session_rate: custom event \"purchase\" is not declared under \"events\""]);
    }

    #[tokio::test]
    async fn apply_upserts_by_key_and_archives_removed_kpis() {
        let path = crate::db::test_target("config").await;
        let pool = crate::db::connect(&path).await.unwrap();
        let (_, project) = crate::db::create_account_with_site(&pool, "Stride", None).await.unwrap();

        let first = apply(&pool, project.id, &parse(STRIDE).unwrap(), "test").await.unwrap();
        assert_eq!(first.created.len(), 4);
        let id_before = crate::kpi::list(&pool, project.id).await.unwrap().iter().find(|k| k.key.as_deref() == Some("js_errors")).unwrap().id;

        let edited = STRIDE.replace("\"target\": 0.01", "\"target\": 0.02").replace(
            r#""pay_rage": { "per_session": { "rage_click": "button#pay", "path": "/checkout" }, "goal": "decrease" }"#,
            r#""bounce": { "session_rate": { "leave": "/" }, "goal": "decrease" }"#,
        );
        let second = apply(&pool, project.id, &parse(&edited).unwrap(), "test").await.unwrap();
        assert_eq!(second.updated, vec!["js_errors"]);
        assert_eq!(second.created, vec!["bounce"]);
        assert_eq!(second.archived, vec!["pay_rage"]);
        assert_eq!(second.unchanged.len(), 2);

        let kpis = crate::kpi::list(&pool, project.id).await.unwrap();
        assert_eq!(kpis.len(), 4);
        let js = kpis.iter().find(|k| k.key.as_deref() == Some("js_errors")).unwrap();
        assert_eq!((js.id, js.target), (id_before, Some(0.02)), "editing keeps the KPI's id");
        assert_eq!(crate::db::project(&pool, project.id).await.unwrap().name, "Stride");
        assert_eq!(events(&pool, project.id).await.unwrap().len(), 2);
        crate::db::drop_test_target(&path).await;
    }
}
