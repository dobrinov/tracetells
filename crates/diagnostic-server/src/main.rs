mod api;
mod auth_api;
mod mcp;
mod demo;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use diagnostic_agent::{ClaudeConfig, Engine};
use diagnostic_core::model::IdeaStatus;
use diagnostic_core::util::DAY_MS;
use diagnostic_core::{config, db, evaluate, ideas, kpi, prompt, retention};

#[derive(Parser)]
#[command(name = "diagnostic", version, about = "Behaviour analytics for agents: collect, measure, suggest, evaluate.")]
struct Cli {
    /// SQLite database file.
    #[arg(long, global = true, env = "DIAGNOSTIC_DB", default_value = "diagnostic.db")]
    db: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the collector endpoint, API and dashboard.
    Serve {
        #[arg(long, env = "DIAGNOSTIC_ADDR", default_value = "127.0.0.1:4318")]
        addr: SocketAddr,
        /// Require this bearer token for the dashboard API. Set it whenever the server is reachable by others.
        #[arg(long, env = "DIAGNOSTIC_ADMIN_TOKEN")]
        admin_token: Option<String>,
        /// How often to re-evaluate released ideas, in minutes (0 turns it off).
        #[arg(long, default_value_t = 60)]
        evaluate_every: u64,
        /// Delete raw events older than this many days (0 keeps them forever). Ideas, KPIs and evaluations are kept.
        #[arg(long, env = "DIAGNOSTIC_RETENTION_DAYS", default_value_t = 90)]
        retention_days: u32,
        /// URL browsers use to reach this server (for the collector script). Defaults to http://<addr>.
        #[arg(long, env = "DIAGNOSTIC_PUBLIC_URL")]
        public_url: Option<String>,
        /// Also serve the instrumented Stride demo shop on this address, e.g. 127.0.0.1:4319.
        #[arg(long)]
        demo_site: Option<SocketAddr>,
        /// Keep KPIs and the event catalogue in sync with this diagnostic.json (re-applied when it changes).
        #[arg(long, env = "DIAGNOSTIC_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Validate a diagnostic.json without applying it. Exits non-zero on errors, for CI.
    Check {
        #[arg(default_value = config::DEFAULT_FILE)]
        file: PathBuf,
    },
    /// Apply a diagnostic.json: to a running server with --server, otherwise to the local database.
    Push {
        #[arg(default_value = config::DEFAULT_FILE)]
        file: PathBuf,
        /// Server URL, e.g. https://diagnostic.example.com
        #[arg(long, env = "DIAGNOSTIC_SERVER")]
        server: Option<String>,
        /// An API token created under Team in the dashboard.
        #[arg(long, env = "DIAGNOSTIC_TOKEN")]
        token: Option<String>,
    },
    /// Analyse recent behaviour and create improvement ideas.
    Analyze {
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
        /// Days of behaviour to analyse.
        #[arg(long, default_value_t = 14)]
        days: u32,
        #[arg(long, default_value_t = 5)]
        max_ideas: usize,
    },
    /// Measure released ideas against their KPIs.
    Evaluate,
    /// List ideas.
    Ideas {
        /// new, selected, in_progress, released or dismissed.
        #[arg(long)]
        status: Option<IdeaStatus>,
    },
    /// Print an idea as a prompt for a coding agent, e.g. `diagnostic prompt 3 | claude`.
    Prompt { id: i64 },
    /// Move an idea to another status.
    Status {
        id: i64,
        status: IdeaStatus,
        /// Why the idea was dismissed; the analyst learns from it.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run an MCP server on stdin/stdout for a local coding agent (Claude Code, Cursor, ...).
    /// Remote agents use the server's /mcp endpoint with an API token instead.
    Mcp,
    /// Manage dashboard accounts from the server, e.g. when the owner is locked out.
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Fill the database with 30 days of simulated traffic for the Stride demo shop.
    /// Then `diagnostic serve --demo-site 127.0.0.1:4319` serves the shop itself.
    Demo {
        #[arg(long, default_value_t = 4000)]
        sessions: usize,
    },
}

#[derive(Subcommand)]
enum UserAction {
    /// Create an account. The password comes from DIAGNOSTIC_PASSWORD, or a random one is printed.
    Create {
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "")]
        name: String,
        /// owner, admin, member or viewer. The first account is always the owner.
        #[arg(long, default_value = "member")]
        role: String,
    },
    /// List accounts.
    List,
    /// Set a new random password (printed once) and sign the user out everywhere.
    ResetPassword {
        #[arg(long)]
        email: String,
    },
    /// Turn off two-step verification for a user who lost their device; they set it up again at next sign-in.
    ResetMfa {
        #[arg(long)]
        email: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum EngineArg {
    Auto,
    Claude,
    Heuristics,
}

impl From<EngineArg> for Engine {
    fn from(e: EngineArg) -> Self {
        match e {
            EngineArg::Auto => Engine::Auto,
            EngineArg::Claude => Engine::Claude,
            EngineArg::Heuristics => Engine::Heuristics,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let pool = db::connect(&cli.db).await.with_context(|| format!("opening {}", cli.db))?;
    let project = db::default_project(&pool).await?;
    let claude = ClaudeConfig::from_env();

    match cli.command {
        Command::Serve { addr, admin_token, evaluate_every, retention_days, public_url, demo_site, config: config_file } => {
            if let Some(path) = config_file {
                // Fail fast on a bad file at startup; later edits are applied as they're saved.
                apply_file(&pool, project.id, &path).await?;
                let pool = pool.clone();
                tokio::spawn(async move {
                    let mut last = modified(&path);
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        let now = modified(&path);
                        if now != last {
                            last = now;
                            if let Err(e) = apply_file(&pool, project.id, &path).await {
                                tracing::error!("{}: {e:#}", path.display());
                            }
                        }
                    }
                });
            }
            if admin_token.is_none() && !addr.ip().is_loopback() {
                tracing::warn!("listening on {addr} without DIAGNOSTIC_ADMIN_TOKEN: anyone who can reach it can use the dashboard API");
            }
            if evaluate_every > 0 || retention_days > 0 {
                let pool = pool.clone();
                let every = if evaluate_every > 0 { evaluate_every } else { 60 };
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(Duration::from_secs(every * 60));
                    loop {
                        tick.tick().await;
                        if retention_days > 0 {
                            match retention::purge(&pool, project.id, retention_days).await {
                                Ok(n) if n > 0 => tracing::info!("deleted {n} events older than {retention_days} days"),
                                Ok(_) => {}
                                Err(e) => tracing::error!(error = %format!("{e:#}"), "retention purge failed"),
                            }
                        }
                        if evaluate_every > 0
                            && let Err(e) = evaluate::run(&pool, project.id, evaluate::Options::default()).await
                        {
                            tracing::error!(error = %format!("{e:#}"), "scheduled evaluation failed");
                        }
                    }
                });
            }
            if let Some(site_addr) = demo_site {
                let public_url = public_url.clone().unwrap_or_else(|| format!("http://{addr}"));
                let site = demo::site(&public_url, &project.write_key);
                let listener = tokio::net::TcpListener::bind(site_addr).await?;
                tracing::info!("Stride demo shop on http://{site_addr}");
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, site).await {
                        tracing::error!(error = %e, "demo site stopped");
                    }
                });
            }
            let public_url = public_url.unwrap_or_else(|| format!("http://{addr}"));
            let setup_code = if diagnostic_core::auth::user_count(&pool, project.id).await? == 0 {
                let code = diagnostic_core::auth::random_token(12);
                tracing::warn!("No accounts yet. Create the owner account here (this link works once): {public_url}/#/welcome/{code}");
                Some(code)
            } else {
                None
            };
            let rules = diagnostic_core::privacy::HostRules::load(&pool, project.id).await?;
            let state = Arc::new(api::AppState {
                pool,
                project_id: project.id,
                admin_token,
                claude,
                setup_code: std::sync::Mutex::new(setup_code),
                limiter: Default::default(),
                secure_cookies: public_url.starts_with("https://"),
                rules: std::sync::RwLock::new(std::sync::Arc::new(rules)),
                collector: Default::default(),
            });
            let listener = tokio::net::TcpListener::bind(addr).await?;
            tracing::info!("Diagnostic listening on http://{addr}");
            axum::serve(listener, api::router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::Analyze { engine, days, max_ideas } => {
            let opts = diagnostic_agent::Options { engine: engine.into(), days, max_ideas };
            let report = diagnostic_agent::analyze(&pool, project.id, claude.as_ref(), &opts).await?;
            if let Some(w) = &report.warning {
                eprintln!("warning: {w}");
            }
            println!(
                "{} new idea(s) from {}{} ({} already existed)",
                report.created.len(),
                report.engine,
                report.model.map(|m| format!(" ({m})")).unwrap_or_default(),
                report.duplicates
            );
            for id in report.created {
                if let Some(i) = ideas::get(&pool, project.id, id).await? {
                    println!("  #{:<4} {}", i.id, i.title);
                }
            }
        }
        Command::Evaluate => {
            let evals = evaluate::run(&pool, project.id, evaluate::Options::default()).await?;
            if evals.is_empty() {
                println!("No released ideas linked to a KPI.");
            }
            for e in evals {
                println!("#{:<4} {:<17} {}", e.idea_id, e.verdict.as_str(), e.summary);
            }
        }
        Command::Ideas { status } => {
            for i in ideas::list(&pool, project.id, status).await? {
                println!("#{:<4} {:<11} {:<10} {}", i.id, i.status.as_str(), i.source, i.title);
            }
        }
        Command::Prompt { id } => {
            let idea = ideas::get(&pool, project.id, id).await?.with_context(|| format!("no idea #{id}"))?;
            print!("{}", render_prompt(&pool, &project, &idea).await?);
        }
        Command::Status { id, status, reason } => {
            let idea = ideas::set_status(&pool, project.id, id, status, reason.as_deref())
                .await?
                .with_context(|| format!("no idea #{id}"))?;
            println!("#{} is now {}", idea.id, idea.status);
        }
        Command::Check { file } => {
            let parsed = read_config(&file)?;
            let c = &parsed.config;
            println!(
                "{} is valid: {} KPI(s), {} declared event(s){}",
                file.display(),
                c.kpis.len(),
                c.events.len(),
                c.product.as_deref().map(|p| format!(", product \"{p}\"")).unwrap_or_default()
            );
            for k in &c.kpis {
                println!("  {:<24} {} ({})", k.key, k.name, k.definition.describe());
            }
            for w in &parsed.warnings {
                println!("warning: {w}");
            }
        }
        Command::Push { file, server, token } => match server {
            Some(server) => push_remote(&file, &server, token.as_deref()).await?,
            None => {
                let report = apply_file(&pool, project.id, &file).await?;
                print_report(&report);
            }
        },
        Command::Mcp => {
            let ctx = mcp::Context {
                pool: &pool,
                project_id: project.id,
                claude: claude.as_ref(),
                actor: "mcp (local)".into(),
                // Whoever can run this can already read the database file.
                role: diagnostic_core::auth::Role::Admin,
            };
            mcp::serve_stdio(ctx).await?;
        }
        Command::User { action } => user_command(&pool, project.id, action).await?,
        Command::Demo { sessions } => demo::seed(&pool, project.id, sessions).await?,
    }
    Ok(())
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_config(path: &Path) -> Result<config::Parsed> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    config::parse(&text).map_err(|errors| {
        anyhow::anyhow!("{} is invalid:\n  {}", path.display(), errors.join("\n  "))
    })
}

/// Describes where a config came from: the file and, inside a git checkout, the commit.
fn source_of(path: &Path) -> String {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match commit {
        Some(c) => format!("{name} @ {c}"),
        None => name,
    }
}

async fn apply_file(pool: &diagnostic_core::SqlitePool, project_id: i64, path: &Path) -> Result<config::SyncReport> {
    let parsed = read_config(path)?;
    let report = config::apply(pool, project_id, &parsed, &source_of(path)).await?;
    tracing::info!(
        "applied {}: {} created, {} updated, {} archived",
        path.display(),
        report.created.len(),
        report.updated.len(),
        report.archived.len()
    );
    Ok(report)
}

async fn push_remote(path: &Path, server: &str, token: Option<&str>) -> Result<()> {
    read_config(path)?; // validate locally first, with the same messages as `check`
    let body = std::fs::read_to_string(path)?;
    let mut req = reqwest::Client::new()
        .post(format!("{}/api/config", server.trim_end_matches('/')))
        .header("content-type", "application/json")
        .header("x-diagnostic-source", source_of(path))
        .body(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await.with_context(|| format!("connecting to {server}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or(text);
        anyhow::bail!("{server} rejected the config ({status}):\n{message}");
    }
    let report: config::SyncReport = serde_json::from_str(&text).context("unexpected response from server")?;
    print_report(&report);
    Ok(())
}

fn print_report(r: &config::SyncReport) {
    let list = |v: &[String]| if v.is_empty() { "-".to_string() } else { v.join(", ") };
    println!("KPIs created:   {}", list(&r.created));
    println!("KPIs updated:   {}", list(&r.updated));
    println!("KPIs archived:  {}", list(&r.archived));
    println!("KPIs unchanged: {}", r.unchanged.len());
    println!("Events declared: {}", r.events);
    for w in &r.warnings {
        println!("warning: {w}");
    }
}

pub async fn render_prompt(
    pool: &diagnostic_core::SqlitePool,
    project: &diagnostic_core::model::Project,
    idea: &diagnostic_core::model::Idea,
) -> Result<String> {
    let kpi = match idea.kpi_id {
        Some(id) => kpi::get(pool, project.id, id).await?,
        None => None,
    };
    let kpi_value = match &kpi {
        Some(k) => {
            let now = diagnostic_core::util::now_ms();
            kpi::measure(pool, project.id, &k.definition, now - 14 * DAY_MS, now).await?.value
        }
        None => None,
    };
    Ok(prompt::render(idea, &prompt::PromptContext { product: &project.name, kpi: kpi.as_ref(), kpi_value }))
}

async fn user_command(pool: &diagnostic_core::SqlitePool, project_id: i64, action: UserAction) -> Result<()> {
    use diagnostic_core::auth::{self, Role};
    let find = |email: String| async move {
        auth::find_user(pool, project_id, &email).await?.with_context(|| format!("no account for {email}"))
    };
    match action {
        UserAction::Create { email, name, role } => {
            let first = auth::user_count(pool, project_id).await? == 0;
            let role: Role = if first { Role::Owner } else { role.parse()? };
            let (password, generated) = match std::env::var("DIAGNOSTIC_PASSWORD") {
                Ok(p) if !p.is_empty() => (p, false),
                _ => (auth::random_token(15), true),
            };
            let user = auth::create_user(pool, project_id, &email, &name, &password, role).await?;
            auth::audit(pool, project_id, "cli", "member.created", &format!("{} as {}", user.email, user.role)).await?;
            println!("Created {} ({}).", user.email, user.role);
            if generated {
                println!("Password: {password}\n(shown once; change it under Account after signing in)");
            }
        }
        UserAction::List => {
            for u in auth::list_users(pool, project_id).await? {
                println!("{:<32} {:<7} 2FA {:<3} {}", u.email, u.role.as_str(), if u.totp_enabled { "on" } else { "off" }, u.name);
            }
        }
        UserAction::ResetPassword { email } => {
            let user = find(email).await?;
            let password = auth::random_token(15);
            auth::set_password(pool, user.id, &password).await?;
            auth::end_user_sessions(pool, user.id, None).await?;
            auth::audit(pool, project_id, "cli", "account.password_reset", &user.email).await?;
            println!("New password for {}: {password}\n(shown once; signed out everywhere)", user.email);
        }
        UserAction::ResetMfa { email } => {
            let user = find(email).await?;
            auth::disable_totp(pool, user.id).await?;
            auth::end_user_sessions(pool, user.id, None).await?;
            auth::audit(pool, project_id, "cli", "mfa.reset", &user.email).await?;
            println!("Two-step verification is off for {}; they'll set it up again at next sign-in if it's required.", user.email);
        }
    }
    Ok(())
}
