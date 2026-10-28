mod analysis;
mod api;
mod auth_api;
mod demo;
mod mcp;
mod operator;
mod setup;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use trace_agent::{ClaudeConfig, Engine};
use trace_core::model::{InsightStatus, Project};
use trace_core::util::DAY_MS;
use trace_core::{Pool, config, db, evaluate, insights, kpi, prompt, retention};

#[derive(Parser)]
#[command(name = "trace", version, about = "Behaviour analytics for agents: collect, measure, suggest, evaluate.")]
struct Cli {
    /// Database: a SQLite file (the default) or a Postgres URL, e.g. postgres://trace@localhost/trace.
    #[arg(long, global = true, env = "TRACE_DB", default_value = "trace.db")]
    db: String,
    /// The website a command is about (its identifier). Can be left out when there is only one.
    #[arg(long, global = true, env = "TRACE_SITE")]
    site: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Set up this server: choose SQLite (for trying Trace and development) or
    /// Postgres (recommended for production), create the tables and save the
    /// choice to trace.env, which every command reads.
    Init {
        /// sqlite or postgres. Asked for when left out.
        #[arg(long, value_enum)]
        database: Option<setup::DatabaseKind>,
        /// Replace a different database already saved in trace.env.
        #[arg(long)]
        force: bool,
    },
    /// Run the collector endpoint, API and dashboard.
    Serve {
        #[arg(long, env = "TRACE_ADDR", default_value = "127.0.0.1:4318")]
        addr: SocketAddr,
        /// Deprecated shared API token with admin rights in the first account. Prefer personal tokens.
        #[arg(long, env = "TRACE_ADMIN_TOKEN")]
        admin_token: Option<String>,
        /// How often to re-evaluate released insights, in minutes (0 turns it off).
        #[arg(long, default_value_t = 60)]
        evaluate_every: u64,
        /// Delete raw events older than this many days on every account (0 = only plan limits apply).
        #[arg(long, env = "TRACE_RETENTION_DAYS", default_value_t = 90)]
        retention_days: u32,
        /// URL browsers use to reach this server (for the collector script). Defaults to http://<addr>.
        #[arg(long, env = "TRACE_PUBLIC_URL")]
        public_url: Option<String>,
        /// Let anyone create an account (on the default plan) from the sign-in page.
        #[arg(long, env = "TRACE_ALLOW_SIGNUP")]
        allow_signup: bool,
        /// Where people ask for higher limits, e.g. mailto:you@example.com or a form's URL.
        #[arg(long, env = "TRACE_CONTACT_URL")]
        contact_url: Option<String>,
        /// Also serve the instrumented Stride demo shop on this address, e.g. 127.0.0.1:4319.
        #[arg(long)]
        demo_site: Option<SocketAddr>,
        /// Keep one website's KPIs and event catalogue in sync with this trace.json (with --site).
        #[arg(long, env = "TRACE_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Validate a trace.json without applying it. Exits non-zero on errors, for CI.
    Check {
        #[arg(default_value = config::DEFAULT_FILE)]
        file: PathBuf,
    },
    /// Apply a trace.json to a website: on a running server with --server, otherwise in the local database.
    Push {
        #[arg(default_value = config::DEFAULT_FILE)]
        file: PathBuf,
        /// Server URL, e.g. https://trace.example.com
        #[arg(long, env = "TRACE_SERVER")]
        server: Option<String>,
        /// A personal API token (create one under Account in the dashboard).
        #[arg(long, env = "TRACE_TOKEN")]
        token: Option<String>,
    },
    /// Analyse a website's recent behaviour and create improvement insights.
    Analyze {
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
        /// Days of behaviour to analyse.
        #[arg(long, default_value_t = 14)]
        days: u32,
        #[arg(long, default_value_t = 5)]
        max_insights: usize,
    },
    /// Measure released insights against their KPIs (one website with --site, otherwise all).
    Evaluate,
    /// List a website's insights.
    Insights {
        /// new, selected, in_progress, released or dismissed.
        #[arg(long)]
        status: Option<InsightStatus>,
    },
    /// Print an insight as a prompt for a coding agent, e.g. `trace prompt 3 | claude`.
    Prompt { id: i64 },
    /// Move an insight to another status.
    Status {
        id: i64,
        status: InsightStatus,
        /// Why the insight was dismissed; the analyst learns from it.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run an MCP server on stdin/stdout for a local coding agent (Claude Code, Cursor, ...),
    /// acting as an admin of one account. Remote agents use the server's /mcp endpoint with a token.
    Mcp {
        /// The account's id or name. Can be left out when there is one account.
        #[arg(long)]
        account: Option<String>,
    },
    /// Manage people from the server, e.g. when an owner is locked out.
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Grant or remove operator rights (the internal admin area). Only possible from here.
    Operator {
        #[command(subcommand)]
        action: OperatorAction,
    },
    /// List, add or remove websites.
    Site {
        #[command(subcommand)]
        action: SiteAction,
    },
    /// Fill an empty database with demo accounts and 30 days of simulated traffic for the Stride shop.
    /// Then `trace serve --demo-site 127.0.0.1:4319` serves the shop itself.
    Demo {
        #[arg(long, default_value_t = 4000)]
        sessions: usize,
    },
}

#[derive(Subcommand)]
enum UserAction {
    /// Create a person. The password comes from TRACE_PASSWORD, or a random one is printed.
    /// The server's first person gets a new account as its owner.
    Create {
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "")]
        name: String,
        /// owner, admin or user.
        #[arg(long, default_value = "user")]
        role: String,
        /// The account's id or name. Can be left out when there is one account.
        #[arg(long)]
        account: Option<String>,
    },
    /// List people, with their accounts.
    List,
    /// Set a new random password (printed once) and sign the person out everywhere.
    ResetPassword {
        #[arg(long)]
        email: String,
    },
    /// Turn off two-step verification for someone who lost their device; they set it up again at next sign-in.
    ResetMfa {
        #[arg(long)]
        email: String,
    },
}

#[derive(Subcommand)]
enum OperatorAction {
    /// Give a person operator rights. They also need two-step verification to use the operator area.
    Grant { email: String },
    /// Remove a person's operator rights.
    Revoke { email: String },
    /// List operators.
    List,
}

#[derive(Subcommand)]
enum SiteAction {
    /// List websites (all accounts, or one with --account).
    List {
        #[arg(long)]
        account: Option<String>,
    },
    /// Add a website to an account (within its plan's limit).
    Add {
        name: String,
        #[arg(long)]
        account: Option<String>,
    },
    /// Remove a website and everything collected for it. Needs the website's name to confirm.
    Remove {
        /// The website's identifier.
        slug: String,
        #[arg(long)]
        account: Option<String>,
        /// The website's name, to confirm.
        #[arg(long)]
        confirm: String,
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

fn main() -> Result<()> {
    // trace.env is read before anything else, while the process has one thread.
    setup::load_env_file();
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    if let Command::Init { database, force } = cli.command {
        // --db given explicitly (flag or environment) is used as the answer.
        let explicit = std::env::args().any(|a| a == "--db" || a.starts_with("--db="))
            || (std::env::var_os("TRACE_DB").is_some() && !setup::from_env_file("TRACE_DB"));
        return setup::init(database, explicit.then(|| cli.db.clone()), force).await;
    }
    let pool = db::connect(&cli.db).await.with_context(|| format!("opening {}", db::describe(&cli.db)))?;
    let claude = ClaudeConfig::from_env();
    let site = cli.site.as_deref();

    match cli.command {
        Command::Init { .. } => unreachable!("handled before opening the database"),
        Command::Serve { addr, admin_token, evaluate_every, retention_days, public_url, allow_signup, contact_url, demo_site, config: config_file } => {
            if let Some(path) = config_file {
                let target = db::resolve_site(&pool, site).await.context("--config needs a website (--site)")?;
                // Fail fast on a bad file at startup; later edits are applied as they're saved.
                apply_file(&pool, target.id, &path).await?;
                let pool = pool.clone();
                tokio::spawn(async move {
                    let mut last = modified(&path);
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        let now = modified(&path);
                        if now != last {
                            last = now;
                            if let Err(e) = apply_file(&pool, target.id, &path).await {
                                tracing::error!("{}: {e:#}", path.display());
                            }
                        }
                    }
                });
            }
            {
                let pool = pool.clone();
                let every = if evaluate_every > 0 { evaluate_every } else { 60 };
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(Duration::from_secs(every * 60));
                    loop {
                        tick.tick().await;
                        match retention::purge(&pool, retention_days).await {
                            Ok(n) if n > 0 => tracing::info!("retention: deleted {n} old events"),
                            Ok(_) => {}
                            Err(e) => tracing::error!(error = %format!("{e:#}"), "retention purge failed"),
                        }
                        if evaluate_every > 0
                            && let Err(e) = evaluate_all(&pool).await
                        {
                            tracing::error!(error = %format!("{e:#}"), "scheduled evaluation failed");
                        }
                    }
                });
            }
            let public_url = public_url.unwrap_or_else(|| format!("http://{addr}"));
            if let Some(site_addr) = demo_site {
                let shop = db::resolve_site(&pool, Some(site.unwrap_or("stride"))).await.context("--demo-site needs the demo data: run `trace demo` first")?;
                let app = demo::site(&public_url, &shop.write_key);
                let listener = tokio::net::TcpListener::bind(site_addr).await?;
                tracing::info!("Stride demo shop on http://{site_addr}");
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!(error = %e, "demo site stopped");
                    }
                });
            }
            let setup_code = if trace_core::auth::total_users(&pool).await? == 0 {
                let code = trace_core::auth::random_token(12);
                tracing::warn!("No accounts yet. Create the first account here (this link works once): {public_url}/#/welcome/{code}");
                Some(code)
            } else {
                None
            };
            let mut state = api::AppState::new(pool, claude);
            state.admin_token = admin_token;
            state.setup_code = std::sync::Mutex::new(setup_code);
            state.secure_cookies = public_url.starts_with("https://");
            state.allow_signup = allow_signup;
            state.contact_url = contact_url;
            state.retention_days = retention_days;
            let app = api::router(Arc::new(state));
            // Website paths are rewritten before routing, so the rewrite wraps the whole router.
            let app = tower::Layer::layer(&tower::util::MapRequestLayer::new(api::route_site_paths), app);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            tracing::info!("Database: {}", db::describe(&cli.db));
            if let Some(warning) = setup::sqlite_in_production(&cli.db, &addr, Some(&public_url)) {
                tracing::warn!("{warning}");
            }
            tracing::info!("Trace listening on http://{addr}");
            axum::serve(listener, axum::ServiceExt::<axum::extract::Request>::into_make_service_with_connect_info::<SocketAddr>(app))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::Analyze { engine, days, max_insights } => {
            let target = db::resolve_site(&pool, site).await?;
            let opts = trace_agent::Options { engine: engine.into(), days, max_insights };
            let report = analysis::run(&pool, &target, claude.as_ref(), &opts).await?;
            if let Some(w) = &report.warning {
                eprintln!("warning: {w}");
            }
            println!(
                "{}: {} new insight(s) from {}{} ({} already existed)",
                target.slug,
                report.created.len(),
                report.engine,
                report.model.map(|m| format!(" ({m})")).unwrap_or_default(),
                report.duplicates
            );
            for id in report.created {
                if let Some(i) = insights::get(&pool, target.id, id).await? {
                    println!("  #{:<4} {}", i.id, i.title);
                }
            }
        }
        Command::Evaluate => {
            let targets = match site {
                Some(_) => vec![db::resolve_site(&pool, site).await?],
                None => all_sites(&pool).await?,
            };
            let mut any = false;
            for t in targets {
                for e in evaluate::run(&pool, t.id, evaluate::Options::default()).await? {
                    any = true;
                    println!("{:<12} #{:<4} {:<17} {}", t.slug, e.insight_id, e.verdict.as_str(), e.summary);
                }
            }
            if !any {
                println!("No released insights linked to a KPI.");
            }
        }
        Command::Insights { status } => {
            let target = db::resolve_site(&pool, site).await?;
            for i in insights::list(&pool, target.id, status).await? {
                println!("#{:<4} {:<11} {:<10} {}", i.id, i.status.as_str(), i.source, i.title);
            }
        }
        Command::Prompt { id } => {
            let target = db::resolve_site(&pool, site).await?;
            let insight = insights::get(&pool, target.id, id).await?.with_context(|| format!("no insight #{id} on {}", target.slug))?;
            print!("{}", render_prompt(&pool, &target, &insight).await?);
        }
        Command::Status { id, status, reason } => {
            let target = db::resolve_site(&pool, site).await?;
            let insight = insights::set_status(&pool, target.id, id, status, reason.as_deref())
                .await?
                .with_context(|| format!("no insight #{id} on {}", target.slug))?;
            println!("#{} is now {}", insight.id, insight.status);
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
            Some(server) => push_remote(&file, &server, token.as_deref(), site).await?,
            None => {
                let target = db::resolve_site(&pool, site).await?;
                let report = apply_file(&pool, target.id, &file).await?;
                print_report(&report);
            }
        },
        Command::Mcp { account } => {
            let account = resolve_account(&pool, account.as_deref()).await?;
            let ctx = mcp::Context {
                pool: &pool,
                account_id: account.id,
                claude: claude.as_ref(),
                actor: "mcp (local)".into(),
                // Whoever can run this can already read the database file.
                role: trace_core::auth::Role::Admin,
                read_only: false,
            };
            mcp::serve_stdio(ctx).await?;
        }
        Command::User { action } => user_command(&pool, action).await?,
        Command::Operator { action } => operator_command(&pool, action).await?,
        Command::Site { action } => site_command(&pool, action).await?,
        Command::Demo { sessions } => demo::seed(&pool, sessions).await?,
    }
    Ok(())
}

async fn all_sites(pool: &Pool) -> Result<Vec<Project>> {
    let mut out = Vec::new();
    for a in db::accounts(pool).await? {
        out.extend(db::sites(pool, a.id).await?);
    }
    Ok(out)
}

async fn evaluate_all(pool: &Pool) -> Result<()> {
    for s in all_sites(pool).await? {
        evaluate::run(pool, s.id, evaluate::Options::default()).await?;
    }
    Ok(())
}

/// An account by id or name, or the only account when there is one.
async fn resolve_account(pool: &Pool, which: Option<&str>) -> Result<db::Account> {
    let accounts = db::accounts(pool).await?;
    let names = || accounts.iter().map(|a| format!("{} ({})", a.id, a.name)).collect::<Vec<_>>().join(", ");
    match which {
        Some(w) => accounts
            .iter()
            .find(|a| a.id.to_string() == w || a.name.eq_ignore_ascii_case(w))
            .cloned()
            .with_context(|| format!("no account {w:?}; accounts: {}", names())),
        None => match accounts.as_slice() {
            [only] => Ok(only.clone()),
            [] => anyhow::bail!("there are no accounts yet"),
            _ => anyhow::bail!("there are several accounts; choose one with --account ({})", names()),
        },
    }
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

async fn apply_file(pool: &Pool, project_id: i64, path: &Path) -> Result<config::SyncReport> {
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

async fn push_remote(path: &Path, server: &str, token: Option<&str>, site: Option<&str>) -> Result<()> {
    read_config(path)?; // validate locally first, with the same messages as `check`
    let body = std::fs::read_to_string(path)?;
    let url = match site {
        Some(s) => format!("{}/api/sites/{s}/config", server.trim_end_matches('/')),
        None => format!("{}/api/config", server.trim_end_matches('/')),
    };
    let mut req = reqwest::Client::new()
        .post(url)
        .header("content-type", "application/json")
        .header("x-trace-source", source_of(path))
        .body(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await.with_context(|| format!("connecting to {server}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let v = serde_json::from_str::<serde_json::Value>(&text).ok();
        let mut message = v.as_ref().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or(text);
        if let Some(sites) = v.as_ref().and_then(|v| v["sites"].as_array()).filter(|s| !s.is_empty()) {
            let slugs: Vec<&str> = sites.iter().filter_map(|s| s["slug"].as_str()).collect();
            message.push_str(&format!("; use --site with one of: {}", slugs.join(", ")));
        }
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

pub async fn render_prompt(pool: &Pool, project: &Project, insight: &trace_core::model::Insight) -> Result<String> {
    let kpi = match insight.kpi_id {
        Some(id) => kpi::get(pool, project.id, id).await?,
        None => None,
    };
    let kpi_value = match &kpi {
        Some(k) => {
            let now = trace_core::util::now_ms();
            kpi::measure(pool, project.id, &k.definition, now - 14 * DAY_MS, now).await?.value
        }
        None => None,
    };
    Ok(prompt::render(insight, &prompt::PromptContext { product: &project.name, kpi: kpi.as_ref(), kpi_value }))
}

async fn user_command(pool: &Pool, action: UserAction) -> Result<()> {
    use trace_core::auth::{self, Role};
    let find = |email: String| async move { auth::find_user(pool, &email).await?.with_context(|| format!("no person with the email {email}")) };
    match action {
        UserAction::Create { email, name, role, account } => {
            let (password, generated) = match std::env::var("TRACE_PASSWORD") {
                Ok(p) if !p.is_empty() => (p, false),
                _ => (auth::random_token(15), true),
            };
            let (account_id, role) = if auth::total_users(pool).await? == 0 && db::accounts(pool).await?.is_empty() {
                let label = email.split('@').nth(1).unwrap_or("My account").to_string();
                (db::create_account_with_site(pool, &label, None).await?.0.id, Role::Owner)
            } else {
                (resolve_account(pool, account.as_deref()).await?.id, role.parse()?)
            };
            let user = auth::create_user(pool, account_id, &email, &name, &password, role).await?;
            auth::audit(pool, account_id, "cli", "member.created", &format!("{} as {}", user.email, user.role)).await?;
            println!("Created {} ({}) in account {}.", user.email, user.role, account_id);
            if generated {
                println!("Password: {password}\n(shown once; change it under Account after signing in)");
            }
        }
        UserAction::List => {
            for a in db::accounts(pool).await? {
                for u in auth::list_users(pool, a.id).await? {
                    println!(
                        "{:<24} {:<32} {:<6} 2FA {:<3} {}{}",
                        a.name,
                        u.email,
                        u.role.as_str(),
                        if u.totp_enabled { "on" } else { "off" },
                        u.name,
                        if u.is_operator { "  [operator]" } else { "" }
                    );
                }
            }
        }
        UserAction::ResetPassword { email } => {
            let user = find(email).await?;
            let password = auth::random_token(15);
            auth::set_password(pool, user.id, &password).await?;
            auth::end_user_sessions(pool, user.id, None).await?;
            auth::audit(pool, user.account_id, "cli", "account.password_reset", &user.email).await?;
            println!("New password for {}: {password}\n(shown once; signed out everywhere)", user.email);
        }
        UserAction::ResetMfa { email } => {
            let user = find(email).await?;
            auth::disable_totp(pool, user.id).await?;
            auth::end_user_sessions(pool, user.id, None).await?;
            auth::audit(pool, user.account_id, "cli", "mfa.reset", &user.email).await?;
            println!("Two-step verification is off for {}; they'll set it up again at next sign-in if it's required.", user.email);
        }
    }
    Ok(())
}

async fn operator_command(pool: &Pool, action: OperatorAction) -> Result<()> {
    use trace_core::auth;
    match action {
        OperatorAction::Grant { email } => {
            let u = auth::set_operator(pool, &email, true).await?;
            println!("{} is now an operator.", u.email);
            if !u.totp_enabled {
                println!("They need two-step verification (Account → Two-step verification) before the operator area opens.");
            }
        }
        OperatorAction::Revoke { email } => {
            let u = auth::set_operator(pool, &email, false).await?;
            println!("{} is no longer an operator.", u.email);
        }
        OperatorAction::List => {
            for u in auth::operators(pool).await? {
                println!("{:<32} 2FA {}", u.email, if u.totp_enabled { "on" } else { "off (needed for the operator area)" });
            }
        }
    }
    Ok(())
}

async fn site_command(pool: &Pool, action: SiteAction) -> Result<()> {
    match action {
        SiteAction::List { account } => {
            let accounts = match account {
                Some(a) => vec![resolve_account(pool, Some(&a)).await?],
                None => db::accounts(pool).await?,
            };
            for a in accounts {
                for s in db::sites(pool, a.id).await? {
                    let c = db::collection(pool, s.id).await?;
                    println!("{:<24} {:<20} {:<24} {}", a.name, s.slug, s.name, if c.active { "collecting" } else { "not collecting" });
                }
            }
        }
        SiteAction::Add { name, account } => {
            let a = resolve_account(pool, account.as_deref()).await?;
            let s = db::create_site(pool, a.id, &name).await?;
            trace_core::auth::audit(pool, a.id, "cli", "site.added", &format!("{} ({})", s.name, s.slug)).await?;
            println!("Added {} ({}) to {}. Write key: {}", s.name, s.slug, a.name, s.write_key);
        }
        SiteAction::Remove { slug, account, confirm } => {
            let a = resolve_account(pool, account.as_deref()).await?;
            let s = db::site_by_slug(pool, a.id, &slug).await?.with_context(|| format!("no website {slug:?} in {}", a.name))?;
            anyhow::ensure!(confirm == s.name, "to confirm, pass --confirm {:?}", s.name);
            db::delete_site(pool, a.id, s.id).await?;
            trace_core::auth::audit(pool, a.id, "cli", "site.removed", &format!("{} ({}) and all its data", s.name, s.slug)).await?;
            println!("Removed {} and all its data.", s.name);
        }
    }
    Ok(())
}
