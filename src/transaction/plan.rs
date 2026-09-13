use std::collections::HashSet;

use crate::lockfile::{Channel, CompatMode, LockFile, PluginEntry, SourceRef};
use crate::server::Server;
use crate::sources::resolve::pick;
use crate::sources::{CompatCtx, DependencyKind, ProjectLocator, ProjectRef, ResolvedVersion, SourceKind, Sources, VersionFile};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct PlanItem {
    /// Plugin name (descriptor name for updates; project name for fresh installs).
    pub name: String,
    pub project: ProjectRef,
    /// `None` for a fresh install.
    pub from: Option<PluginEntry>,
    pub to: ResolvedVersion,
    pub file: VersionFile,
    /// Pulled in as a required dependency of another item.
    pub dependency_of: Option<String>,
    pub unverified: bool,
}

#[derive(Debug, Clone)]
pub struct UpdatePlan {
    pub tx_id: String,
    pub items: Vec<PlanItem>,
    /// Required dependencies we couldn't resolve to an installable version.
    pub unresolved_deps: Vec<String>,
    pub notes: Vec<String>,
}

impl UpdatePlan {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn summary(&self) -> String {
        self.items
            .iter()
            .map(|i| match &i.from {
                Some(f) => format!("{} {}→{}", i.name, installed_label(f), i.to.version_number),
                None => format!("+{} {}", i.name, i.to.version_number),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What the user asked for.
#[derive(Debug, Clone)]
pub enum PlanRequest {
    /// Update an installed plugin to the newest acceptable version.
    UpdateLatest { name: String },
    /// Update (or downgrade) an installed plugin to a specific version id.
    UpdateTo { name: String, version_id: String },
    /// Install a project that isn't installed yet.
    Install { locator: ProjectLocator, version_id: Option<String> },
}

pub fn installed_label(e: &PluginEntry) -> String {
    match &e.source {
        SourceRef::Modrinth { version_number, .. } => version_number.clone(),
        SourceRef::Hangar { version_name, .. } => version_name.clone(),
        SourceRef::GitHub { tag, .. } => tag.clone(),
        SourceRef::GeyserMc { build, .. } => build.map(|b| format!("build {b}")).unwrap_or_else(|| "?".into()),
        _ => e.descriptor_version.clone().unwrap_or_else(|| "?".into()),
    }
}

fn entry_locator(e: &PluginEntry) -> Option<ProjectLocator> {
    Some(match &e.source {
        SourceRef::Modrinth { project_id, .. } => ProjectLocator { source: SourceKind::Modrinth, id: project_id.clone() },
        SourceRef::Hangar { slug, .. } => ProjectLocator { source: SourceKind::Hangar, id: slug.clone() },
        SourceRef::GitHub { owner, repo, .. } => ProjectLocator { source: SourceKind::GitHub, id: format!("{owner}/{repo}") },
        SourceRef::GeyserMc { project, .. } => ProjectLocator { source: SourceKind::GeyserMc, id: project.clone() },
        _ => return None,
    })
}

fn installed_project_ids(lock: &LockFile) -> HashSet<(SourceKind, String)> {
    lock.plugins.iter().filter_map(entry_locator).map(|l| (l.source, l.id)).collect()
}

/// Turn requests into a concrete, verified-as-far-as-possible plan. Required Modrinth/Hangar
/// dependencies that aren't installed are added automatically (marked `dependency_of`).
pub async fn build_plan(server: &Server, lock: &LockFile, sources: &Sources, ctx: &CompatCtx, requests: Vec<PlanRequest>, allow_unverified: bool) -> Result<UpdatePlan> {
    let _ = server;
    let mut plan = UpdatePlan { tx_id: super::new_tx_id(), items: Vec::new(), unresolved_deps: Vec::new(), notes: Vec::new() };
    let mut installed = installed_project_ids(lock);
    let mut queued: HashSet<(SourceKind, String)> = HashSet::new();
    let mut queue: Vec<(PlanRequest, Option<String>)> = requests.into_iter().map(|r| (r, None)).collect();

    while let Some((req, dep_of)) = queue.pop() {
        let (locator, from, want_version, name) = match &req {
            PlanRequest::UpdateLatest { name } | PlanRequest::UpdateTo { name, .. } => {
                let e = lock.get(name).ok_or_else(|| Error::Msg(format!("{name}: not in lockfile (scan first)")))?;
                let loc = entry_locator(e).ok_or_else(|| Error::Msg(format!("{name}: no source — identify it first")))?;
                let want = match &req {
                    PlanRequest::UpdateTo { version_id, .. } => Some(version_id.clone()),
                    _ => None,
                };
                (loc, Some(e.clone()), want, name.clone())
            }
            PlanRequest::Install { locator, version_id } => (locator.clone(), None, version_id.clone(), String::new()),
        };
        let src = sources.get(locator.source).ok_or_else(|| Error::Msg(format!("source {} is disabled", locator.source)))?;
        let project = src.project(&locator.id).await?;
        let name = if name.is_empty() { project.name.clone() } else { name };
        if from.is_none() && installed.contains(&(locator.source, locator.id.clone())) {
            plan.notes.push(format!("{name}: already installed"));
            continue;
        }
        let entry_ctx = match &from {
            Some(e) => ctx.with_mode(e.compat, CompatMode::Strict),
            None => ctx.clone(),
        };
        let versions = src.versions(&locator.id, &entry_ctx).await?;
        let chosen = match &want_version {
            Some(id) => versions.iter().find(|v| &v.version_id == id).cloned().ok_or_else(|| Error::Msg(format!("{name}: version {id} not found")))?,
            None => {
                let tmp_entry = from.clone().unwrap_or_else(|| fresh_entry(&name));
                pick(&versions, &tmp_entry, &entry_ctx).map(|(v, _)| v).ok_or_else(|| Error::Msg(format!("{name}: no version compatible with {}", ctx.mc_version)))?
            }
        };
        let mut file = chosen.primary_file().cloned().ok_or_else(|| Error::Msg(format!("{name}: version {} has no downloadable file", chosen.version_number)))?;
        if let Some(SourceRef::GitHub { asset_glob, .. }) = from.as_ref().map(|e| &e.source) {
            let mut v = chosen.clone();
            crate::sources::github::GitHub::filter_assets(&mut v, asset_glob);
            file = v.primary_file().cloned().ok_or_else(|| Error::Msg(format!("{name}: no asset matches {asset_glob}")))?;
        }
        let unverified = file.sha512.is_none() && file.sha256.is_none() && file.sha1.is_none();
        if unverified && !allow_unverified {
            return Err(Error::Msg(format!("{name}: {} publishes no checksum; pass --allow-unverified to accept", locator.source)));
        }
        if let Some(e) = &from {
            if e.hashes.sha512.as_str() == file.sha512.as_deref().unwrap_or("") || Some(e.hashes.sha256.as_str()) == file.sha256.as_deref() {
                plan.notes.push(format!("{name}: {} is already installed", chosen.version_number));
                continue;
            }
        }
        // dependencies (Modrinth ids, Hangar slugs)
        for d in &chosen.dependencies {
            if d.kind != DependencyKind::Required {
                continue;
            }
            let Some(pid) = &d.project_id else { continue };
            let key = (locator.source, pid.clone());
            if installed.contains(&key) || queued.contains(&key) {
                continue;
            }
            queued.insert(key);
            queue.push((PlanRequest::Install { locator: ProjectLocator { source: locator.source, id: pid.clone() }, version_id: None }, Some(name.clone())));
        }
        installed.insert((locator.source, locator.id.clone()));
        plan.items.push(PlanItem { name, project, from, to: chosen, file, dependency_of: dep_of, unverified });
    }
    // dependencies first, so a missing dep is caught before its dependent is swapped in
    plan.items.sort_by_key(|i| i.dependency_of.is_none());
    Ok(plan)
}

fn fresh_entry(name: &str) -> PluginEntry {
    PluginEntry {
        name: name.to_string(),
        file: String::new(),
        descriptor_version: None,
        hashes: crate::plugins::JarHashes::of_bytes(b""),
        channel: Channel::Release,
        pinned: false,
        ignored_versions: vec![],
        compat: CompatMode::Inherit,
        installed_at: None,
        source: SourceRef::Unidentified,
    }
}
