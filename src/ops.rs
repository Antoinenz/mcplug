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
    let p =
        crate::platform::for_kind(server.platform).ok_or_else(|| Error::Msg(format!("{}: platform {} is not supported yet", server.id, server.platform)))?;
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
    let report = identify::identify(
        sources,
        &lock,
        scan.jars.clone(),
        &cctx,
        &IdentifyOptions {
            accept_exact_name: accept_exact,
        },
    )
    .await;

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
        lock.plugins.retain(|p| p.name != entry.name);
        lock.plugins.push(entry);
    }
    for u in &report.undecided {
        let entry = identify::unidentified_entry(&u.jar);
        lock.plugins.retain(|p| p.name != entry.name);
        lock.plugins.push(entry);
    }
    lock.plugins.sort_by_key(|a| a.name.to_ascii_lowercase());
    lock.server.platform = Some(server.platform);
    lock.server.mc_version = Some(cctx.mc_version.clone());
    lock.server.last_scan = Some(chrono::Utc::now());
    lock.server.jar = match (&server.jar, server.jar_path()) {
        (Some(j), Some(path)) => Some(LockServerJar {
            provider: j.platform.to_string(),
            file: j.file_name.clone(),
            build: j.build_hint,
            sha256: crate::plugins::JarHashes::of_file(&path).map(|h| h.sha256).unwrap_or_default(),
        }),
        _ => None,
    };
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

/// Everything around a transaction: backup before, apply, restart, backup after — each step
/// journaled so the TUI can show what happened.
pub struct ApplyOptions {
    pub restart: crate::control::RestartPolicy,
    pub backup: Option<crate::backup::Mcbackup>,
    pub control: Box<dyn crate::control::ServerControl>,
}

pub async fn apply_plan(
    server: &Server,
    lock: &mut LockFile,
    sources: &Sources,
    plan: &crate::transaction::UpdatePlan,
    opts: &ApplyOptions,
    progress: crate::transaction::ProgressFn,
) -> Result<crate::transaction::TxOutcome> {
    use crate::transaction::journal::{self, JournalEntry};
    let (platform, _) = server_platform(server)?;
    let plugins_dir = server.plugins_dir();
    let summary = plan.summary();
    let note = |action: &str, outcome: String, note: Option<String>| {
        let _ = journal::append(
            &plugins_dir,
            &JournalEntry {
                time: chrono::Utc::now(),
                tx_id: plan.tx_id.clone(),
                action: action.into(),
                outcome,
                items: vec![],
                note,
            },
        );
    };

    if let Some(b) = &opts.backup {
        progress(crate::transaction::Progress::Step(format!("mcbackup checkpoint {}", server.backup_slug)));
        match b.checkpoint(&server.backup_slug, &format!("before: {summary}")).await {
            Ok(out) => note("backup", "checkpoint before".into(), Some(out)),
            Err(e) => {
                note("backup", format!("checkpoint failed: {e}"), None);
                return Err(Error::Msg(format!("backup before update failed, nothing changed: {e}")));
            }
        }
    }

    let outcome = crate::transaction::apply(server, &platform, lock, sources, plan, progress.clone()).await?;

    let log_progress = progress.clone();
    let log = move |s: String| log_progress(crate::transaction::Progress::Step(s));
    match crate::control::execute_restart(opts.control.as_ref(), &opts.restart, &summary, &log).await {
        Ok(()) => note(
            "restart",
            if opts.restart == crate::control::RestartPolicy::Never {
                "skipped".into()
            } else {
                "restarted".into()
            },
            None,
        ),
        Err(e) => {
            note("restart", format!("failed: {e}"), Some("jars are in place; restart manually or revert".into()));
            return Err(Error::Msg(format!("updated, but restart failed: {e}")));
        }
    }

    if let (Some(b), false) = (&opts.backup, opts.restart == crate::control::RestartPolicy::Never) {
        progress(crate::transaction::Progress::Step(format!("mcbackup backup {}", server.backup_slug)));
        match b.backup(&server.backup_slug, &format!("after: {summary}")).await {
            Ok(out) => note("backup", "snapshot after".into(), Some(out)),
            Err(e) => note("backup", format!("snapshot after failed: {e}"), None),
        }
    }
    Ok(outcome)
}

/// Install (or upgrade) the McplugBridge plugin into a server: config with a unique port and
/// token, the bundled jar, then a restart according to `restart`.
pub async fn install_bridge(
    server: &Server,
    all_servers: &[Server],
    daemon_port: u16,
    control: &dyn crate::control::ServerControl,
    restart: &crate::control::RestartPolicy,
    progress: crate::transaction::ProgressFn,
) -> Result<()> {
    use crate::control::companion::{self, BridgeConfig};
    if companion::JAR_BYTES.is_empty() {
        return Err(Error::Msg(
            "this mcplug build has no bundled McplugBridge jar (build companion/ with `mvn package` first, or use a release binary)".into(),
        ));
    }
    if !server.access.writable() {
        return Err(Error::Msg(format!("{}: plugin directory is not writable", server.name)));
    }
    let plugins_dir = server.plugins_dir();
    let existing = BridgeConfig::load(&plugins_dir);
    let (port, token) = match &existing {
        Some(c) => (c.port, c.token.clone()),
        None => {
            let used: Vec<u16> = all_servers
                .iter()
                .filter(|s| s.id != server.id)
                .filter_map(|s| BridgeConfig::load(&s.plugins_dir()).map(|c| c.port))
                .collect();
            (companion::free_port(&used), companion::new_token())
        }
    };
    BridgeConfig::write(&plugins_dir, port, &token, daemon_port)?;
    progress(crate::transaction::Progress::Step(format!("bridge config: port {port}")));
    // replace any older bridge jar
    let mut old = Vec::new();
    for e in std::fs::read_dir(&plugins_dir)?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(companion::PLUGIN_NAME) && n.ends_with(".jar") {
            old.push(n);
        }
    }
    let file = format!("{}-{}.jar", companion::PLUGIN_NAME, companion::JAR_VERSION);
    std::fs::write(plugins_dir.join(&file), companion::JAR_BYTES)?;
    for n in &old {
        if n != &file {
            let _ = std::fs::remove_file(plugins_dir.join(n));
        }
    }
    progress(crate::transaction::Progress::Step(format!("installed {file}")));
    let _ = crate::transaction::journal::append(
        &plugins_dir,
        &crate::transaction::journal::JournalEntry {
            time: chrono::Utc::now(),
            tx_id: crate::transaction::new_tx_id(),
            action: "bridge".into(),
            outcome: if existing.is_some() { "upgraded".into() } else { "installed".into() },
            items: vec![],
            note: Some(format!("port {port}")),
        },
    );
    let log_progress = progress.clone();
    let log = move |s: String| log_progress(crate::transaction::Progress::Step(s));
    crate::control::execute_restart(control, restart, "installing the mcplug bridge", &log).await?;
    Ok(())
}
