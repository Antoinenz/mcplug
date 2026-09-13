use crate::lockfile::LockFile;
use crate::ops;
use crate::server::discover;
use crate::Result;

use super::{server_platform, Ctx};

/// Exit code 2 when updates are available, so scripts can branch on it.
pub async fn run(ctx: &Ctx, server: Option<&str>) -> Result<bool> {
    let servers = match server {
        Some(q) => vec![ctx.server(q)?],
        None => discover(&ctx.loaded.config),
    };
    let sources = ctx.sources();
    let mut any = false;
    let mut all_json = Vec::new();
    for s in &servers {
        let Ok((_, cctx)) = server_platform(s) else { continue };
        let Some(lock) = LockFile::load(&s.plugins_dir())? else {
            if server.is_some() {
                eprintln!("{}: not scanned yet — run `mcplug scan {}` first", s.name, s.id);
            }
            continue;
        };
        let report = ops::check_server(s, &sources, &lock).await?;
        any |= !report.updates.is_empty();
        if ctx.json {
            all_json.push(serde_json::json!({ "server": s.id, "report": report }));
            continue;
        }
        println!(
            "{} ({} {}): {} update{}, {} up to date, {} pinned, {} unmanaged",
            s.name,
            s.platform,
            cctx.mc_version,
            report.updates.len(),
            if report.updates.len() == 1 { "" } else { "s" },
            report.up_to_date.len(),
            report.pinned.len(),
            report.unmanaged.len()
        );
        for u in &report.updates {
            let flags = format!(
                "{}{}",
                if u.untested { " untested" } else { "" },
                if u.unverified { " unverified" } else { "" }
            );
            println!(
                "  ↑ {:<22} {:<24} → {:<24} {:<9} {}{}",
                u.name,
                u.installed,
                u.latest.version_number,
                u.latest.source,
                u.latest.published.format("%Y-%m-%d"),
                flags
            );
        }
        for e in &report.errors {
            println!("  ! {e}");
        }
    }
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&all_json)?);
    }
    Ok(any)
}
