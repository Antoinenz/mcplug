use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mcplug", version, about = "Plugin and server-jar manager for Minecraft servers")]
struct Cli {
    /// Config directory (default: ~/.config/mcplug, or /etc/mcplug as root)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Machine-readable output where supported
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List detected servers (MCSManager instances + manual entries)
    Servers,
    /// Scan a server's plugins and identify them on Modrinth/Hangar/GeyserMC
    Scan {
        server: String,
        /// Accept a candidate whose name matches exactly, even without a hash match
        #[arg(long)]
        accept_exact: bool,
    },
    /// Check for available updates
    Check { server: Option<String> },
    /// Update plugins (all with updates, or the named ones)
    Update {
        server: String,
        plugins: Vec<String>,
        /// Install this exact version id (single plugin only)
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        allow_unverified: bool,
        /// Don't ask for confirmation
        #[arg(short, long)]
        yes: bool,
        /// Show the plan and stop
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        flow: Flow,
    },
    /// Install a plugin from a URL, `owner/repo`, `hangar:<slug>` or a Modrinth slug
    Install {
        server: String,
        target: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        allow_unverified: bool,
        #[arg(short, long)]
        yes: bool,
        #[command(flatten)]
        flow: Flow,
    },
    /// Undo a transaction (default: the last applied one)
    Revert { server: String, tx: Option<String> },
    /// Show what mcplug has done to a server
    History { server: String },
    /// Server jar: show build status; --update to the newest build; --mc <version> to upgrade Minecraft
    Jar {
        server: String,
        #[arg(long)]
        update: bool,
        #[arg(long)]
        mc: Option<String>,
        /// With --mc: only print the plugin compatibility table
        #[arg(long)]
        check_only: bool,
        #[arg(short, long)]
        yes: bool,
        #[command(flatten)]
        flow: Flow,
    },
    /// Run the scheduler: periodic checks and policy-driven automatic updates
    Daemon,
    /// Store a token: `auth modrinth <PAT>`, `auth github <token>`, `auth mcsm <key>`, `auth rcon/<server> <pw>`; no value = show status
    Auth { what: Option<String>, value: Option<String> },
    /// Open the terminal UI (default)
    Tui,
}

#[derive(clap::Args)]
struct Flow {
    /// When to restart the server: now, when-empty, never
    #[arg(long, default_value = "never")]
    restart: String,
    /// Seconds of in-game warning before a restart
    #[arg(long, default_value_t = 60)]
    countdown: u32,
    /// Skip the mcbackup checkpoint/snapshot even if mcbackup is installed
    #[arg(long)]
    no_backup: bool,
}

impl From<Flow> for mcplug::cli::update::FlowArgs {
    fn from(f: Flow) -> Self {
        Self {
            restart: f.restart,
            countdown: f.countdown,
            no_backup: f.no_backup,
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("MCPLUG_LOG"))
        .with_writer(std::io::stderr)
        .init();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> mcplug::Result<()> {
    let loaded = mcplug::config::Loaded::load(cli.config)?;
    let http = mcplug::http::client(loaded.config.contact.as_deref());
    let ctx = mcplug::cli::Ctx { loaded, http, json: cli.json };
    match cli.cmd.unwrap_or(Cmd::Tui) {
        Cmd::Tui => mcplug::tui::run(ctx).await,
        Cmd::Update {
            server,
            plugins,
            version,
            allow_unverified,
            yes,
            dry_run,
            flow,
        } => {
            mcplug::cli::update::update(
                &ctx,
                &mcplug::cli::update::UpdateArgs {
                    server,
                    plugins,
                    version,
                    allow_unverified,
                    yes,
                    dry_run,
                    flow: flow.into(),
                },
            )
            .await
        }
        Cmd::Install {
            server,
            target,
            version,
            allow_unverified,
            yes,
            flow,
        } => {
            mcplug::cli::update::install(
                &ctx,
                &mcplug::cli::update::InstallArgs {
                    server,
                    target,
                    version,
                    allow_unverified,
                    yes,
                    flow: flow.into(),
                },
            )
            .await
        }
        Cmd::Revert { server, tx } => mcplug::cli::update::revert(&ctx, &server, tx.as_deref()).await,
        Cmd::History { server } => mcplug::cli::update::history(&ctx, &server).await,
        Cmd::Jar {
            server,
            update,
            mc,
            check_only,
            yes,
            flow,
        } => {
            mcplug::cli::jar::run(
                &ctx,
                &mcplug::cli::jar::JarArgs {
                    server,
                    update,
                    mc,
                    check_only,
                    yes,
                    flow: flow.into(),
                },
            )
            .await
        }
        Cmd::Daemon => mcplug::daemon::run(ctx).await,
        Cmd::Auth { what: None, .. } => mcplug::cli::auth::status(&ctx),
        Cmd::Auth { what: Some(w), value } => mcplug::cli::auth::set(&ctx, &w, value.as_deref()).await,
        Cmd::Servers => mcplug::cli::servers::run(&ctx).await,
        Cmd::Scan { server, accept_exact } => mcplug::cli::scan::run(&ctx, &mcplug::cli::scan::ScanArgs { server, accept_exact }).await,
        Cmd::Check { server } => {
            let updates = mcplug::cli::check::run(&ctx, server.as_deref()).await?;
            if updates {
                std::process::exit(2);
            }
            Ok(())
        }
    }
}
