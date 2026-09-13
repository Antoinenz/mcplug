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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_env("MCPLUG_LOG")).with_writer(std::io::stderr).init();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> mcplug::Result<()> {
    let loaded = mcplug::config::Loaded::load(cli.config)?;
    let http = mcplug::http::client(loaded.config.contact.as_deref());
    let ctx = mcplug::cli::Ctx { loaded, http, json: cli.json };
    match cli.cmd.unwrap_or(Cmd::Servers) {
        Cmd::Servers => mcplug::cli::servers::run(&ctx).await,
    }
}
