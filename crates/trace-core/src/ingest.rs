//! Accepts event batches from the browser collector (`d.js`). Every field is
//! scrubbed with [`crate::privacy`] before it is stored.

use anyhow::{Result, bail};
use serde::Deserialize;
use crate::Pool;

use crate::privacy::{HostRules, clean_props_with, clean_user_id, redact_path, redact_with, referrer_origin};
use crate::util::{DAY_MS, now_ms, truncate};

pub const EVENT_TYPES: &[&str] = &[
    "pageview",
    "click",
    "rage_click",
    "dead_click",
    "error",
    "scroll",
    "leave",
    "form_start",
    "submit",
    "custom",
];

const MAX_EVENTS_PER_BATCH: usize = 500;
const MAX_PROPS_BYTES: usize = 2048;

#[derive(Debug, Deserialize)]
pub struct Batch {
    pub key: String,
    pub sid: String,
    #[serde(default)]
    pub vid: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    /// Viewport width, used to bucket the device.
    #[serde(default)]
    pub vw: Option<u32>,
    #[serde(default, rename = "ref")]
    pub referrer: Option<String>,
    /// Client clock when the batch was sent; used to correct clock skew.
    #[serde(default)]
    pub now: Option<i64>,
    #[serde(default)]
    pub events: Vec<IncomingEvent>,
}

#[derive(Debug, Deserialize)]
pub struct IncomingEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub t: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sel: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub props: Option<serde_json::Value>,
}

fn device(vw: Option<u32>) -> Option<&'static str> {
    vw.map(|w| match w {
        0..768 => "mobile",
        768..1024 => "tablet",
        _ => "desktop",
    })
}

/// Stores a batch and returns how many events were kept.
pub async fn store(
    pool: &Pool,
    project_id: i64,
    batch: Batch,
    user_agent: Option<&str>,
    // The page's origin (from the Origin or Referer header): which site is sending.
    origin: Option<&str>,
    // The workspace's own privacy rules, applied on top of the built-in ones.
    rules: &HostRules,
) -> Result<usize> {
    if batch.sid.is_empty() || batch.sid.len() > 64 {
        bail!("invalid session id");
    }
    let server_now = now_ms();
    let skew = batch.now.map(|c| server_now - c).unwrap_or(0);
    let events: Vec<_> = batch
        .events
        .into_iter()
        .filter(|e| EVENT_TYPES.contains(&e.kind.as_str()))
        // Ignored pages: nothing from them is kept, whatever the client sent.
        .filter(|e| {
            e.path.as_deref().is_none_or(|p| !rules.ignores(p) && !rules.ignores(&redact_path(p)))
        })
        .take(MAX_EVENTS_PER_BATCH)
        .map(|e| {
            let ts = e
                .t
                .map(|t| t + skew)
                .filter(|t| (server_now - DAY_MS..=server_now + 60_000).contains(t))
                .unwrap_or(server_now);
            (ts, e)
        })
        .collect();
    if events.is_empty() {
        return Ok(0);
    }

    let first_ts = events.iter().map(|(ts, _)| *ts).min().unwrap_or(server_now);
    let last_ts = events.iter().map(|(ts, _)| *ts).max().unwrap_or(server_now);
    let entry_path = events
        .iter()
        .find(|(_, e)| e.kind == "pageview")
        .and_then(|(_, e)| e.path.as_deref())
        .map(|p| redact_path(&truncate(p, 512)));

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at, device, user_agent, entry_path, referrer)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT(id) DO UPDATE SET
           last_seen_at = CASE WHEN excluded.last_seen_at > sessions.last_seen_at THEN excluded.last_seen_at ELSE sessions.last_seen_at END,
           user_id = COALESCE(excluded.user_id, sessions.user_id),
           entry_path = COALESCE(sessions.entry_path, excluded.entry_path)
         WHERE sessions.project_id = excluded.project_id",
    )
    .bind(&batch.sid)
    .bind(project_id)
    .bind(batch.vid.as_deref().map(|v| truncate(v, 64)))
    .bind(batch.uid.as_deref().and_then(clean_user_id))
    .bind(first_ts)
    .bind(last_ts)
    .bind(device(batch.vw))
    .bind(user_agent.map(|v| truncate(v, 300)))
    .bind(entry_path)
    .bind(batch.referrer.as_deref().and_then(referrer_origin))
    .execute(&mut *tx)
    .await?;

    let count = events.len();
    for (ts, e) in events {
        let props = e
            .props
            .and_then(|p| clean_props_with(rules, p, 0))
            .map(|p| p.to_string())
            .filter(|p| p.len() <= MAX_PROPS_BYTES);
        // Page text (labels, button text) gets strict redaction: every digit is masked.
        // Error messages and event names keep short numbers such as status codes.
        let strict = !matches!(e.kind.as_str(), "error" | "custom");
        sqlx::query(
            "INSERT INTO events (project_id, session_id, ts, type, path, selector, text, name, value, props)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(project_id)
        .bind(&batch.sid)
        .bind(ts)
        .bind(&e.kind)
        .bind(e.path.as_deref().map(|v| redact_path(&truncate(v, 512))))
        .bind(e.sel.as_deref().map(|v| redact_with(rules, &truncate(v, 512), false)))
        .bind(e.text.as_deref().map(|v| redact_with(rules, &truncate(v, 300), strict)))
        .bind(e.name.as_deref().map(|v| redact_with(rules, &truncate(v, 128), false)))
        .bind(crate::db::Real(e.value.filter(|v| v.is_finite())))
        .bind(props)
        .execute(&mut *tx)
        .await?;
    }
    // Proof of collection. Writes are skipped when the value is under a minute
    // old, so busy sites don't turn every batch into extra writes.
    sqlx::query("UPDATE projects SET last_event_at = $1 WHERE id = $2 AND (last_event_at IS NULL OR last_event_at < $1 - 60000)")
        .bind(server_now)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    if let Some(origin) = origin.and_then(referrer_origin) {
        sqlx::query(
            "INSERT INTO event_sources (project_id, origin, first_seen, last_seen) VALUES ($1, $2, $3, $3)
             ON CONFLICT(project_id, origin) DO UPDATE SET last_seen = excluded.last_seen
             WHERE event_sources.last_seen < excluded.last_seen - 60000",
        )
        .bind(project_id)
        .bind(origin)
        .bind(server_now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(count)
}
