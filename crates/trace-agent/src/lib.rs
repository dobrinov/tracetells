//! The Trace analyst. Reads a behaviour digest plus the project's KPIs and
//! existing insights, and proposes new improvement insights.

pub mod analyst;
pub mod claude;
pub mod heuristics;

pub use analyst::{Engine, Options, RunReport, analyze};
pub use claude::ClaudeConfig;
