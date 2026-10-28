//! Installation: `trace init` chooses where Trace keeps its data and saves the
//! choice in `trace.env`, which every `trace` command reads on start.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use trace_core::db;

/// The settings file `trace init` writes. `TRACE_ENV_FILE` points elsewhere.
pub fn env_file() -> String {
    std::env::var("TRACE_ENV_FILE").unwrap_or_else(|_| "trace.env".into())
}

/// `KEY=VALUE` lines from a settings file. Blank lines and `#` comments are
/// skipped, and one pair of surrounding quotes is removed from a value.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (key, value) = l.strip_prefix("export ").unwrap_or(l).split_once('=')?;
            let value = value.trim();
            let value = [('"', '"'), ('\'', '\'')]
                .iter()
                .find_map(|(a, b)| value.strip_prefix(*a).and_then(|v| v.strip_suffix(*b)))
                .unwrap_or(value);
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

static FROM_FILE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Loads the settings file into the environment, without overriding
/// variables that are already set. Runs before the async runtime starts,
/// while the process has one thread.
pub fn load_env_file() {
    let mut loaded = Vec::new();
    if let Ok(text) = std::fs::read_to_string(env_file()) {
        for (key, value) in parse_env(&text) {
            if std::env::var_os(&key).is_none() {
                // SAFETY: called at the top of `main`, before any other thread exists.
                unsafe { std::env::set_var(&key, value) };
                loaded.push(key);
            }
        }
    }
    let _ = FROM_FILE.set(loaded);
}

/// True if `key` was set by the settings file rather than the environment.
pub fn from_env_file(key: &str) -> bool {
    FROM_FILE.get().is_some_and(|keys| keys.iter().any(|k| k == key))
}

/// The settings file with `key` set to `value`, keeping every other line.
pub fn with_setting(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key}={value}");
    let mut replaced = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let bare = l.trim().strip_prefix("export ").unwrap_or(l.trim());
            if bare.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
                replaced = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !replaced {
        if out.is_empty() {
            out.push("# Settings for the trace command, written by `trace init`. Real environment variables win.".into());
        }
        out.push(line);
    }
    out.join("\n") + "\n"
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
pub enum DatabaseKind {
    /// One file next to the server: for trying Trace, development and small sites.
    Sqlite,
    /// A Postgres server: recommended for production.
    Postgres,
}

const CHOICES: &str = "Where should Trace keep its data?

  1) SQLite: one file next to the server, nothing to install.
     Right for trying Trace, development and a few small websites.

  2) Postgres (recommended for production): for a server other people rely on.
     Handles many busy websites, with Postgres's own backups and replication.
     Create an empty database first, e.g.
       createuser trace --pwprompt && createdb trace --owner trace
";

fn ask(prompt: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(d) => eprint!("{prompt} [{d}]: "),
        None => eprint!("{prompt}: "),
    }
    std::io::stderr().flush()?;
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        bail!("no answer (stdin closed); run `trace init --database sqlite|postgres` instead");
    }
    let line = line.trim();
    Ok(if line.is_empty() { default.unwrap_or("").to_string() } else { line.to_string() })
}

/// `trace init`: choose the database, check it works, create the tables and
/// save `TRACE_DB` to the settings file.
pub async fn init(kind: Option<DatabaseKind>, target: Option<String>, force: bool) -> Result<()> {
    let interactive = std::io::stdin().is_terminal() && (kind.is_none() || target.is_none());
    let kind = match (kind, &target) {
        (Some(k), _) => k,
        (None, Some(t)) if db::is_postgres_url(t) => DatabaseKind::Postgres,
        (None, Some(_)) => DatabaseKind::Sqlite,
        (None, None) if interactive => {
            eprintln!("{CHOICES}");
            loop {
                match ask("Choose 1 or 2", Some("1"))?.as_str() {
                    "1" | "sqlite" => break DatabaseKind::Sqlite,
                    "2" | "postgres" => break DatabaseKind::Postgres,
                    _ => eprintln!("Type 1 for SQLite or 2 for Postgres."),
                }
            }
        }
        (None, None) => bail!("choose a database: `trace init --database sqlite` or `--database postgres --db postgres://...`"),
    };
    let target = match (kind, target) {
        (DatabaseKind::Sqlite, Some(t)) => t,
        (DatabaseKind::Sqlite, None) if interactive => ask("Database file", Some("trace.db"))?,
        (DatabaseKind::Sqlite, None) => "trace.db".into(),
        (DatabaseKind::Postgres, Some(t)) => t,
        (DatabaseKind::Postgres, None) if interactive => ask("Connection URL, e.g. postgres://trace:PASSWORD@localhost/trace", None)?,
        (DatabaseKind::Postgres, None) => bail!("give the Postgres connection URL with --db postgres://..."),
    };
    if kind == DatabaseKind::Postgres && !db::is_postgres_url(&target) {
        bail!("a Postgres connection URL starts with postgres:// or postgresql://");
    }
    if kind == DatabaseKind::Sqlite && db::is_postgres_url(&target) {
        bail!("that's a Postgres URL; use --database postgres");
    }

    // Settle the settings file before touching any database.
    let file = env_file();
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let current = parse_env(&existing).into_iter().find(|(k, _)| k == "TRACE_DB").map(|(_, v)| v);
    if let Some(current) = current.filter(|c| c != &target) {
        let question = format!("{file} already uses {}. Replace it?", db::describe(&current));
        let replace = force || (interactive && matches!(ask(&format!("{question} (y/n)"), Some("n"))?.as_str(), "y" | "yes"));
        if !replace {
            bail!("{question} Run `trace init` again with --force to replace it.");
        }
    }

    let shown = db::describe(&target);
    eprintln!("\nConnecting to {shown}…");
    let pool = db::connect(&target).await.with_context(|| match kind {
        DatabaseKind::Postgres => format!(
            "couldn't open {shown}. Check that the server is running, the database exists and the user owns it \
             (createdb trace --owner trace)"
        ),
        DatabaseKind::Sqlite => format!("couldn't open {shown}"),
    })?;
    if kind == DatabaseKind::Postgres {
        let version: String = sqlx::query_scalar("SELECT current_setting('server_version')").fetch_one(&pool).await?;
        eprintln!("Connected to Postgres {version}; Trace's tables are ready.");
    } else {
        eprintln!("Trace's tables are ready in {target}.");
    }
    let accounts = db::accounts(&pool).await?.len();
    pool.close().await;

    write_private(Path::new(&file), &with_setting(&existing, "TRACE_DB", &target))?;

    let has_password = shown.contains("***");
    eprintln!("\nSaved the database to {file}{}.", if has_password { " (readable only by you: it holds the password)" } else { "" });
    eprintln!("Next: `trace serve` in this folder reads it.");
    if accounts == 0 {
        eprintln!("      It prints a one-time link for creating the first account.");
    }
    if kind == DatabaseKind::Sqlite {
        eprintln!("For a production server, run `trace init --database postgres` when you're ready. Data isn't copied between the two.");
    }
    Ok(())
}

/// Writes the file so only its owner can read it (it can hold a password).
fn write_private(path: &Path, text: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(text.as_bytes())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, text)?;
    Ok(())
}

/// A warning for `serve` when SQLite backs a server others can reach.
pub fn sqlite_in_production(target: &str, addr: &std::net::SocketAddr, public_url: Option<&str>) -> Option<String> {
    if db::is_postgres_url(target) {
        return None;
    }
    let public = !addr.ip().is_loopback() || public_url.is_some_and(|u| !u.contains("://localhost") && !u.contains("://127.0.0.1"));
    public.then(|| {
        "this server is reachable from outside but runs on SQLite. That's fine for trying Trace and small sites; \
         for production, use Postgres (`trace init --database postgres`)."
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_files_parse_and_update_in_place() {
        let text = "# a comment\nTRACE_ADDR=0.0.0.0:4318\nexport TRACE_DB=\"trace.db\"\n\nANTHROPIC_API_KEY='sk'\n";
        let parsed = parse_env(text);
        assert_eq!(parsed[1], ("TRACE_DB".into(), "trace.db".into()));
        assert_eq!(parsed[2], ("ANTHROPIC_API_KEY".into(), "sk".into()));
        let updated = with_setting(text, "TRACE_DB", "postgres://trace:pw@localhost/trace");
        assert!(updated.contains("TRACE_ADDR=0.0.0.0:4318") && updated.contains("ANTHROPIC_API_KEY='sk'"), "other lines stay");
        assert_eq!(parse_env(&updated).iter().filter(|(k, _)| k == "TRACE_DB").count(), 1);
        let fresh = with_setting("", "TRACE_DB", "trace.db");
        assert!(fresh.starts_with('#') && fresh.ends_with("TRACE_DB=trace.db\n"));
    }

    #[test]
    fn warns_about_sqlite_only_when_reachable() {
        let local: std::net::SocketAddr = "127.0.0.1:4318".parse().unwrap();
        let open: std::net::SocketAddr = "0.0.0.0:4318".parse().unwrap();
        assert!(sqlite_in_production("trace.db", &local, None).is_none());
        assert!(sqlite_in_production("trace.db", &local, Some("https://app.tracetells.com")).is_some(), "behind a proxy");
        assert!(sqlite_in_production("trace.db", &open, None).is_some());
        assert!(sqlite_in_production("postgres://localhost/trace", &open, None).is_none());
    }
}
