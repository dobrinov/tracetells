use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A website. (Internally still called a project.)
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    /// Short identifier, unique within the account.
    pub slug: String,
    pub write_key: String,
}

/// Selects events by type and, optionally, path / selector / custom-event name.
/// String patterns match exactly, or by prefix when they end in `*`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EventMatch {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EventMatch {
    pub fn matches(&self, path: Option<&str>, selector: Option<&str>, name: Option<&str>) -> bool {
        pattern_matches(&self.path, path)
            && pattern_matches(&self.selector, selector)
            && pattern_matches(&self.name, name)
    }

    pub fn describe(&self) -> String {
        let mut s = self.kind.clone();
        if let Some(n) = self.name.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!(" \"{n}\""));
        }
        if let Some(sel) = self.selector.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!(" on {sel}"));
        }
        if let Some(p) = self.path.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!(" at {p}"));
        }
        s
    }
}

pub fn pattern_matches(pattern: &Option<String>, value: Option<&str>) -> bool {
    match pattern.as_deref().map(str::trim) {
        None | Some("") | Some("*") => true,
        Some(p) => {
            let Some(v) = value else { return false };
            match p.strip_suffix('*') {
                Some(prefix) => v.starts_with(prefix),
                None => v == p,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KpiDefinition {
    /// Share of sessions with a `from` event that later have a `to` event.
    Conversion { from: EventMatch, to: EventMatch },
    /// Share of sessions that contain at least one matching event.
    SessionRate {
        #[serde(rename = "match")]
        matcher: EventMatch,
    },
    /// Average number of matching events per session.
    PerSession {
        #[serde(rename = "match")]
        matcher: EventMatch,
    },
}

impl KpiDefinition {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Conversion { .. } => "conversion",
            Self::SessionRate { .. } => "session_rate",
            Self::PerSession { .. } => "per_session",
        }
    }

    pub fn matchers(&self) -> Vec<&EventMatch> {
        match self {
            Self::Conversion { from, to } => vec![from, to],
            Self::SessionRate { matcher } | Self::PerSession { matcher } => vec![matcher],
        }
    }

    pub fn is_rate(&self) -> bool {
        !matches!(self, Self::PerSession { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Conversion { from, to } => {
                format!("sessions with {} that go on to {}", from.describe(), to.describe())
            }
            Self::SessionRate { matcher } => format!("share of sessions with {}", matcher.describe()),
            Self::PerSession { matcher } => format!("{} per session", matcher.describe()),
        }
    }

    pub fn format_value(&self, v: f64) -> String {
        if self.is_rate() {
            crate::util::pct(v)
        } else {
            format!("{v:.2}")
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Increase,
    Decrease,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Increase => "increase",
            Self::Decrease => "decrease",
        }
    }

    /// +1 when a rising value is good, -1 when a falling value is good.
    pub fn sign(self) -> f64 {
        match self {
            Self::Increase => 1.0,
            Self::Decrease => -1.0,
        }
    }
}

impl FromStr for Direction {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "increase" => Ok(Self::Increase),
            "decrease" => Ok(Self::Decrease),
            other => anyhow::bail!("unknown direction {other:?}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Kpi {
    pub id: i64,
    /// Stable key from trace.json.
    pub key: Option<String>,
    pub name: String,
    pub description: String,
    #[serde(flatten)]
    pub definition: KpiDefinition,
    pub direction: Direction,
    pub target: Option<f64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InsightStatus {
    New,
    Selected,
    InProgress,
    Released,
    Dismissed,
}

impl InsightStatus {
    pub const ALL: [InsightStatus; 5] = [
        Self::New,
        Self::Selected,
        Self::InProgress,
        Self::Released,
        Self::Dismissed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Selected => "selected",
            Self::InProgress => "in_progress",
            Self::Released => "released",
            Self::Dismissed => "dismissed",
        }
    }
}

impl fmt::Display for InsightStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InsightStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| anyhow::anyhow!("unknown status {s:?} (expected one of new, selected, in_progress, released, dismissed)"))
    }
}

/// One piece of an insight's evidence: a claim with the measurement behind
/// it, which Trace takes itself, or a written note ("not measured").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
// An insight has at most ten of these, so the measured variant's size doesn't matter.
#[allow(clippy::large_enum_variant)]
pub enum Evidence {
    Measured {
        claim: String,
        metric: crate::query::Metric,
        /// Which way the number moves if the problem is fixed.
        goal: Direction,
    },
    Note { note: String },
}

impl Evidence {
    pub fn note(text: impl Into<String>) -> Self {
        Self::Note { note: text.into() }
    }
    pub fn text(&self) -> &str {
        match self {
            Self::Measured { claim, .. } => claim,
            Self::Note { note } => note,
        }
    }
    pub fn metric(&self) -> Option<&crate::query::Metric> {
        match self {
            Self::Measured { metric, .. } => Some(metric),
            Self::Note { .. } => None,
        }
    }
}

/// One measurement of an evidence item (`item` is its index).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub item: usize,
    /// first, refresh, before or after (a release).
    pub kind: String,
    pub value: Option<f64>,
    pub formatted: Option<String>,
    /// The sessions the value is about.
    pub sessions: i64,
    pub from: i64,
    pub to: i64,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Insight {
    pub id: i64,
    pub title: String,
    pub problem: String,
    pub evidence: Vec<Evidence>,
    pub proposal: String,
    pub acceptance_criteria: Vec<String>,
    pub pages: Vec<String>,
    pub kpi_id: Option<i64>,
    pub expected_impact: String,
    pub confidence: String,
    pub status: InsightStatus,
    pub dismiss_reason: Option<String>,
    pub source: String,
    pub fingerprint: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub released_at: Option<i64>,
    pub evaluation: Option<Evaluation>,
    /// Who created it: a person and the app they used, or which part of Trace.
    pub created_by: Option<String>,
    /// The window its evidence was first measured over.
    pub measured_from: Option<i64>,
    pub measured_to: Option<i64>,
    /// Every measurement of its evidence, oldest first.
    pub readings: Vec<Reading>,
}

/// An insight produced by the analyst (LLM or heuristics) before it is stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewInsight {
    pub title: String,
    pub problem: String,
    pub evidence: Vec<Evidence>,
    pub proposal: String,
    pub acceptance_criteria: Vec<String>,
    pub pages: Vec<String>,
    pub kpi_id: Option<i64>,
    pub expected_impact: String,
    pub confidence: String,
    pub source: String,
    /// Stable key for heuristic findings so re-running analysis does not duplicate them.
    pub fingerprint: Option<String>,
    pub created_by: Option<String>,
    /// The first measurements, when they were taken before saving (a draft).
    #[serde(default)]
    pub readings: Vec<Reading>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The window after the release hasn't passed yet.
    Measuring,
    Improved,
    Regressed,
    NoChange,
    InsufficientData,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Measuring => "measuring",
            Self::Improved => "improved",
            Self::Regressed => "regressed",
            Self::NoChange => "no_change",
            Self::InsufficientData => "insufficient_data",
        }
    }
}

impl FromStr for Verdict {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Ok(match s {
            "measuring" => Self::Measuring,
            "improved" => Self::Improved,
            "regressed" => Self::Regressed,
            "no_change" => Self::NoChange,
            "insufficient_data" => Self::InsufficientData,
            other => anyhow::bail!("unknown verdict {other:?}"),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub insight_id: i64,
    pub kpi_id: i64,
    pub baseline: Option<f64>,
    pub current: Option<f64>,
    pub baseline_n: i64,
    pub current_n: i64,
    pub p_value: Option<f64>,
    pub verdict: Verdict,
    pub summary: String,
    pub window_days: f64,
    pub created_at: i64,
    /// When the verdict became final; none while measuring.
    pub settled_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        let p = |s: &str| Some(s.to_string());
        assert!(pattern_matches(&None, None));
        assert!(pattern_matches(&p("/checkout"), Some("/checkout")));
        assert!(!pattern_matches(&p("/checkout"), Some("/checkout/success")));
        assert!(pattern_matches(&p("/checkout*"), Some("/checkout/success")));
        assert!(!pattern_matches(&p("/checkout"), None));
    }
}
