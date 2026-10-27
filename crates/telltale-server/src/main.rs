mod api;
mod demo;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use telltale_agent::{ClaudeConfig, Engine};
use telltale_core::model::IdeaStatus;
use telltale_core::util::DAY_MS;
use telltale_core::{db, evaluate, ideas, kpi, prompt};

#[derive(Parser)]
#[command(name = "telltale", version, about = "Behaviour analytics for agents: collect, measure, suggest, evaluate.")]
struct Cli {
    /// SQLite database file.
    #[arg(long, global = true, env = "TELLTALE_DB", default_value = "telltale.db")]
    db: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the collector endpoint, API and dashboard.
    Serve {
        #[arg(long, env = "TELLTALE_ADDR", default_value = "127.0.0.1:4318")]
        addr: SocketAddr,
        /// Require this bearer token for the dashboard API. Set it whenever the server is reachable by others.
        #[arg(long, env = "TELLTALE_ADMIN_TOKEN")]
        admin_token: Option<String>,
        /// How often to re-evaluate released ideas, in minutes (0 turns it off).
        #[arg(long, default_value_t = 60)]
        evaluate_every: u64,
        /// URL browsers use to reach this server (for the collector script). Defaults to http://<addr>.
        #[arg(long, env = "TELLTALE_PUBLIC_URL")]
        public_url: Option<String>,
        /// Also serve the instrumented Stride demo shop on this address, e.g. 127.0.0.1:4319.
        #[arg(long)]
        demo_site: Option<SocketAddr>,
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
    /// Print an idea as a prompt for a coding agent, e.g. `telltale prompt 3 | claude`.
    Prompt { id: i64 },
    /// Move an idea to another status.
    Status {
        id: i64,
        status: IdeaStatus,
        /// Why the idea was dismissed; the analyst learns from it.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Fill the database with 30 days of simulated traffic for the Stride demo shop.
    /// Then `telltale serve --demo-site 127.0.0.1:4319` serves the shop itself.
    Demo {
        #[arg(long, default_value_t = 4000)]
        sessions: usize,
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
        Command::Serve { addr, admin_token, evaluate_every, public_url, demo_site } => {
            if admin_token.is_none() && !addr.ip().is_loopback() {
                tracing::warn!("listening on {addr} without TELLTALE_ADMIN_TOKEN: anyone who can reach it can use the dashboard API");
            }
            if evaluate_every > 0 {
                let pool = pool.clone();
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(Duration::from_secs(evaluate_every * 60));
                    loop {
                        tick.tick().await;
                        if let Err(e) = evaluate::run(&pool, project.id, evaluate::Options::default()).await {
                            tracing::error!(error = %format!("{e:#}"), "scheduled evaluation failed");
                        }
                    }
                });
            }
            if let Some(site_addr) = demo_site {
                let public_url = public_url.unwrap_or_else(|| format!("http://{addr}"));
                let site = demo::site(&public_url, &project.write_key);
                let listener = tokio::net::TcpListener::bind(site_addr).await?;
                tracing::info!("Stride demo shop on http://{site_addr}");
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, site).await {
                        tracing::error!(error = %e, "demo site stopped");
                    }
                });
            }
            let state = Arc::new(api::AppState { pool, project_id: project.id, admin_token, claude });
            let listener = tokio::net::TcpListener::bind(addr).await?;
            tracing::info!("Telltale listening on http://{addr}");
            axum::serve(listener, api::router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::Analyze { engine, days, max_ideas } => {
            let opts = telltale_agent::Options { engine: engine.into(), days, max_ideas };
            let report = telltale_agent::analyze(&pool, project.id, claude.as_ref(), &opts).await?;
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
        Command::Demo { sessions } => demo::seed(&pool, project.id, sessions).await?,
    }
    Ok(())
}

pub async fn render_prompt(
    pool: &telltale_core::SqlitePool,
    project: &telltale_core::model::Project,
    idea: &telltale_core::model::Idea,
) -> Result<String> {
    let kpi = match idea.kpi_id {
        Some(id) => kpi::get(pool, project.id, id).await?,
        None => None,
    };
    let kpi_value = match &kpi {
        Some(k) => {
            let now = telltale_core::util::now_ms();
            kpi::measure(pool, project.id, &k.definition, now - 14 * DAY_MS, now).await?.value
        }
        None => None,
    };
    Ok(prompt::render(idea, &prompt::PromptContext { product: &project.name, kpi: kpi.as_ref(), kpi_value }))
}
