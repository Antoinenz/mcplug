use std::sync::Arc;

use crate::ops;
use crate::server::discover;
use crate::Result;

use super::update::FlowArgs;
use super::Ctx;

pub async fn install(ctx: &Ctx, server: &str, flow: &FlowArgs) -> Result<()> {
    let server = ctx.server(server)?;
    let all = discover(&ctx.loaded.config);
    let opts = super::update::apply_options(ctx, &server, flow)?;
    let progress: crate::transaction::ProgressFn = Arc::new(|p| {
        if let crate::transaction::Progress::Step(s) = p {
            eprintln!("· {s}");
        }
    });
    ops::install_bridge(&server, &all, ctx.loaded.config.daemon.api_port, opts.control.as_ref(), &opts.restart, progress).await?;
    println!(
        "McplugBridge is installed on {}. In-game: /mcplug (operators only). The daemon API on port {} answers it.",
        server.name, ctx.loaded.config.daemon.api_port
    );
    Ok(())
}

pub async fn status(ctx: &Ctx, server: &str) -> Result<()> {
    let server = ctx.server(server)?;
    let Some(cfg) = crate::control::companion::BridgeConfig::load(&server.plugins_dir()) else {
        println!("{}: bridge not installed (`mcplug bridge {} --install`)", server.name, server.id);
        return Ok(());
    };
    let c = crate::control::companion::Companion::wrap(ctx.control(&server), cfg.clone(), ctx.http.clone());
    match c.bridge_status().await {
        Ok(s) => println!(
            "{}: bridge {} on port {} — {} online ({}), tps {:.1}, mspt {:.1}, mc {}",
            server.name,
            s.bridge,
            cfg.port,
            s.players,
            s.names.join(", "),
            s.tps,
            s.mspt,
            s.version
        ),
        Err(e) => println!(
            "{}: bridge configured on port {} but not answering ({e}) — is the server running with the plugin loaded?",
            server.name, cfg.port
        ),
    }
    Ok(())
}
