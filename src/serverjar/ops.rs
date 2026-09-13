//! Status, build updates and Minecraft-version upgrades for the server jar.

use std::path::Path;

use super::{identify_build, BuildInfo, ServerJarProvider};
use crate::lockfile::LockFile;
use crate::plugins::JarHashes;
use crate::server::start_command::StartCommand;
use crate::server::Server;
use crate::sources::{Compat, CompatCtx, Sources};
use crate::transaction::journal::{self, JournalEntry, JournalItem};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct JarStatus {
    pub file: String,
    pub mc: crate::util::McVersion,
    pub installed: Option<BuildInfo>,
    pub sha256: String,
    pub latest_same_mc: Option<BuildInfo>,
    pub newest_mc: Option<crate::util::McVersion>,
    pub java_major: Option<u32>,
}

pub async fn status(server: &Server, provider: &dyn ServerJarProvider) -> Result<JarStatus> {
    let jar = server.jar.as_ref().ok_or_else(|| Error::Msg("no server jar".into()))?;
    let mc = jar.mc_version.clone().ok_or_else(|| Error::Msg("unknown Minecraft version".into()))?;
    let path = server.jar_path().expect("jar path");
    let hashes = tokio::task::spawn_blocking(move || JarHashes::of_file(&path)).await.map_err(|e| Error::Msg(e.to_string()))??;
    let md5 = md5_of(&server.jar_path().expect("jar path"));
    let installed = identify_build(provider, &mc, &hashes.sha256, md5.as_deref()).await?;
    let latest_same_mc = provider.latest_build(&mc).await.ok();
    let newest_mc = provider.mc_versions().await.ok().and_then(|v| v.into_iter().next());
    let java_major = match server.start_command.as_deref().map(StartCommand::parse).and_then(|c| c.java().map(str::to_string)) {
        Some(j) => crate::server::java::java_major(&j, Some(&server.root)).await,
        None => None,
    };
    Ok(JarStatus { file: jar.file_name.clone(), mc, installed, sha256: hashes.sha256, latest_same_mc, newest_mc, java_major })
}

fn md5_of(path: &Path) -> Option<String> {
    // Only Purpur publishes md5; compute lazily and cheaply enough (single pass).
    let data = std::fs::read(path).ok()?;
    use md5::Digest;
    Some(hex::encode(md5::Md5::digest(data)))
}

#[derive(Debug, Clone)]
pub struct PluginCompat {
    pub name: String,
    pub current: String,
    pub best: Option<String>,
    pub compat: Compat,
}

/// For an MC upgrade: which version of each managed plugin would run on `target`.
pub async fn plugin_compat(lock: &LockFile, sources: &Sources, base_ctx: &CompatCtx, target: &crate::util::McVersion) -> Vec<PluginCompat> {
    let ctx = CompatCtx { mc_version: target.clone(), ..base_ctx.clone() };
    let mut out = Vec::new();
    for e in &lock.plugins {
        if !e.source.is_managed() {
            continue;
        }
        let (kind, pid) = match &e.source {
            crate::lockfile::SourceRef::Modrinth { project_id, .. } => (crate::sources::SourceKind::Modrinth, project_id.clone()),
            crate::lockfile::SourceRef::Hangar { slug, .. } => (crate::sources::SourceKind::Hangar, slug.clone()),
            crate::lockfile::SourceRef::GitHub { owner, repo, .. } => (crate::sources::SourceKind::GitHub, format!("{owner}/{repo}")),
            crate::lockfile::SourceRef::GeyserMc { project, .. } => (crate::sources::SourceKind::GeyserMc, project.clone()),
            _ => continue,
        };
        let current = crate::transaction::plan::installed_label(e);
        let Some(src) = sources.get(kind) else { continue };
        let versions = src.versions(&pid, &ctx).await.unwrap_or_default();
        // versions are newest-first; keep the newest one in the best compatibility class
        let rank = |c: &Compat| match c {
            Compat::Exact => 3,
            Compat::Lenient | Compat::SameLineOnly => 2,
            Compat::Unknown => 1,
            Compat::Incompatible => 0,
        };
        let mut best: Option<(&crate::sources::ResolvedVersion, Compat)> = None;
        for v in &versions {
            let c = v.compat(&ctx);
            if best.as_ref().is_none_or(|(_, bc)| rank(&c) > rank(bc)) {
                best = Some((v, c));
            }
        }
        out.push(PluginCompat { name: e.name.clone(), current, best: best.map(|(v, _)| v.version_number.clone()), compat: best.map(|(_, c)| c).unwrap_or(Compat::Incompatible) });
    }
    out
}

/// Which JVM the start command uses and whether it satisfies `java_min`.
pub async fn java_check(server: &Server, java_min: Option<u32>) -> Result<Option<String>> {
    let Some(min) = java_min else { return Ok(None) };
    let cmd = server.start_command.as_deref().map(StartCommand::parse);
    let java = cmd.as_ref().and_then(|c| c.java().map(str::to_string)).unwrap_or_else(|| "java".into());
    let have = crate::server::java::java_major(&java, Some(&server.root)).await;
    match have {
        Some(h) if h >= min => Ok(None),
        _ => {
            let alternatives: Vec<String> = crate::server::java::installed_javas().into_iter().map(|p| p.display().to_string()).collect();
            Ok(Some(format!(
                "needs Java {min}, but `{java}` is Java {}. {}",
                have.map(|h| h.to_string()).unwrap_or_else(|| "?".into()),
                if alternatives.is_empty() { "Install a newer JDK and point the start command at it.".into() } else { format!("Installed JVMs: {}", alternatives.join(", ")) }
            )))
        }
    }
}

/// Download `build`, verify, swap it in next to the old jar and update the start command.
pub async fn install_build(server: &Server, control: &dyn crate::control::ServerControl, build: &BuildInfo, http: &reqwest::Client, progress: crate::transaction::ProgressFn) -> Result<String> {
    let plugins_dir = server.plugins_dir();
    let tx = crate::transaction::new_tx_id();
    let staging = crate::transaction::staging_dir(&plugins_dir, &tx);
    std::fs::create_dir_all(&staging)?;
    progress(crate::transaction::Progress::Step(format!("downloading {}", build.file_name)));
    let file = crate::sources::VersionFile { name: build.file_name.clone(), url: build.url.clone(), size: None, sha512: None, sha256: build.sha256.clone(), sha1: None, primary: true };
    let dest = staging.join(&build.file_name);
    let name = build.file_name.clone();
    let p = progress.clone();
    let hashes = crate::sources::download_file(http, &file, &dest, Box::new(move |done, total| p(crate::transaction::Progress::Download { name: name.clone(), done, total }))).await?;
    if let Some(want) = &build.sha256 {
        if !want.eq_ignore_ascii_case(&hashes.sha256) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(Error::Msg("server jar checksum mismatch".into()));
        }
    }
    if let Some(want) = &build.md5 {
        if md5_of(&dest).as_deref() != Some(want.as_str()) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(Error::Msg("server jar md5 mismatch".into()));
        }
    }
    let info = crate::server::detect::inspect_server_jar(&dest)?;
    if info.mc_version.as_ref() != Some(&build.mc) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Error::Msg(format!("downloaded jar reports {:?}, expected {}", info.mc_version, build.mc)));
    }
    // keep the old jar (renamed only if the new one has the same name)
    let target = server.root.join(&build.file_name);
    let old_name = server.jar.as_ref().map(|j| j.file_name.clone());
    let rb = crate::transaction::rollback_dir(&plugins_dir, &tx);
    std::fs::create_dir_all(&rb)?;
    if let Some(old) = &old_name {
        if old == &build.file_name && target.exists() {
            std::fs::rename(&target, rb.join(old))?;
        }
    }
    std::fs::rename(&dest, &target)?;
    let _ = std::fs::remove_dir_all(&staging);
    // start command
    let mut note = None;
    if let Some(cmd) = server.start_command.as_deref().map(StartCommand::parse) {
        if cmd.jar() != Some(build.file_name.as_str()) {
            let new_cmd = cmd.with_jar(&build.file_name);
            match control.set_start_command(&new_cmd).await {
                Ok(()) => note = Some(format!("start command now runs {}", build.file_name)),
                Err(e) => note = Some(format!("could not update the start command ({e}); set it to: {new_cmd}")),
            }
        }
    }
    let _ = journal::append(&plugins_dir, &JournalEntry {
        time: chrono::Utc::now(),
        tx_id: tx.clone(),
        action: "server-jar".into(),
        outcome: "applied".into(),
        items: vec![JournalItem { name: format!("{}", build.mc), from: server.jar.as_ref().and_then(|j| j.build_hint).map(|b| format!("build {b}")), to: format!("build {}", build.build), old_file: old_name, new_file: build.file_name.clone() }],
        note: note.clone(),
    });
    if let Some(n) = note {
        progress(crate::transaction::Progress::Step(n));
    }
    Ok(tx)
}
