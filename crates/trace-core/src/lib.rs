//! Trace core: storage, behaviour ingestion, KPI measurement, the digest the
//! analyst agent reads, insight lifecycle, prompt rendering and impact evaluation.

pub mod auth;
pub mod config;
pub mod db;
pub mod digest;
pub mod evaluate;
pub mod insights;
pub mod ingest;
pub mod kpi;
pub mod model;
pub mod onboarding;
pub mod plans;
pub mod privacy;
pub mod readiness;
pub mod prompt;
pub mod retention;
pub mod usage;
pub mod util;

pub use sqlx::AnyPool as Pool;
