use crate::lockfile::{LockFile, LockServerJar};
use crate::plugins::scan_plugins;
use crate::sources::identify::{self, IdentifyOptions};
use crate::sources::Confidence;
use crate::Result;

use super::{server_platform, Ctx};

pub struct ScanArgs {
    pub server: String,
    /// Accept single exact-name candidates without asking.
    pub accept_exact: bool,
}

pub async fn run(ctx: &Ctx, args: &ScanArgs) -> Result<()> {
    let server = ctx.server(&args.server)?;
    let (platform, cctx) = server_platform(&server)?;
    let plugins_dir = server.plugins_dir();
    let scan = scan_plugins(&platform, &plugins_dir);
    for e in &scan.errors {
        eprintln!("warning: {e}");
    }
    let mut lock = LockFile::load_or_new(&plugins_dir)?;
    let sources = ctx.sources();
    let report = identify::identify(&sources, &lock, scan.jars, &cctx, &IdentifyOptions { accept_exact_name: args.accept_exact }).await;
    for e in &report.errors {
        eprintln!("warning: {e}");
    }

    // Update the lock: drop entries whose jar is gone, add identified, record unidentified.
    let present: std::collections::HashSet<String> = report.unchanged.iter().chain(report.identified.iter().map(|i| &i.jar)).chain(report.undecided.iter().map(|u| &u.jar)).map(|j| j.hashes.sha512.clone()).collect();
    lock.plugins.retain(|p| present.contains(&p.hashes.sha512));
    for id in &report.identified {
        let entry = identify::entry_for(id);
        lock.plugins.retain(|p| p.name != entry.name);
        lock.plugins.push(entry);
    }
    for u in &report.undecided {
        let entry = identify::unidentified_entry(&u.jar);
        lock.plugins.retain(|p| p.name != entry.name);
        lock.plugins.push(entry);
    }
    lock.plugins.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    lock.server.platform = Some(server.platform);
    lock.server.mc_version = Some(cctx.mc_version.clone());
    lock.server.last_scan = Some(chrono::Utc::now());
    lock.server.jar = server.jar.as_ref().map(|j| LockServerJar { provider: j.platform.to_string(), file: j.file_name.clone(), build: j.build_hint, sha256: j.sha256.clone() });
    if server.access.writable() {
        lock.save(&plugins_dir)?;
    } else {
        eprintln!("note: {} — lock not written", match &server.access { crate::server::Access::ReadOnly { reason } => reason.clone(), _ => "plugin dir missing".into() });
    }

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&lock)?);
        return Ok(());
    }
    println!("{} ({} {}) — {} plugins, {} unchanged, {} identified, {} need a decision, {} skipped", server.name, server.platform, cctx.mc_version, lock.plugins.len(), report.unchanged.len(), report.identified.len(), report.undecided.len(), report.skipped.len());
    if !scan.duplicates.is_empty() {
        println!("  ! duplicate plugins (two jars with the same name): {}", scan.duplicates.join(", "));
    }
    if !scan.staged_updates.is_empty() {
        println!("  ! plugins/update/ already holds: {}", scan.staged_updates.join(", "));
    }
    for id in &report.identified {
        println!("  + {:<24} {:<9} {:<28} {}", id.jar.descriptor.as_ref().map(|d| d.name.as_str()).unwrap_or("?"), id.project.source, id.version.version_number, id.project.page_url);
    }
    for u in &report.undecided {
        let name = u.jar.descriptor.as_ref().map(|d| d.name.as_str()).unwrap_or("?");
        println!("  ? {:<24} {}", name, u.jar.file);
        for c in u.candidates.iter().take(4) {
            println!("      {:<14} {:<9} {:<30} {}", confidence_str(c.confidence), c.project.source, c.project.name, c.project.page_url);
        }
    }
    for j in &report.skipped {
        println!("  - {:<24} no plugin descriptor for this platform", j.file);
    }
    Ok(())
}

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::HashConfirmed => "hash-match",
        Confidence::ExactName => "exact-name",
        Confidence::NameMatch => "name-match",
        Confidence::Weak => "weak",
    }
}
