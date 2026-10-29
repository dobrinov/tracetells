use anyhow::{Context, Result, bail};
use sqlx::any::AnyPoolOptions;

use crate::Pool;
use crate::model::Project;
use crate::util::{new_key, now_ms};

const SCHEMA: &str = include_str!("schema.sql");
const STRUCTURE: &str = include_str!("structure.sql");

/// True for a Postgres connection URL (`postgres://` or `postgresql://`).
pub fn is_postgres_url(target: &str) -> bool {
    target.starts_with("postgres://") || target.starts_with("postgresql://")
}

/// A database target as it may be shown or logged: the password in a
/// Postgres URL is replaced, so `postgres://trace:s3cret@db/trace` becomes
/// `Postgres postgres://trace:***@db/trace`, and a path `SQLite trace.db`.
pub fn describe(target: &str) -> String {
    if !is_postgres_url(target) {
        return format!("SQLite {}", target.strip_prefix("sqlite://").unwrap_or(target));
    }
    let (scheme, rest) = target.split_once("://").unwrap_or(("postgres", target));
    let (before_query, query) = rest.split_once('?').map_or((rest, None), |(b, q)| (b, Some(q)));
    // The last `@` before the query ends the credentials, even if the password has one.
    let base = match before_query.rsplit_once('@') {
        Some((user_info, host)) => match user_info.split_once(':') {
            Some((user, _password)) => format!("{scheme}://{user}:***@{host}"),
            None => format!("{scheme}://{before_query}"),
        },
        None => format!("{scheme}://{before_query}"),
    };
    let redacted = match query {
        Some(q) => format!("{base}?{q}"),
        None => base,
    };
    // A password can also come as a query parameter.
    let redacted = match redacted.split_once('?') {
        Some((base, query)) => {
            let query: Vec<String> = query
                .split('&')
                .map(|kv| if kv.starts_with("password=") { "password=***".to_string() } else { kv.to_string() })
                .collect();
            format!("{base}?{}", query.join("&"))
        }
        None => redacted,
    };
    format!("Postgres {redacted}")
}

/// True when the pool talks to Postgres rather than SQLite.
pub fn is_postgres(pool: &Pool) -> bool {
    is_postgres_url(pool.connect_options().database_url.as_str())
}

/// A yes/no column, stored as 0/1 on both databases: the `Any` driver can't
/// read a SQLite integer as `bool`, and Postgres won't store a `bool`
/// parameter in an integer column. Use it in row types and binds instead of
/// `bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flag(pub bool);

impl sqlx::Type<sqlx::Any> for Flag {
    fn type_info() -> sqlx::any::AnyTypeInfo {
        <i64 as sqlx::Type<sqlx::Any>>::type_info()
    }
    fn compatible(ty: &sqlx::any::AnyTypeInfo) -> bool {
        <i64 as sqlx::Type<sqlx::Any>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Any> for Flag {
    fn decode(value: sqlx::any::AnyValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        Ok(Flag(<i64 as sqlx::Decode<sqlx::Any>>::decode(value)? != 0))
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Any> for Flag {
    fn encode_by_ref(&self, buf: &mut sqlx_core::any::AnyArgumentBuffer<'q>) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<sqlx::Any>>::encode(i64::from(self.0), buf)
    }
}

/// A nullable decimal parameter. Bind `Real(value)` instead of an
/// `Option<f64>`: sqlx 0.8.6 (and 0.9.0) send a NULL `f64` to Postgres typed
/// as `f32`, because `any/arguments.rs` swaps the `Real` and `Double` NULL
/// cases, and the cached statement then rejects the next real value
/// ("incorrect binary data format"). This sends the NULL through the
/// swapped case, so Postgres sees an 8-byte double either way.
/// `nullable_reals_round_trip` fails if sqlx changes that mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Real(pub Option<f64>);

impl sqlx::Type<sqlx::Any> for Real {
    fn type_info() -> sqlx::any::AnyTypeInfo {
        <f64 as sqlx::Type<sqlx::Any>>::type_info()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Any> for Real {
    fn encode_by_ref(&self, buf: &mut sqlx_core::any::AnyArgumentBuffer<'q>) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        use sqlx_core::any::AnyValueKind;
        Ok(match self.0 {
            Some(v) => {
                buf.0.push(AnyValueKind::Double(v));
                sqlx::encode::IsNull::No
            }
            None => {
                buf.0.push(AnyValueKind::Null(sqlx::any::AnyTypeInfoKind::Real));
                sqlx::encode::IsNull::Yes
            }
        })
    }
}

/// Lets one server at a time create or upgrade the schema, so several
/// starting together against one Postgres database don't race.
const SCHEMA_LOCK: i64 = 0x7472_6163_6500; // "trace\0"

/// Opens the database and brings its schema up to date. `target` is a SQLite
/// file path (the default, `trace.db`) or a `postgres://` URL. Queries use `$1`
/// placeholders and only SQL both databases understand. Yes/no columns are
/// 0/1 integers on both ([`Flag`]), and nullable decimals are bound as
/// [`Real`]; see those types for why.
pub async fn connect(target: &str) -> Result<Pool> {
    sqlx::any::install_default_drivers();
    if is_postgres_url(target) {
        let pool = AnyPoolOptions::new().max_connections(16).connect(target).await?;
        let mut lock = pool.acquire().await?;
        sqlx::query("SELECT pg_advisory_lock($1)").bind(SCHEMA_LOCK).execute(&mut *lock).await?;
        let upgraded = async {
            sqlx::raw_sql(STRUCTURE).execute(&pool).await?;
            migrate(&pool, target).await
        }
        .await;
        // Closing the connection releases the lock, whether or not the upgrade worked.
        lock.close().await?;
        upgraded?;
        return Ok(pool);
    }
    let path = target.strip_prefix("sqlite://").unwrap_or(target);
    let memory = path == ":memory:";
    let pool = if memory {
        // Through the Any driver each new connection would open its own,
        // empty in-memory database, so use exactly one and never close it.
        AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect("sqlite::memory:")
            .await?
    } else {
        // sqlx turns on foreign keys and a 5 s busy timeout for every SQLite connection.
        AnyPoolOptions::new().max_connections(8).connect(&sqlite_url(path)).await?
    };
    // Another process (a second server, a cron `trace evaluate`) may be
    // setting up the same file. SQLite reports some of those conflicts as
    // "database is locked" at once instead of waiting, and every step below
    // is safe to repeat, so try again for a few seconds.
    let mut attempt = 0;
    loop {
        match setup_sqlite(&pool, path, memory).await {
            Err(e) if attempt < 50 && is_busy(&e) => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            done => return done.map(|()| pool),
        }
    }
}

async fn setup_sqlite(pool: &Pool, path: &str, memory: bool) -> Result<()> {
    if !memory {
        // WAL is a property of the file, so setting it once is enough.
        sqlx::raw_sql("PRAGMA journal_mode = WAL").execute(pool).await?;
    }
    rename_ideas_to_insights(pool).await?;
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    migrate(pool, path).await
}

/// SQLite's SQLITE_BUSY ("database is locked").
fn is_busy(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| match cause.downcast_ref::<sqlx::Error>() {
        Some(sqlx::Error::Database(db)) => db.code().as_deref() == Some("5") || db.message().contains("database is locked"),
        _ => false,
    })
}

/// A `sqlite://` URL for a file path. sqlx percent-decodes the path, so
/// characters that would end it or decode (`?`, `#`, `%`, spaces) are encoded.
fn sqlite_url(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_graphic() && !b"%?#\"<>`{}".contains(&b) {
            encoded.push(b as char);
        } else {
            // Spaces, control characters, the bytes above and each byte of a non-ASCII character.
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("sqlite://{encoded}?mode=rwc")
}

async fn has_table(pool: &Pool, table: &str) -> Result<bool> {
    let sql = if is_postgres(pool) {
        "SELECT 1 FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = $1"
    } else {
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = $1"
    };
    let exists: Option<i64> = sqlx::query_scalar(sql).bind(table).fetch_optional(pool).await?;
    Ok(exists.is_some())
}

/// Databases from before the rename call insights "ideas". Runs before the
/// schema, which would otherwise create an empty `insights` table next to them.
async fn rename_ideas_to_insights(pool: &Pool) -> Result<()> {
    if !has_table(pool, "ideas").await? || has_table(pool, "insights").await? {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    sqlx::raw_sql("ALTER TABLE ideas RENAME TO insights; DROP INDEX IF EXISTS ideas_project_status;").execute(&mut *tx).await?;
    for (table, old, new) in [("evaluations", "idea_id", "insight_id"), ("analysis_runs", "ideas_created", "insights_created")] {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM pragma_table_info($1) WHERE name = $2")
            .bind(table)
            .bind(old)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_some() {
            sqlx::raw_sql(&format!("ALTER TABLE {table} RENAME COLUMN {old} TO {new}")).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn has_column(pool: &Pool, table: &str, column: &str) -> Result<bool> {
    let sql = if is_postgres(pool) {
        "SELECT 1 FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = $1 AND column_name = $2"
    } else {
        "SELECT 1 FROM pragma_table_info($1) WHERE name = $2"
    };
    let exists: Option<i64> = sqlx::query_scalar(sql).bind(table).bind(column).fetch_optional(pool).await?;
    Ok(exists.is_some())
}

/// Brings databases created by earlier versions up to the current schema.
/// Postgres support arrived with the current schema, so only SQLite has
/// older layouts to upgrade; the column list runs on both so that columns
/// added from now on reach existing Postgres databases too.
async fn migrate(pool: &Pool, path: &str) -> Result<()> {
    let pg = is_postgres(pool);
    for (table, column, ty) in [
        ("projects", "config", "TEXT"),
        ("projects", "config_synced_at", "INTEGER"),
        ("projects", "config_source", "TEXT"),
        ("projects", "last_event_at", "INTEGER"),
        ("projects", "account_id", "INTEGER REFERENCES accounts(id)"),
        ("projects", "slug", "TEXT"),
        ("projects", "privacy_reviewed_at", "INTEGER"),
        ("projects", "privacy_reviewed_by", "TEXT"),
        ("projects", "analysed_at", "INTEGER"),
        ("projects", "analysed_by", "TEXT"),
        ("projects", "public_token", "TEXT"),
        ("projects", "public_since", "INTEGER"),
        ("projects", "public_by", "TEXT"),
        ("kpis", "key", "TEXT"),
        ("kpis", "archived_at", "INTEGER"),
        ("insights", "released_definition", "TEXT"),
        ("insights", "released_direction", "TEXT"),
        ("evaluations", "settled_at", "INTEGER"),
        ("insights", "created_by", "TEXT"),
        ("insights", "measured_from", "INTEGER"),
        ("insights", "measured_to", "INTEGER"),
        ("auth_sessions", "account_id", "INTEGER"),
        ("auth_sessions", "impersonator_id", "INTEGER"),
    ] {
        if !has_column(pool, table, column).await? {
            let ty = if pg { ty.replace("INTEGER", "BIGINT").replace("REAL", "DOUBLE PRECISION") } else { ty.to_string() };
            sqlx::raw_sql(&format!("ALTER TABLE {table} ADD COLUMN {column} {ty}")).execute(pool).await?;
        }
    }
    if pg {
        // The first Postgres schema made evaluations.insight_id an identity
        // column by mistake; it's a foreign key and never generated.
        sqlx::raw_sql("ALTER TABLE evaluations ALTER COLUMN insight_id DROP IDENTITY IF EXISTS").execute(pool).await?;
    }
    crate::plans::seed(pool, std::env::var("TRACE_DEFAULT_PLAN").ok().as_deref()).await?;
    if !pg && has_column(pool, "users", "project_id").await? {
        upgrade_to_accounts(pool, path).await?;
    }
    // People from before memberships belong to the account and role on their
    // own row. Afterwards nobody exists without a membership, so this finds no
    // one to copy and costs one cheap query per start.
    sqlx::query(
        "INSERT INTO memberships (user_id, account_id, role, created_at)
         SELECT u.id, u.account_id, u.role, u.created_at FROM users u
         WHERE NOT EXISTS (SELECT 1 FROM memberships m WHERE m.user_id = u.id)",
    )
    .execute(pool)
    .await?;
    sqlx::raw_sql(
        "CREATE UNIQUE INDEX IF NOT EXISTS kpis_project_key ON kpis(project_id, key);
         CREATE UNIQUE INDEX IF NOT EXISTS projects_account_slug ON projects(account_id, slug);
         CREATE UNIQUE INDEX IF NOT EXISTS projects_public_token ON projects(public_token);
         CREATE INDEX IF NOT EXISTS users_account ON users(account_id);
         CREATE INDEX IF NOT EXISTS sessions_project_visitor ON sessions(project_id, visitor_id, last_seen_at);
         CREATE INDEX IF NOT EXISTS audit_account ON audit_log(account_id, id);",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One workspace per server becomes one account per website, with people,
/// invites, tokens and the activity log moved to the account. Runs in one
/// transaction after copying the database to `<path>.pre-accounts`.
async fn upgrade_to_accounts(pool: &Pool, path: &str) -> Result<()> {
    let projects: Vec<(i64, String, Option<i64>)> =
        sqlx::query_as("SELECT id, name, account_id FROM projects ORDER BY id").fetch_all(pool).await?;
    let duplicates: Vec<String> =
        sqlx::query_scalar("SELECT lower(email) FROM users GROUP BY lower(email) HAVING COUNT(*) > 1").fetch_all(pool).await?;
    if !duplicates.is_empty() {
        bail!(
            "can't upgrade to accounts: these email addresses are used in more than one workspace: {}. \
             Remove the extra accounts with an earlier version, then start again.",
            duplicates.join(", ")
        );
    }
    if !projects.is_empty() && path != ":memory:" {
        let backup = format!("{path}.pre-accounts");
        if !std::path::Path::new(&backup).exists() {
            sqlx::query("VACUUM INTO ?").bind(&backup).execute(pool).await.context("backing up before the accounts upgrade")?;
            tracing::info!("upgrading to accounts; the previous database is saved as {backup}");
        }
    }
    let had_mfa = has_column(pool, "projects", "require_mfa").await?;
    let default_plan = crate::plans::default_plan(pool).await?;
    let mut tx = pool.begin().await?;
    for (project_id, name, account_id) in &projects {
        if account_id.is_some() {
            continue;
        }
        let require_mfa: Flag = if had_mfa {
            sqlx::query_scalar("SELECT require_mfa FROM projects WHERE id = $1").bind(project_id).fetch_one(&mut *tx).await?
        } else {
            Flag(false)
        };
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO accounts (name, plan_id, require_mfa, created_at) VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(name)
        .bind(default_plan.id)
        .bind(require_mfa)
        .bind(now_ms())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("UPDATE projects SET account_id = $1, slug = $2 WHERE id = $3")
            .bind(account)
            .bind(slugify(name))
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
    }
    // Former viewers gain write access as users: say so in each account's log.
    sqlx::query(
        "INSERT INTO audit_log (project_id, actor, action, detail, at)
         SELECT project_id, 'upgrade', 'member.role_changed', email || ': viewer → user (the viewer role was removed)', $1
         FROM users WHERE role = 'viewer'",
    )
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;
    let role = "CASE role WHEN 'member' THEN 'user' WHEN 'viewer' THEN 'user' ELSE role END";
    let rebuild = format!(
        "CREATE TABLE users_v2 (
           id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, email TEXT NOT NULL, name TEXT NOT NULL DEFAULT '',
           password_hash TEXT NOT NULL, role TEXT NOT NULL, totp_secret TEXT, totp_pending_secret TEXT,
           totp_enabled_at INTEGER, totp_last_step INTEGER, created_at INTEGER NOT NULL, last_login_at INTEGER,
           is_operator INTEGER NOT NULL DEFAULT 0, UNIQUE (email));
         INSERT INTO users_v2 SELECT u.id, p.account_id, lower(u.email), u.name, u.password_hash, {role},
           u.totp_secret, u.totp_pending_secret, u.totp_enabled_at, u.totp_last_step, u.created_at, u.last_login_at, 0
           FROM users u JOIN projects p ON p.id = u.project_id;
         DROP TABLE users; ALTER TABLE users_v2 RENAME TO users;

         CREATE TABLE invites_v2 (
           id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, token_hash TEXT NOT NULL UNIQUE, email TEXT NOT NULL,
           role TEXT NOT NULL, invited_by INTEGER, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, accepted_at INTEGER);
         INSERT INTO invites_v2 SELECT i.id, p.account_id, i.token_hash, i.email, {role}, i.invited_by, i.created_at, i.expires_at, i.accepted_at
           FROM invites i JOIN projects p ON p.id = i.project_id;
         DROP TABLE invites; ALTER TABLE invites_v2 RENAME TO invites;

         CREATE TABLE api_tokens_v2 (
           id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
           prefix TEXT NOT NULL, created_by INTEGER, read_only INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
           last_used_at INTEGER, revoked_at INTEGER);
         INSERT INTO api_tokens_v2 SELECT t.id, p.account_id, t.name, t.token_hash, t.prefix, t.created_by, t.read_only,
           t.created_at, t.last_used_at, t.revoked_at FROM api_tokens t JOIN projects p ON p.id = t.project_id;
         DROP TABLE api_tokens; ALTER TABLE api_tokens_v2 RENAME TO api_tokens;

         CREATE TABLE audit_log_v2 (
           id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, project_id INTEGER, actor TEXT NOT NULL,
           action TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', at INTEGER NOT NULL);
         INSERT INTO audit_log_v2 SELECT a.id, p.account_id, a.project_id, a.actor, a.action, a.detail, a.at
           FROM audit_log a JOIN projects p ON p.id = a.project_id;
         DROP TABLE audit_log; ALTER TABLE audit_log_v2 RENAME TO audit_log;"
    );
    sqlx::raw_sql(&rebuild).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// A URL-friendly identifier: "Docs site" → "docs-site".
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(40).collect::<String>();
    if out.is_empty() { "site".into() } else { out }
}

// ---- accounts ------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub require_mfa: bool,
    pub created_at: i64,
}

pub async fn create_account(pool: &Pool, name: &str, plan_key: Option<&str>) -> Result<Account> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        bail!("give the account a name of up to 100 characters");
    }
    let plan = match plan_key {
        Some(k) => crate::plans::by_key(pool, k).await?.with_context(|| format!("no plan {k:?}"))?,
        None => crate::plans::default_plan(pool).await?,
    };
    let id: i64 = sqlx::query_scalar("INSERT INTO accounts (name, plan_id, created_at) VALUES ($1, $2, $3) RETURNING id")
        .bind(name)
        .bind(plan.id)
        .bind(now_ms())
        .fetch_one(pool)
        .await?;
    account(pool, id).await
}

/// A new account together with its first website (named after the account).
pub async fn create_account_with_site(pool: &Pool, name: &str, plan_key: Option<&str>) -> Result<(Account, Project)> {
    let account = create_account(pool, name, plan_key).await?;
    let site = create_site(pool, account.id, name).await?;
    Ok((account, site))
}

pub async fn account(pool: &Pool, id: i64) -> Result<Account> {
    let (id, name, require_mfa, created_at): (i64, String, Flag, i64) =
        sqlx::query_as("SELECT id, name, require_mfa, created_at FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .context("no such account")?;
    Ok(Account { id, name, require_mfa: require_mfa.0, created_at })
}

pub async fn accounts(pool: &Pool) -> Result<Vec<Account>> {
    let rows: Vec<(i64, String, Flag, i64)> =
        sqlx::query_as("SELECT id, name, require_mfa, created_at FROM accounts ORDER BY id").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(id, name, require_mfa, created_at)| Account { id, name, require_mfa: require_mfa.0, created_at }).collect())
}

pub async fn rename_account(pool: &Pool, id: i64, name: &str) -> Result<()> {
    sqlx::query("UPDATE accounts SET name = $1 WHERE id = $2").bind(name.trim()).bind(id).execute(pool).await?;
    Ok(())
}

// ---- websites ------------------------------------------------------------------

const SITE_COLS: &str = "id, account_id, name, slug, write_key";
type SiteRow = (i64, Option<i64>, String, Option<String>, String);

fn site_from((id, account_id, name, slug, write_key): SiteRow) -> Project {
    Project { id, account_id: account_id.unwrap_or_default(), slug: slug.unwrap_or_else(|| slugify(&name)), name, write_key }
}

pub async fn project(pool: &Pool, id: i64) -> Result<Project> {
    let row: SiteRow = sqlx::query_as(&format!("SELECT {SITE_COLS} FROM projects WHERE id = $1")).bind(id).fetch_one(pool).await?;
    Ok(site_from(row))
}

/// The website a collector write key belongs to: (website id, account id).
pub async fn project_by_key(pool: &Pool, key: &str) -> Result<Option<(i64, i64)>> {
    let row: Option<(i64, Option<i64>)> = sqlx::query_as("SELECT id, account_id FROM projects WHERE write_key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, account)| (id, account.unwrap_or_default())))
}

pub async fn sites(pool: &Pool, account_id: i64) -> Result<Vec<Project>> {
    let rows: Vec<SiteRow> = sqlx::query_as(&format!("SELECT {SITE_COLS} FROM projects WHERE account_id = $1 ORDER BY id"))
        .bind(account_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(site_from).collect())
}

pub async fn site_by_slug(pool: &Pool, account_id: i64, slug: &str) -> Result<Option<Project>> {
    let row: Option<SiteRow> = sqlx::query_as(&format!("SELECT {SITE_COLS} FROM projects WHERE account_id = $1 AND slug = $2"))
        .bind(account_id)
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(site_from))
}

/// Adds a website, refusing when the account is at its website limit.
pub async fn create_site(pool: &Pool, account_id: i64, name: &str) -> Result<Project> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        bail!("give the website a name of up to 100 characters");
    }
    let existing = sites(pool, account_id).await?;
    let limit = crate::plans::for_account(pool, account_id).await?.effective.max_sites;
    if let Some(max) = limit
        && existing.len() as i64 >= max
    {
        bail!("this account can have {max} website{}; ask for more to add another", if max == 1 { "" } else { "s" });
    }
    let base = slugify(name);
    let mut slug = base.clone();
    let mut n = 2;
    while existing.iter().any(|s| s.slug == slug) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (account_id, name, slug, write_key, created_at) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(account_id)
    .bind(name)
    .bind(&slug)
    .bind(new_key())
    .bind(now_ms())
    .fetch_one(pool)
    .await?;
    project(pool, id).await
}

/// Deletes a website and everything collected or configured for it.
pub async fn delete_site(pool: &Pool, account_id: i64, id: i64) -> Result<bool> {
    let owned: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 AND account_id = $2")
        .bind(id)
        .bind(account_id)
        .fetch_optional(pool)
        .await?;
    if owned.is_none() {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM evaluations WHERE insight_id IN (SELECT id FROM insights WHERE project_id = $1)").bind(id).execute(&mut *tx).await?;
    for table in ["events", "sessions", "event_sources", "insights", "kpis", "event_definitions", "analysis_runs", "privacy_rules"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE project_id = $1")).bind(id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE audit_log SET project_id = NULL WHERE project_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM projects WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

/// For the command line: the website named by `slug`, or the only website
/// when there is exactly one. Errors list the choices.
pub async fn resolve_site(pool: &Pool, slug: Option<&str>) -> Result<Project> {
    let rows: Vec<SiteRow> = sqlx::query_as(&format!("SELECT {SITE_COLS} FROM projects ORDER BY id")).fetch_all(pool).await?;
    let all: Vec<Project> = rows.into_iter().map(site_from).collect();
    let choices = || all.iter().map(|s| s.slug.clone()).collect::<Vec<_>>().join(", ");
    match slug {
        Some(slug) => {
            let found: Vec<&Project> = all.iter().filter(|s| s.slug == slug).collect();
            match found.as_slice() {
                [one] => Ok((*one).clone()),
                [] => bail!("no website {slug:?}; websites: {}", choices()),
                _ => bail!("more than one account has a website {slug:?}; use the API with that account's token"),
            }
        }
        None => match all.as_slice() {
            [one] => Ok(one.clone()),
            [] => bail!("there are no websites yet; create an account in the dashboard or run `trace demo`"),
            _ => bail!("there are several websites; choose one with --site ({})", choices()),
        },
    }
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

pub async fn collection(pool: &Pool, project_id: i64) -> Result<Collection> {
    let last_event_at: Option<i64> = sqlx::query_scalar("SELECT last_event_at FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    let sources = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT origin, first_seen, last_seen FROM event_sources WHERE project_id = $1 ORDER BY last_seen DESC LIMIT 10",
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

/// A new, empty database for a test, as a target for [`connect`]: a
/// temporary SQLite file, or, when `TRACE_TEST_POSTGRES` names a Postgres
/// server (e.g. `postgres://localhost/postgres`), a new database on it.
/// Remove it with [`drop_test_target`].
#[doc(hidden)]
pub async fn test_target(label: &str) -> String {
    let label = label.replace('-', "_");
    match std::env::var("TRACE_TEST_POSTGRES") {
        Ok(server) if !server.is_empty() => {
            let name = format!("trace_test_{label}_{}", new_key());
            sqlx::any::install_default_drivers();
            let admin = Pool::connect(&server).await.expect("connecting to TRACE_TEST_POSTGRES");
            sqlx::raw_sql(&format!("CREATE DATABASE {name}")).execute(&admin).await.expect("creating a test database");
            admin.close().await;
            with_database(&server, &name)
        }
        _ => std::env::temp_dir().join(format!("trace-{label}-{}.db", new_key())).to_string_lossy().into_owned(),
    }
}

#[doc(hidden)]
pub async fn drop_test_target(target: &str) {
    if is_postgres_url(target) {
        let name = database_name(target).to_string();
        let Ok(server) = std::env::var("TRACE_TEST_POSTGRES") else { return };
        if let Ok(admin) = Pool::connect(&server).await {
            let _ = sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).execute(&admin).await;
            admin.close().await;
        }
    } else {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{target}{suffix}"));
        }
    }
}

/// The database name in a Postgres URL: `postgres://host/name?x` → `name`.
fn database_name(url: &str) -> &str {
    let no_query = url.split('?').next().unwrap_or(url);
    no_query.rsplit('/').next().unwrap_or(no_query)
}

/// The same Postgres URL pointing at another database.
fn with_database(url: &str, name: &str) -> String {
    let (base, query) = url.split_once('?').map_or((url, None), |(b, q)| (b, Some(q)));
    let (server, _) = base.rsplit_once('/').filter(|(s, _)| s.contains("://") && !s.ends_with('/')).unwrap_or((base, ""));
    match query {
        Some(q) => format!("{server}/{name}?{q}"),
        None => format!("{server}/{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh() -> (Pool, String) {
        let target = test_target("db").await;
        (connect(&target).await.unwrap(), target)
    }

    #[test]
    fn postgres_urls() {
        assert!(is_postgres_url("postgres://localhost/trace") && is_postgres_url("postgresql://u@h:5432/trace"));
        assert!(!is_postgres_url("trace.db") && !is_postgres_url("/var/lib/trace/trace.db"));
        assert_eq!(with_database("postgres://u@h:5432/postgres?sslmode=disable", "t1"), "postgres://u@h:5432/t1?sslmode=disable");
        assert_eq!(with_database("postgres://localhost", "t1"), "postgres://localhost/t1");
        assert_eq!(database_name("postgres://u@h/t1?sslmode=disable"), "t1");
    }

    #[test]
    fn describing_a_target_hides_the_password() {
        assert_eq!(describe("trace.db"), "SQLite trace.db");
        assert_eq!(describe("postgres://trace:s3cret@db:5432/trace"), "Postgres postgres://trace:***@db:5432/trace");
        assert_eq!(describe("postgresql://trace@db/trace"), "Postgres postgresql://trace@db/trace");
        assert_eq!(
            describe("postgres://db/trace?user=trace&password=s3cret&sslmode=require"),
            "Postgres postgres://db/trace?user=trace&password=***&sslmode=require"
        );
        assert!(!describe("postgres://a:p%40ss@db/t").contains("p%40ss"));
        assert_eq!(describe("postgres://a:p@ss@db/t"), "Postgres postgres://a:***@db/t");
    }

    /// schema.sql and structure.sql must define the same tables and columns.
    #[test]
    fn schemas_match() {
        fn columns(sql: &str) -> Vec<String> {
            let mut out = Vec::new();
            let mut table = String::new();
            for line in sql.lines().map(str::trim) {
                if let Some(rest) = line.strip_prefix("CREATE TABLE IF NOT EXISTS ") {
                    table = rest.trim_end_matches(" (").to_string();
                } else if !table.is_empty() && !line.starts_with("--") && !line.starts_with(')') {
                    let first = line.split_whitespace().next().unwrap_or("");
                    if !first.is_empty() && !["PRIMARY", "UNIQUE", "FOREIGN"].contains(&first) {
                        out.push(format!("{table}.{first}"));
                    }
                }
            }
            out
        }
        let (lite, pg) = (columns(SCHEMA), columns(STRUCTURE));
        assert!(lite.len() > 100, "parsed {} columns", lite.len());
        assert_eq!(lite, pg);
        // Only surrogate `id` columns generate their own values.
        for line in STRUCTURE.lines().filter(|l| l.contains("IDENTITY")) {
            assert!(line.trim_start().starts_with("id "), "identity on a non-id column: {line}");
        }
    }

    /// Several servers creating the schema of one new database at once.
    #[tokio::test]
    async fn several_servers_can_start_at_once() {
        let target = test_target("starts").await;
        let started = tokio::join!(connect(&target), connect(&target), connect(&target), connect(&target));
        for pool in [started.0, started.1, started.2, started.3] {
            pool.unwrap().close().await;
        }
        let pool = connect(&target).await.unwrap();
        assert_eq!(crate::plans::list(&pool).await.unwrap().len(), 4, "plans seeded once");
        pool.close().await;
        drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn sqlite_paths_are_taken_literally() {
        let dir = std::env::temp_dir().join(format!("trace paths {}", new_key()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a?b#c 100%25 ünï.db");
        connect(path.to_str().unwrap()).await.unwrap().close().await;
        assert!(path.exists(), "created {}", path.display());
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "db")).count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_in_memory_database_keeps_its_data() {
        let pool = connect(":memory:").await.unwrap();
        create_account_with_site(&pool, "Memo", None).await.unwrap();
        for _ in 0..3 {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts").fetch_one(&pool).await.unwrap();
            assert_eq!(n, 1, "every query sees the same database");
        }
        assert_eq!(pool.options().get_max_connections(), 1);
        assert!(pool.options().get_idle_timeout().is_none() && pool.options().get_max_lifetime().is_none(), "never closed");
    }

    /// The same statement first with a NULL real, then with a value; see [`Real`].
    #[tokio::test]
    async fn nullable_reals_round_trip() {
        let (pool, target) = fresh().await;
        // One connection, so the statement is prepared once and then reused.
        let mut conn = pool.acquire().await.unwrap();
        for value in [None, Some(42.5_f64), None, Some(0.125)] {
            sqlx::query("INSERT INTO events (project_id, session_id, ts, type, value) VALUES (1, 's', 0, 'scroll', $1)")
                .bind(Real(value))
                .execute(&mut *conn)
                .await
                .unwrap();
        }
        drop(conn);
        let values: Vec<Option<f64>> = sqlx::query_scalar("SELECT value FROM events ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(values, [None, Some(42.5), None, Some(0.125)]);
        drop_test_target(&target).await;
    }

    #[tokio::test]
    async fn fresh_server_defaults_to_unlimited() {
        let (pool, path) = fresh().await;
        let (account, site) = create_account_with_site(&pool, "Solo", None).await.unwrap();
        let plan = crate::plans::for_account(&pool, account.id).await.unwrap();
        assert_eq!(plan.plan.key, "unlimited");
        assert_eq!(plan.effective, crate::plans::Limits::default(), "no limits at all");
        assert_eq!((site.slug.as_str(), site.account_id), ("solo", account.id));
        let keys: Vec<String> = crate::plans::list(&pool).await.unwrap().into_iter().map(|p| p.key).collect();
        assert_eq!(keys, ["unlimited", "free", "friend", "owner"]);
        crate::db::drop_test_target(&path).await;
    }

    #[tokio::test]
    async fn website_limit_and_unique_slugs() {
        let (pool, path) = fresh().await;
        let (free, _) = create_account_with_site(&pool, "Free", Some("free")).await.unwrap();
        let refused = create_site(&pool, free.id, "Second").await.unwrap_err();
        assert!(refused.to_string().contains("1 website"), "{refused}");
        let (open, _) = create_account_with_site(&pool, "Shop", None).await.unwrap();
        assert_eq!(create_site(&pool, open.id, "Shop").await.unwrap().slug, "shop-2");
        crate::db::drop_test_target(&path).await;
    }

    /// A database written by the version before accounts: one workspace per
    /// project, people and tokens keyed by project, member and viewer roles.
    const PRE_ACCOUNTS: &str = "
        CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL, write_key TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL,
          config TEXT, config_synced_at INTEGER, config_source TEXT, last_event_at INTEGER, require_mfa INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE users (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL, email TEXT NOT NULL, name TEXT NOT NULL DEFAULT '',
          password_hash TEXT NOT NULL, role TEXT NOT NULL, totp_secret TEXT, totp_pending_secret TEXT, totp_enabled_at INTEGER,
          totp_last_step INTEGER, created_at INTEGER NOT NULL, last_login_at INTEGER, UNIQUE (project_id, email));
        CREATE TABLE invites (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL, token_hash TEXT NOT NULL UNIQUE, email TEXT NOT NULL,
          role TEXT NOT NULL, invited_by INTEGER, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, accepted_at INTEGER);
        CREATE TABLE api_tokens (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
          prefix TEXT NOT NULL, created_by INTEGER, read_only INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL,
          last_used_at INTEGER, revoked_at INTEGER);
        CREATE TABLE audit_log (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL,
          detail TEXT NOT NULL DEFAULT '', at INTEGER NOT NULL);
        CREATE TABLE events (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL, session_id TEXT NOT NULL, ts INTEGER NOT NULL,
          type TEXT NOT NULL, path TEXT, selector TEXT, text TEXT, name TEXT, value REAL, props TEXT);
        INSERT INTO projects VALUES (1, 'Stride', 'key-stride', 1, NULL, NULL, NULL, NULL, 1), (2, 'Docs', 'key-docs', 2, NULL, NULL, NULL, NULL, 0);
        INSERT INTO users (id, project_id, email, password_hash, role, created_at) VALUES
          (1, 1, 'owner@stride.example', 'h', 'owner', 1), (2, 1, 'mia@stride.example', 'h', 'member', 1),
          (3, 1, 'vic@stride.example', 'h', 'viewer', 1), (4, 2, 'dora@docs.example', 'h', 'owner', 1);
        INSERT INTO invites VALUES (1, 1, 'inv', 'new@stride.example', 'viewer', 1, 1, 9999999999999, NULL);
        INSERT INTO api_tokens VALUES (1, 1, 'CI', 'tok', 'trc_abc', 2, 0, 1, NULL, NULL);
        INSERT INTO audit_log VALUES (1, 1, 'owner@stride.example', 'member.invited', '', 1);
        INSERT INTO events VALUES (1, 1, 's1', 1, 'pageview', '/', NULL, NULL, NULL, NULL, NULL);";

    #[tokio::test]
    async fn ideas_become_insights() {
        let path = std::env::temp_dir().join(format!("trace-rename-{}.db", new_key()));
        {
            sqlx::any::install_default_drivers();
            let old = Pool::connect(&format!("sqlite://{}?mode=rwc", path.display())).await.unwrap();
            sqlx::raw_sql(
                "CREATE TABLE ideas (id INTEGER PRIMARY KEY, project_id INTEGER, title TEXT, status TEXT NOT NULL DEFAULT 'new');
                 CREATE INDEX ideas_project_status ON ideas(project_id);
                 CREATE TABLE evaluations (idea_id INTEGER PRIMARY KEY REFERENCES ideas(id) ON DELETE CASCADE, verdict TEXT);
                 CREATE TABLE analysis_runs (id INTEGER PRIMARY KEY, ideas_created INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO ideas (id, project_id, title) VALUES (7, 1, 'Fix checkout');
                 INSERT INTO evaluations VALUES (7, 'improved');
                 INSERT INTO analysis_runs VALUES (1, 3);",
            )
            .execute(&old)
            .await
            .unwrap();
            old.close().await;
        }
        let pool = connect(path.to_str().unwrap()).await.unwrap();
        let title: String = sqlx::query_scalar("SELECT title FROM insights WHERE id = 7").fetch_one(&pool).await.unwrap();
        assert_eq!(title, "Fix checkout");
        let verdict: String = sqlx::query_scalar("SELECT verdict FROM evaluations WHERE insight_id = 7").fetch_one(&pool).await.unwrap();
        assert_eq!(verdict, "improved");
        let created: i64 = sqlx::query_scalar("SELECT insights_created FROM analysis_runs").fetch_one(&pool).await.unwrap();
        assert_eq!(created, 3);
        assert!(!has_table(&pool, "ideas").await.unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn upgrades_a_pre_accounts_database() {
        let path = std::env::temp_dir().join(format!("trace-upgrade-{}.db", new_key()));
        let p = path.to_str().unwrap().to_string();
        {
            sqlx::any::install_default_drivers();
            let old = Pool::connect(&format!("sqlite://{p}?mode=rwc")).await.unwrap();
            sqlx::raw_sql(PRE_ACCOUNTS).execute(&old).await.unwrap();
            old.close().await;
        }
        let pool = connect(&p).await.unwrap();
        assert!(std::path::Path::new(&format!("{p}.pre-accounts")).exists(), "a copy is kept for rollback");

        let stride = sites(&pool, project(&pool, 1).await.unwrap().account_id).await.unwrap();
        let docs = sites(&pool, project(&pool, 2).await.unwrap().account_id).await.unwrap();
        assert_ne!(stride[0].account_id, docs[0].account_id, "each workspace becomes its own account");
        assert_eq!((stride[0].slug.as_str(), stride[0].write_key.as_str()), ("stride", "key-stride"), "write keys keep working");
        let (a, d) = (stride[0].account_id, docs[0].account_id);

        let people = crate::auth::list_users(&pool, a).await.unwrap();
        let roles: Vec<(&str, &str)> = people.iter().map(|u| (u.email.as_str(), u.role.as_str())).collect();
        assert_eq!(roles, [("owner@stride.example", "owner"), ("mia@stride.example", "user"), ("vic@stride.example", "user")]);
        assert_eq!(crate::auth::list_users(&pool, d).await.unwrap().len(), 1, "nobody crosses accounts");
        assert!(crate::auth::require_mfa(&pool, a).await.unwrap() && !crate::auth::require_mfa(&pool, d).await.unwrap());
        assert_eq!(crate::auth::pending_invites(&pool, a).await.unwrap()[0].role, crate::auth::Role::User);
        assert_eq!(crate::auth::list_api_tokens(&pool, a, None).await.unwrap().len(), 1);
        let log = crate::auth::audit_log(&pool, a, 10).await.unwrap();
        assert!(log.iter().any(|e| e.detail.contains("vic@stride.example: viewer → user")), "former viewers are recorded");
        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE project_id = 1").fetch_one(&pool).await.unwrap();
        assert_eq!(events, 1, "collected data stays");

        pool.close().await;
        let again = connect(&p).await.unwrap(); // upgrading twice is a no-op
        assert_eq!(accounts(&again).await.unwrap().len(), 2);
        for f in [p.clone(), format!("{p}.pre-accounts")] {
            let _ = std::fs::remove_file(f);
        }
    }

    #[tokio::test]
    async fn upgrade_stops_on_shared_emails() {
        let path = std::env::temp_dir().join(format!("trace-dupes-{}.db", new_key()));
        let p = path.to_str().unwrap().to_string();
        {
            sqlx::any::install_default_drivers();
            let old = Pool::connect(&format!("sqlite://{p}?mode=rwc")).await.unwrap();
            sqlx::raw_sql(PRE_ACCOUNTS).execute(&old).await.unwrap();
            sqlx::raw_sql("INSERT INTO users (project_id, email, password_hash, role, created_at) VALUES (2, 'mia@stride.example', 'h', 'owner', 1)")
                .execute(&old)
                .await
                .unwrap();
            old.close().await;
        }
        let err = connect(&p).await.unwrap_err().to_string();
        assert!(err.contains("mia@stride.example"), "{err}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Docs site"), "docs-site");
        assert_eq!(slugify("  Stride — Shop!  "), "stride-shop");
        assert_eq!(slugify("???"), "site");
    }
}
