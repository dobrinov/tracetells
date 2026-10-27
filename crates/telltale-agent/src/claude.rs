//! Minimal Claude Messages API client (raw HTTP; there is no official Rust SDK).

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Models that accept `fallbacks: "default"` on the first-party API.
const FALLBACK_MODELS: &[&str] = &["claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-sonnet-5-5"];
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    /// `output_config.effort`: low, medium, high, xhigh or max.
    pub effort: String,
}

impl ClaudeConfig {
    /// Reads `ANTHROPIC_API_KEY`, plus optional `TELLTALE_MODEL`,
    /// `TELLTALE_EFFORT` and `ANTHROPIC_BASE_URL`.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Some(Self {
            api_key: var("ANTHROPIC_API_KEY")?,
            model: var("TELLTALE_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into()),
            base_url: var("ANTHROPIC_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            effort: var("TELLTALE_EFFORT").unwrap_or_else(|| "high".into()),
        })
    }

    fn fallbacks_enabled(&self) -> bool {
        self.base_url.trim_end_matches('/') == DEFAULT_BASE_URL && FALLBACK_MODELS.contains(&self.model.as_str())
    }
}

#[derive(Deserialize)]
struct MessageResponse {
    model: String,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    #[serde(default)]
    stop_details: Option<Value>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

pub struct Structured<T> {
    pub value: T,
    /// The model that produced the answer (differs from the request if a fallback served it).
    pub model: String,
}

/// Sends one request whose answer is constrained to `schema` via
/// `output_config.format` and parses it into `T`.
pub async fn structured<T: DeserializeOwned>(
    cfg: &ClaudeConfig,
    system: &str,
    user: &str,
    schema: Value,
) -> Result<Structured<T>> {
    let mut body = json!({
        "model": cfg.model,
        "max_tokens": 16000,
        "system": system,
        "messages": [{"role": "user", "content": user}],
        "output_config": {
            "effort": cfg.effort,
            "format": {"type": "json_schema", "schema": schema},
        },
    });
    if cfg.fallbacks_enabled() {
        body["fallbacks"] = json!("default");
    }

    let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let mut attempt = 0;
    let resp: MessageResponse = loop {
        attempt += 1;
        let mut req = client
            .post(&url)
            .header("x-api-key", &cfg.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body);
        if cfg.fallbacks_enabled() {
            req = req.header("anthropic-beta", FALLBACK_BETA);
        }
        match req.send().await {
            Ok(res) if res.status().is_success() => break res.json().await.context("decoding Claude response")?,
            Ok(res) => {
                let status = res.status();
                let retry_after = res
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let text = res.text().await.unwrap_or_default();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if !retryable || attempt >= 3 {
                    bail!("Claude API returned {status}: {text}");
                }
                let wait = retry_after.unwrap_or(2u64.pow(attempt));
                tracing::warn!(%status, wait, "Claude API busy, retrying");
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Err(e) if attempt < 3 && (e.is_connect() || e.is_timeout()) => {
                tracing::warn!(error = %e, "Claude API unreachable, retrying");
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
            }
            Err(e) => return Err(anyhow!(e).context("calling Claude API")),
        }
    };

    match resp.stop_reason.as_deref() {
        Some("refusal") => bail!(
            "Claude declined the request ({})",
            resp.stop_details.map(|d| d.to_string()).unwrap_or_else(|| "no details".into())
        ),
        Some("max_tokens") => bail!("Claude's answer was cut off at max_tokens"),
        _ => {}
    }
    let text: String = resp
        .content
        .iter()
        .filter(|b| b.kind == "text")
        .filter_map(|b| b.text.as_deref())
        .collect();
    let value = serde_json::from_str(&text).with_context(|| format!("Claude returned invalid JSON: {text}"))?;
    Ok(Structured { value, model: resp.model })
}
