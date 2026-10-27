//! The Telltale analyst. Reads a behaviour digest plus the project's KPIs and
//! existing ideas, and proposes new improvement ideas.

pub mod analyst;
pub mod claude;
pub mod heuristics;

pub use analyst::{Engine, Options, RunReport, analyze};
pub use claude::ClaudeConfig;
