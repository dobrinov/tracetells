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
    Ok(pool)
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

pub async fn rename_project(pool: &SqlitePool, id: i64, name: &str) -> Result<()> {
    sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
