use std::time::Duration;

use anyhow::Result;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::model::Project;
use crate::util::{new_key, now_ms};

const SCHEMA: &str = include_str!("schema.sql");

pub async fn connect(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    sqlx::raw_sql(SCHEMA).execute(&pool).await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Brings databases created by earlier versions up to the current schema.
async fn migrate(pool: &SqlitePool) -> Result<()> {
    for (table, column, ty) in [
        ("projects", "config", "TEXT"),
        ("projects", "config_synced_at", "INTEGER"),
        ("projects", "config_source", "TEXT"),
        ("projects", "last_event_at", "INTEGER"),
        ("projects", "require_mfa", "INTEGER NOT NULL DEFAULT 0"),
        ("kpis", "key", "TEXT"),
        ("kpis", "archived_at", "INTEGER"),
    ] {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM pragma_table_info(?) WHERE name = ?")
            .bind(table)
            .bind(column)
            .fetch_optional(pool)
            .await?;
        if exists.is_none() {
            sqlx::raw_sql(&format!("ALTER TABLE {table} ADD COLUMN {column} {ty}")).execute(pool).await?;
        }
    }
    sqlx::raw_sql("CREATE UNIQUE INDEX IF NOT EXISTS kpis_project_key ON kpis(project_id, key)").execute(pool).await?;
    Ok(())
}

/// The community edition runs a single project; it is created on first start.
pub async fn default_project(pool: &SqlitePool) -> Result<Project> {
    if let Some(p) = first_project(pool).await? {
        return Ok(p);
    }
    sqlx::query("INSERT INTO projects (name, write_key, created_at) VALUES (?, ?, ?)")
        .bind("My product")
        .bind(new_key())
        .bind(now_ms())
        .execute(pool)
        .await?;
    Ok(first_project(pool).await?.expect("project was just inserted"))
}

async fn first_project(pool: &SqlitePool) -> Result<Option<Project>> {
    let row: Option<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, write_key FROM projects ORDER BY id LIMIT 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id, name, write_key)| Project { id, name, write_key }))
}

pub async fn project(pool: &SqlitePool, id: i64) -> Result<Project> {
    let (id, name, write_key): (i64, String, String) =
        sqlx::query_as("SELECT id, name, write_key FROM projects WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(Project { id, name, write_key })
}

pub async fn project_by_key(pool: &SqlitePool, key: &str) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar("SELECT id FROM projects WHERE write_key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?)
}


/// Whether the collector is delivering data, and from which sites.
#[derive(Debug, serde::Serialize)]
pub struct Collection {
    pub last_event_at: Option<i64>,
    /// Events arrived within the last 24 hours.
    pub active: bool,
    pub sources: Vec<Source>,
}

#[derive(Debug, serde::Serialize)]
pub struct Source {
    pub origin: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

pub async fn collection(pool: &SqlitePool, project_id: i64) -> Result<Collection> {
    let last_event_at: Option<i64> = sqlx::query_scalar("SELECT last_event_at FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    let sources = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT origin, first_seen, last_seen FROM event_sources WHERE project_id = ? ORDER BY last_seen DESC LIMIT 10",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(origin, first_seen, last_seen)| Source { origin, first_seen, last_seen })
    .collect();
    Ok(Collection {
        active: last_event_at.is_some_and(|t| now_ms() - t < crate::util::DAY_MS),
        last_event_at,
        sources,
    })
}
