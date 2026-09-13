//! Work out which project each installed jar is.
//!
//! Order: lockfile (already known, hash unchanged) → Modrinth bulk hash lookup → GeyserMC hash
//! lookup → name search on every source (Hangar confirms by sha256 where it can). Anything not
//! hash-confirmed is returned as candidates for the user to accept.

use std::collections::HashMap;

use super::model::*;
use super::Sources;
use crate::lockfile::{LockFile, PluginEntry, SourceRef};
use crate::plugins::ScannedJar;

#[derive(Debug)]
pub struct Identified {
    pub jar: ScannedJar,
    pub project: ProjectRef,
    pub version: ResolvedVersion,
}

#[derive(Debug)]
pub struct Undecided {
    pub jar: ScannedJar,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Default)]
pub struct IdentifyReport {
    /// Jars whose lock entry still matches by hash. Nothing to do.
    pub unchanged: Vec<ScannedJar>,
    /// Newly identified by hash (or an accepted exact-name match).
    pub identified: Vec<Identified>,
    /// Needs a human: candidates sorted by confidence, best first.
    pub undecided: Vec<Undecided>,
    /// Jars without a usable descriptor (not plugins for this platform).
    pub skipped: Vec<ScannedJar>,
    pub errors: Vec<String>,
}

pub struct IdentifyOptions {
    /// Accept a single exact-name candidate without asking.
    pub accept_exact_name: bool,
}

pub async fn identify(sources: &Sources, lock: &LockFile, jars: Vec<ScannedJar>, ctx: &CompatCtx, opts: &IdentifyOptions) -> IdentifyReport {
    let mut report = IdentifyReport::default();
    let mut pending: Vec<ScannedJar> = Vec::new();
    for jar in jars {
        if jar.descriptor.is_none() {
            report.skipped.push(jar);
            continue;
        }
        match lock.by_sha512(&jar.hashes.sha512) {
            // Still unidentified: search again, the user may have a decision to make.
            Some(e) if matches!(e.source, SourceRef::Unidentified) => pending.push(jar),
            // Known and untouched — including ones the user marked unmanaged.
            Some(_) => report.unchanged.push(jar),
            None => pending.push(jar),
        }
    }
    if pending.is_empty() {
        return report;
    }

    // 1. hash lookups, bulk, on the sources that support them
    let hashes: Vec<_> = pending.iter().map(|j| j.hashes.clone()).collect();
    let mut by_hash: HashMap<String, (ProjectRef, ResolvedVersion)> = HashMap::new();
    for src in &sources.list {
        match src.identify_by_hashes(&hashes).await {
            Ok(m) => {
                for (h, v) in m {
                    by_hash.entry(h).or_insert(v);
                }
            }
            Err(e) => report.errors.push(format!("{}: {e}", src.kind())),
        }
    }
    let mut still: Vec<ScannedJar> = Vec::new();
    for jar in pending {
        match by_hash.remove(&jar.hashes.sha512) {
            Some((project, version)) => report.identified.push(Identified { jar, project, version }),
            None => still.push(jar),
        }
    }

    // 2. name search for the rest
    for jar in still {
        let name = jar.descriptor.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| jar.file.clone());
        let mut candidates: Vec<Candidate> = Vec::new();
        for src in &sources.list {
            match src.search(&name, ctx, Some(&jar.hashes)).await {
                Ok(c) => candidates.extend(c),
                Err(e) => report.errors.push(format!("{} search {name}: {e}", src.kind())),
            }
        }
        candidates.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(b.project.downloads.cmp(&a.project.downloads)));
        candidates.truncate(8);
        let best = candidates.first();
        let hash_confirmed = best.filter(|c| c.confidence == Confidence::HashConfirmed && c.version.is_some());
        let exact_single = best.filter(|c| {
            opts.accept_exact_name && c.confidence == Confidence::ExactName && candidates.iter().filter(|x| x.confidence == Confidence::ExactName).count() == 1
        });
        if let Some(c) = hash_confirmed {
            report.identified.push(Identified { jar, project: c.project.clone(), version: c.version.clone().expect("hash confirmed has version") });
        } else if let Some(c) = exact_single {
            // Exact name but the installed build isn't a published file: record the project
            // with a placeholder version; the first "check" will find the newest.
            let placeholder = placeholder_version(&c.project);
            report.identified.push(Identified { jar, project: c.project.clone(), version: placeholder });
        } else {
            report.undecided.push(Undecided { jar, candidates });
        }
    }
    report
}

fn placeholder_version(p: &ProjectRef) -> ResolvedVersion {
    ResolvedVersion {
        source: p.source,
        project_id: p.id.clone(),
        version_id: String::new(),
        version_number: "unknown".into(),
        channel: crate::lockfile::Channel::Release,
        game_versions: vec![],
        loaders: vec![],
        published: chrono::DateTime::<chrono::Utc>::default(),
        files: vec![],
        dependencies: vec![],
        changelog: None,
    }
}

/// Build the lock entry for an identification.
pub fn entry_for(id: &Identified) -> PluginEntry {
    let d = id.jar.descriptor.as_ref().expect("identified jars have descriptors");
    PluginEntry {
        name: d.name.clone(),
        file: id.jar.file.clone(),
        descriptor_version: d.version.clone(),
        hashes: id.jar.hashes.clone(),
        channel: id.version.channel.max(crate::lockfile::Channel::Release),
        pinned: false,
        ignored_versions: vec![],
        compat: crate::lockfile::CompatMode::Inherit,
        installed_at: id.jar.modified.map(chrono::DateTime::<chrono::Utc>::from),
        source: source_ref(&id.project, &id.version),
    }
}

pub fn source_ref(p: &ProjectRef, v: &ResolvedVersion) -> SourceRef {
    match p.source {
        SourceKind::Modrinth => SourceRef::Modrinth { project_id: p.id.clone(), version_id: v.version_id.clone(), version_number: v.version_number.clone() },
        SourceKind::Hangar => SourceRef::Hangar { slug: p.id.clone(), version_name: v.version_id.clone(), platform: v.loaders.first().cloned().unwrap_or_else(|| "paper".into()).to_ascii_uppercase() },
        SourceKind::GitHub => {
            let (owner, repo) = p.id.split_once('/').unwrap_or((&p.id, ""));
            SourceRef::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
                asset_glob: "*.jar".into(),
                tag: v.version_id.clone(),
                asset_name: v.primary_file().map(|f| f.name.clone()).unwrap_or_default(),
            }
        }
        SourceKind::GeyserMc => SourceRef::GeyserMc { project: p.id.clone(), download: v.loaders.first().cloned().unwrap_or_else(|| "spigot".into()), build: v.version_id.parse().ok() },
    }
}

/// Unidentified entry so the lock still records the jar (and stops re-hashing it as "new").
pub fn unidentified_entry(jar: &ScannedJar) -> PluginEntry {
    let d = jar.descriptor.as_ref().expect("descriptor");
    PluginEntry {
        name: d.name.clone(),
        file: jar.file.clone(),
        descriptor_version: d.version.clone(),
        hashes: jar.hashes.clone(),
        channel: crate::lockfile::Channel::Release,
        pinned: false,
        ignored_versions: vec![],
        compat: crate::lockfile::CompatMode::Inherit,
        installed_at: jar.modified.map(chrono::DateTime::<chrono::Utc>::from),
        source: SourceRef::Unidentified,
    }
}
