//! Operations shared by the CLI, the TUI and the daemon.

use crate::lockfile::{CompatMode, LockFile, LockServerJar, SourceRef};
use crate::platform::Platform;
use crate::plugins::{scan_plugins, ScanResult};
use crate::server::Server;
use crate::sources::identify::{self, IdentifyOptions, IdentifyReport};
use crate::sources::resolve::{self, CheckReport};
use crate::sources::{CompatCtx, Sources};
use crate::{Error, Result};

/// Platform + compatibility context for a server, or an error if we can't manage it.
pub fn server_platform(server: &Server) -> Result<(crate::platform::bukkit::Bukkit, CompatCtx)> {
    let p = crate::platform::for_kind(server.platform).ok_or_else(|| Error::Msg(format!("{}: platform {} is not supported yet", server.id, server.platform)))?;
    let mc = server
        .jar
        .as_ref()
        .and_then(|j| j.mc_version.clone())
        .ok_or_else(|| Error::Msg(format!("{}: could not determine the Minecraft version from the server jar", server.id)))?;
    let ctx = CompatCtx {
        mc_version: mc,
        loaders: p.modrinth_loaders().iter().map(|s| s.to_string()).collect(),
        hangar_platform: p.hangar_platform().map(str::to_string),
        mode: CompatMode::Strict,
    };
    Ok((p, ctx))
}

pub struct ScanOutcome {
    pub scan: ScanResult,
    pub report: IdentifyReport,
    pub lock: LockFile,
    pub saved: bool,
}

/// Scan + identify + merge into the lockfile (saved when the plugin dir is writable).
pub async fn scan_server(server: &Server, sources: &Sources, accept_exact: bool) -> Result<ScanOutcome> {
    let (platform, cctx) = server_platform(server)?;
    let plugins_dir = server.plugins_dir();
    let scan = scan_plugins(&platform, &plugins_dir);
    let mut lock = LockFile::load_or_new(&plugins_dir)?;
    let report = identify::identify(sources, &lock, scan.jars.clone(), &cctx, &IdentifyOptions { accept_exact_name: accept_exact }).await;

    let present: std::collections::HashSet<String> = report
        .unchanged
        .iter()
        .chain(report.identified.iter().map(|i| &i.jar))
        .chain(report.undecided.iter().map(|u| &u.jar))
        .map(|j| j.hashes.sha512.clone())
        .collect();
    lock.plugins.retain(|p| present.contains(&p.hashes.sha512));
    for id in &report.identified {
        let entry = identify::entry_for(id);
        lock.plugins.retain(|p| p.name != entry.name || p.hashes.sha512 == entry.hashes.sha512);
        lock.plugins.push(entry);
    }
    for u in &report.undecided {
        let entry = identify::unidentified_entry(&u.jar);
        lock.plugins.retain(|p| p.name != entry.name || p.hashes.sha512 == entry.hashes.sha512);
        lock.plugins.push(entry);
    }
    lock.plugins.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    lock.server.platform = Some(server.platform);
    lock.server.mc_version = Some(cctx.mc_version.clone());
    lock.server.last_scan = Some(chrono::Utc::now());
    lock.server.jar = server.jar.as_ref().map(|j| LockServerJar { provider: j.platform.to_string(), file: j.file_name.clone(), build: j.build_hint, sha256: j.sha256.clone() });
    let saved = server.access.writable();
    if saved {
        lock.save(&plugins_dir)?;
    }
    Ok(ScanOutcome { scan, report, lock, saved })
}

pub async fn check_server(server: &Server, sources: &Sources, lock: &LockFile) -> Result<CheckReport> {
    let (_, cctx) = server_platform(server)?;
    Ok(resolve::check(sources, lock, &cctx, CompatMode::Strict).await)
}

/// Record a user's identification decision for a plugin and save.
pub fn set_source(server: &Server, lock: &mut LockFile, name: &str, source: SourceRef) -> Result<()> {
    let entry = lock.get_mut(name).ok_or_else(|| Error::Msg(format!("{name}: not in lockfile")))?;
    entry.source = source;
    lock.save(&server.plugins_dir())
}
