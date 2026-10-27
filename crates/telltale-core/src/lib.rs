//! Telltale core: storage, behaviour ingestion, KPI measurement, the digest the
//! analyst agent reads, idea lifecycle, prompt rendering and impact evaluation.

pub mod db;
pub mod digest;
pub mod evaluate;
pub mod ideas;
pub mod ingest;
pub mod kpi;
pub mod model;
pub mod prompt;
pub mod util;

pub use sqlx::SqlitePool;
