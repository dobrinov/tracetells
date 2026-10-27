//! Diagnostic core: storage, behaviour ingestion, KPI measurement, the digest the
//! analyst agent reads, idea lifecycle, prompt rendering and impact evaluation.

pub mod auth;
pub mod config;
pub mod db;
pub mod digest;
pub mod evaluate;
pub mod ideas;
pub mod ingest;
pub mod kpi;
pub mod model;
pub mod privacy;
pub mod prompt;
pub mod retention;
pub mod util;

pub use sqlx::SqlitePool;
