//! Decide, for every managed plugin, which version it *should* be on.

use std::collections::HashMap;

use super::model::*;
use super::modrinth::Modrinth;
use super::Sources;
use crate::lockfile::{CompatMode, LockFile, PluginEntry, SourceRef};

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCandidate {
    pub name: String,
    pub installed: String,
    pub latest: ResolvedVersion,
    pub compat: String,
    /// Same line only / unknown game versions — shown but not auto-applied.
    pub untested: bool,
    pub unverified: bool,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct CheckReport {
    pub updates: Vec<UpdateCandidate>,
    pub up_to_date: Vec<String>,
    pub pinned: Vec<String>,
    pub unmanaged: Vec<String>,
    pub errors: Vec<String>,
}

/// Effective compat mode for an entry.
fn mode_for(entry: &PluginEntry, server_default: CompatMode) -> CompatMode {
    match entry.compat {
        CompatMode::Inherit => match server_default {
            CompatMode::Inherit => CompatMode::Strict,
            m => m,
        },
        m => m,
    }
}

fn installed_version_id(entry: &PluginEntry) -> Option<String> {
    match &entry.source {
        SourceRef::Modrinth { version_id, .. } => Some(version_id.clone()),
        SourceRef::Hangar { version_name, .. } => Some(version_name.clone()),
        SourceRef::GitHub { tag, .. } => Some(tag.clone()),
        SourceRef::GeyserMc { build, .. } => build.map(|b| b.to_string()),
        _ => None,
    }
}

fn installed_label(entry: &PluginEntry) -> String {
    match &entry.source {
        SourceRef::Modrinth { version_number, .. } => version_number.clone(),
        SourceRef::Hangar { version_name, .. } => version_name.clone(),
        SourceRef::GitHub { tag, .. } => tag.clone(),
        SourceRef::GeyserMc { build, .. } => build.map(|b| format!("build {b}")).unwrap_or_else(|| "?".into()),
        _ => entry.descriptor_version.clone().unwrap_or_else(|| "?".into()),
    }
}

fn project_id(entry: &PluginEntry) -> Option<String> {
    match &entry.source {
        SourceRef::Modrinth { project_id, .. } => Some(project_id.clone()),
        SourceRef::Hangar { slug, .. } => Some(slug.clone()),
        SourceRef::GitHub { owner, repo, .. } => Some(format!("{owner}/{repo}")),
        SourceRef::GeyserMc { project, .. } => Some(project.clone()),
        _ => None,
    }
}

fn source_kind(entry: &PluginEntry) -> Option<SourceKind> {
    match &entry.source {
        SourceRef::Modrinth { .. } => Some(SourceKind::Modrinth),
        SourceRef::Hangar { .. } => Some(SourceKind::Hangar),
        SourceRef::GitHub { .. } => Some(SourceKind::GitHub),
        SourceRef::GeyserMc { .. } => Some(SourceKind::GeyserMc),
        _ => None,
    }
}

/// Is `latest` actually a different release than what's installed? Sources publish the same
/// version number as separate files per loader (paper/purpur/folia), so a differing id with an
/// identical version number is a sibling, not an update.
fn is_newer(entry: &PluginEntry, latest: &ResolvedVersion) -> bool {
    if Some(&latest.version_id) == installed_version_id(entry).as_ref() {
        return false;
    }
    let installed = installed_label(entry);
    installed == "unknown" || installed == "?" || installed != latest.version_number
}

/// Pick the newest version that passes channel/ignore/compat rules. Among versions published
/// together for several loaders, the one listing the server's most specific loader wins.
pub fn pick(versions: &[ResolvedVersion], entry: &PluginEntry, ctx: &CompatCtx) -> Option<(ResolvedVersion, Compat)> {
    let mut fallback: Option<(ResolvedVersion, Compat)> = None;
    let mut versions: Vec<&ResolvedVersion> = versions.iter().collect();
    let rank = |v: &ResolvedVersion| ctx.loaders.iter().position(|l| v.loaders.contains(l)).unwrap_or(usize::MAX);
    versions.sort_by(|a, b| b.published.date_naive().cmp(&a.published.date_naive()).then(rank(a).cmp(&rank(b))));
    for v in versions {
        if !entry.channel.accepts(v.channel) || entry.ignored_versions.contains(&v.version_id) {
            continue;
        }
        if let SourceRef::GitHub { asset_glob, .. } = &entry.source {
            let mut v2 = v.clone();
            super::github::GitHub::filter_assets(&mut v2, asset_glob);
            if v2.files.is_empty() {
                continue;
            }
        }
        match v.compat(ctx) {
            c @ (Compat::Exact | Compat::Lenient) => return Some((v.clone(), c)),
            c @ (Compat::SameLineOnly | Compat::Unknown) if fallback.is_none() => fallback = Some((v.clone(), c)),
            _ => {}
        }
    }
    fallback
}

pub async fn check(sources: &Sources, lock: &LockFile, base_ctx: &CompatCtx, server_default: CompatMode) -> CheckReport {
    let mut report = CheckReport::default();
    let mut needs_list: Vec<&PluginEntry> = Vec::new();

    // Modrinth first, in bulk, for strict-mode entries: one request for the whole server.
    let mut bulk: HashMap<String, ResolvedVersion> = HashMap::new();
    if let Some(m) = sources.get(SourceKind::Modrinth) {
        let strict_hashes: Vec<String> = lock
            .plugins
            .iter()
            .filter(|e| matches!(e.source, SourceRef::Modrinth { .. }) && !e.pinned && mode_for(e, server_default) == CompatMode::Strict)
            .map(|e| e.hashes.sha512.clone())
            .collect();
        // Modrinth::latest_for_hashes is a concrete method, so downcast through the registry.
        if let Some(mr) = modrinth_ref(m) {
            match mr.latest_for_hashes(&strict_hashes, base_ctx).await {
                Ok(map) => bulk = map,
                Err(e) => report.errors.push(format!("modrinth bulk check: {e}")),
            }
        }
    }

    for entry in &lock.plugins {
        if !entry.source.is_managed() {
            report.unmanaged.push(entry.name.clone());
            continue;
        }
        if entry.pinned {
            report.pinned.push(entry.name.clone());
            continue;
        }
        let ctx = base_ctx.with_mode(entry.compat, server_default);
        if let Some(latest) = bulk.get(&entry.hashes.sha512) {
            let ok = entry.channel.accepts(latest.channel) && !entry.ignored_versions.contains(&latest.version_id);
            if ok {
                if is_newer(entry, latest) {
                    report.updates.push(candidate(entry, latest.clone(), latest.compat(&ctx)));
                } else {
                    report.up_to_date.push(entry.name.clone());
                }
                continue;
            }
        }
        needs_list.push(entry);
    }

    for entry in needs_list {
        let (Some(kind), Some(pid)) = (source_kind(entry), project_id(entry)) else { continue };
        let Some(src) = sources.get(kind) else {
            report.errors.push(format!("{}: source {kind} is disabled", entry.name));
            continue;
        };
        let ctx = base_ctx.with_mode(entry.compat, server_default);
        match src.versions(&pid, &ctx).await {
            Ok(versions) => match pick(&versions, entry, &ctx) {
                Some((latest, compat)) if is_newer(entry, &latest) => {
                    report.updates.push(candidate(entry, latest, compat));
                }
                Some(_) => report.up_to_date.push(entry.name.clone()),
                None => report.up_to_date.push(entry.name.clone()),
            },
            Err(e) => report.errors.push(format!("{}: {e}", entry.name)),
        }
    }
    report.updates.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    report
}

fn candidate(entry: &PluginEntry, latest: ResolvedVersion, compat: Compat) -> UpdateCandidate {
    UpdateCandidate {
        name: entry.name.clone(),
        installed: installed_label(entry),
        unverified: latest.primary_file().is_some_and(|f| f.sha512.is_none() && f.sha256.is_none()),
        untested: !compat.acceptable(),
        compat: format!("{compat:?}").to_ascii_lowercase(),
        latest,
    }
}

fn modrinth_ref(s: &dyn super::Source) -> Option<&Modrinth> {
    s.as_any().downcast_ref::<Modrinth>()
}
