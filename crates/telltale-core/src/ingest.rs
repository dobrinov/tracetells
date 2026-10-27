//! Accepts event batches from the browser collector (`t.js`).

use anyhow::{Result, bail};
use serde::Deserialize;
use sqlx::SqlitePool;

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
    pool: &SqlitePool,
    project_id: i64,
    batch: Batch,
    user_agent: Option<&str>,
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
        .map(|p| truncate(p, 512));

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO sessions (id, project_id, visitor_id, user_id, started_at, last_seen_at, device, user_agent, entry_path, referrer)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           last_seen_at = MAX(sessions.last_seen_at, excluded.last_seen_at),
           user_id = COALESCE(excluded.user_id, sessions.user_id),
           entry_path = COALESCE(sessions.entry_path, excluded.entry_path)
         WHERE sessions.project_id = excluded.project_id",
    )
    .bind(&batch.sid)
    .bind(project_id)
    .bind(batch.vid.as_deref().map(|v| truncate(v, 64)))
    .bind(batch.uid.as_deref().filter(|v| !v.is_empty()).map(|v| truncate(v, 128)))
    .bind(first_ts)
    .bind(last_ts)
    .bind(device(batch.vw))
    .bind(user_agent.map(|v| truncate(v, 300)))
    .bind(entry_path)
    .bind(batch.referrer.as_deref().filter(|v| !v.is_empty()).map(|v| truncate(v, 512)))
    .execute(&mut *tx)
    .await?;

    let count = events.len();
    for (ts, e) in events {
        let props = e
            .props
            .filter(|p| !p.is_null())
            .map(|p| p.to_string())
            .filter(|p| p.len() <= MAX_PROPS_BYTES);
        sqlx::query(
            "INSERT INTO events (project_id, session_id, ts, type, path, selector, text, name, value, props)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(&batch.sid)
        .bind(ts)
        .bind(&e.kind)
        .bind(e.path.as_deref().map(|v| truncate(v, 512)))
        .bind(e.sel.as_deref().map(|v| truncate(v, 512)))
        .bind(e.text.as_deref().map(|v| truncate(v, 300)))
        .bind(e.name.as_deref().map(|v| truncate(v, 128)))
        .bind(e.value.filter(|v| v.is_finite()))
        .bind(props)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(count)
}
