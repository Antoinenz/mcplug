use crate::ops;
use crate::sources::Confidence;
use crate::Result;

use super::Ctx;

pub struct ScanArgs {
    pub server: String,
    /// Accept single exact-name candidates without asking.
    pub accept_exact: bool,
}

pub async fn run(ctx: &Ctx, args: &ScanArgs) -> Result<()> {
    let server = ctx.server(&args.server)?;
    let sources = ctx.sources();
    let out = ops::scan_server(&server, &sources, args.accept_exact).await?;
    for e in out.scan.errors.iter().chain(out.report.errors.iter()) {
        eprintln!("warning: {e}");
    }
    if !out.saved {
        eprintln!("note: lock not written — {}", match &server.access { crate::server::Access::ReadOnly { reason } => reason.clone(), _ => "plugin dir missing".into() });
    }
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&out.lock)?);
        return Ok(());
    }
    let (scan, report, lock) = (&out.scan, &out.report, &out.lock);
    let mc = lock.server.mc_version.as_ref().map(|v| v.to_string()).unwrap_or_default();
    println!("{} ({} {}) — {} plugins, {} unchanged, {} identified, {} need a decision, {} skipped", server.name, server.platform, mc, lock.plugins.len(), report.unchanged.len(), report.identified.len(), report.undecided.len(), report.skipped.len());
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

pub fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::HashConfirmed => "hash-match",
        Confidence::ExactName => "exact-name",
        Confidence::NameMatch => "name-match",
        Confidence::Weak => "weak",
    }
}
