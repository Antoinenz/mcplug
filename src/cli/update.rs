use std::sync::Arc;

use crate::lockfile::LockFile;
use crate::ops;
use crate::sources::ProjectLocator;
use crate::transaction::{self, PlanRequest, Progress};
use crate::{Error, Result};

use super::Ctx;

pub struct UpdateArgs {
    pub server: String,
    /// Plugin names; empty = every plugin with an available update.
    pub plugins: Vec<String>,
    pub version: Option<String>,
    pub allow_unverified: bool,
    pub yes: bool,
    pub dry_run: bool,
}

pub struct InstallArgs {
    pub server: String,
    /// URL, `owner/repo`, or a Modrinth slug/id.
    pub target: String,
    pub version: Option<String>,
    pub allow_unverified: bool,
    pub yes: bool,
}

pub async fn update(ctx: &Ctx, a: &UpdateArgs) -> Result<()> {
    let server = ctx.server(&a.server)?;
    let (platform, cctx) = ops::server_platform(&server)?;
    let plugins_dir = server.plugins_dir();
    let mut lock = LockFile::load(&plugins_dir)?.ok_or_else(|| Error::Msg(format!("{}: not scanned yet — run `mcplug scan {}`", server.name, server.id)))?;
    let sources = ctx.sources();

    let requests: Vec<PlanRequest> = if a.plugins.is_empty() {
        let report = ops::check_server(&server, &sources, &lock).await?;
        report.updates.iter().filter(|u| !u.untested).map(|u| PlanRequest::UpdateLatest { name: u.name.clone() }).collect()
    } else {
        a.plugins
            .iter()
            .map(|name| {
                let name = lock.plugins.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.name.clone()).unwrap_or_else(|| name.clone());
                match &a.version {
                    Some(v) => PlanRequest::UpdateTo { name, version_id: v.clone() },
                    None => PlanRequest::UpdateLatest { name },
                }
            })
            .collect()
    };
    if requests.is_empty() {
        println!("{}: nothing to update", server.name);
        return Ok(());
    }
    let plan = transaction::build_plan(&server, &lock, &sources, &cctx, requests, a.allow_unverified).await?;
    print_plan(&plan);
    if plan.is_empty() || a.dry_run {
        return Ok(());
    }
    if !a.yes && !confirm("apply?")? {
        return Ok(());
    }
    let out = transaction::apply(&server, &platform, &mut lock, &sources, &plan, Arc::new(cli_progress)).await?;
    println!("\napplied transaction {} ({} plugin{}). restart {} to load the new versions; `mcplug revert {} {}` undoes it.", out.tx_id, out.applied.len(), if out.applied.len() == 1 { "" } else { "s" }, server.name, server.id, out.tx_id);
    Ok(())
}

pub async fn install(ctx: &Ctx, a: &InstallArgs) -> Result<()> {
    let server = ctx.server(&a.server)?;
    let (platform, cctx) = ops::server_platform(&server)?;
    let plugins_dir = server.plugins_dir();
    let mut lock = LockFile::load_or_new(&plugins_dir)?;
    let sources = ctx.sources();
    let locator = locate(&sources, &a.target)?;
    let plan = transaction::build_plan(&server, &lock, &sources, &cctx, vec![PlanRequest::Install { locator, version_id: a.version.clone() }], a.allow_unverified).await?;
    print_plan(&plan);
    if plan.is_empty() {
        return Ok(());
    }
    if !a.yes && !confirm("install?")? {
        return Ok(());
    }
    let out = transaction::apply(&server, &platform, &mut lock, &sources, &plan, Arc::new(cli_progress)).await?;
    println!("\ninstalled ({}). restart {} to load it.", out.tx_id, server.name);
    Ok(())
}

pub fn locate(sources: &crate::sources::Sources, target: &str) -> Result<ProjectLocator> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return sources.locate(target).ok_or_else(|| Error::Msg(format!("{target}: not a Modrinth, Hangar, GitHub or GeyserMC URL")));
    }
    if let Some((src, id)) = target.split_once(':') {
        let source = match src {
            "modrinth" => crate::sources::SourceKind::Modrinth,
            "hangar" => crate::sources::SourceKind::Hangar,
            "github" => crate::sources::SourceKind::GitHub,
            "geysermc" | "geyser" => crate::sources::SourceKind::GeyserMc,
            _ => return Err(Error::Msg(format!("unknown source prefix {src:?}"))),
        };
        return Ok(ProjectLocator { source, id: id.to_string() });
    }
    if target.matches('/').count() == 1 {
        return Ok(ProjectLocator { source: crate::sources::SourceKind::GitHub, id: target.to_string() });
    }
    Ok(ProjectLocator { source: crate::sources::SourceKind::Modrinth, id: target.to_string() })
}

pub async fn revert(ctx: &Ctx, server: &str, tx: Option<&str>) -> Result<()> {
    let server = ctx.server(server)?;
    let plugins_dir = server.plugins_dir();
    let mut lock = LockFile::load_or_new(&plugins_dir)?;
    let tx = match tx {
        Some(t) => t.to_string(),
        None => transaction::journal::read(&plugins_dir).into_iter().rev().find(|e| e.outcome == "applied").map(|e| e.tx_id).ok_or_else(|| Error::Msg("no applied transaction to revert".into()))?,
    };
    let names = transaction::revert(&server, &mut lock, &tx)?;
    println!("reverted {tx}: {}", names.join(", "));
    Ok(())
}

pub async fn history(ctx: &Ctx, server: &str) -> Result<()> {
    let server = ctx.server(server)?;
    let entries = transaction::journal::read(&server.plugins_dir());
    if entries.is_empty() {
        println!("no history for {}", server.name);
        return Ok(());
    }
    for e in entries {
        println!("{}  {:<16} {:<8} {:<9} {}", e.time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"), e.tx_id, e.action, e.outcome, e.items.iter().map(|i| match &i.from { Some(f) => format!("{} {f}→{}", i.name, i.to), None => format!("+{} {}", i.name, i.to) }).collect::<Vec<_>>().join(", "));
        if let Some(n) = e.note {
            println!("{:>18}  {n}", "");
        }
    }
    Ok(())
}

fn print_plan(plan: &transaction::UpdatePlan) {
    for n in &plan.notes {
        println!("  · {n}");
    }
    for i in &plan.items {
        let from = i.from.as_ref().map(transaction::plan::installed_label).unwrap_or_else(|| "(new)".into());
        let dep = i.dependency_of.as_ref().map(|d| format!("  [required by {d}]")).unwrap_or_default();
        let unv = if i.unverified { "  [no checksum]" } else { "" };
        println!("  {:<22} {:<22} → {:<22} {:<9} {}{dep}{unv}", i.name, from, i.to.version_number, i.project.source, i.file.name);
    }
    for d in &plan.unresolved_deps {
        println!("  ! unresolved dependency: {d}");
    }
    if plan.is_empty() {
        println!("  nothing to do");
    }
}

fn confirm(q: &str) -> Result<bool> {
    use std::io::Write;
    print!("{q} [y/N] ");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(matches!(s.trim(), "y" | "Y" | "yes"))
}

fn cli_progress(p: Progress) {
    match p {
        Progress::Step(s) => eprintln!("· {s}"),
        Progress::Download { name, done, total: Some(t) } if done == t => eprintln!("  {name}: {} KB", done / 1000),
        _ => {}
    }
}
