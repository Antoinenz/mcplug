use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::journal::{self, JournalEntry, JournalItem};
use super::plan::{installed_label, UpdatePlan};
use super::{rollback_dir, staging_dir};
use crate::lockfile::{LockFile, PluginEntry};
use crate::platform::Platform;
use crate::plugins::JarHashes;
use crate::server::Server;
use crate::sources::identify::source_ref;
use crate::sources::Sources;
use crate::{Error, Result};

/// Progress events for UIs.
pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum Progress {
    Step(String),
    Download { name: String, done: u64, total: Option<u64> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub tx_id: String,
    pub time: chrono::DateTime<chrono::Utc>,
    pub items: Vec<ManifestItem>,
    /// Set once every swap finished; a manifest without it means a crash mid-swap.
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestItem {
    pub name: String,
    pub old_file: Option<String>,
    pub new_file: String,
    pub old_entry: Option<PluginEntry>,
    pub new_entry: PluginEntry,
}

#[derive(Debug, Clone)]
pub struct TxOutcome {
    pub tx_id: String,
    pub applied: Vec<ManifestItem>,
}

fn manifest_path(plugins_dir: &Path, tx: &str) -> PathBuf {
    rollback_dir(plugins_dir, tx).join("manifest.toml")
}

/// Run the plan. On any failure before the swap nothing on the server changes; a failure
/// during the swap is rolled back from the manifest.
pub async fn apply<P: Platform>(server: &Server, platform: &P, lock: &mut LockFile, sources: &Sources, plan: &UpdatePlan, progress: ProgressFn) -> Result<TxOutcome> {
    let plugins_dir = server.plugins_dir();
    if !server.access.writable() {
        return Err(Error::Msg(format!("{}: plugin directory is not writable", server.name)));
    }
    if plan.is_empty() {
        return Err(Error::Msg("nothing to do".into()));
    }
    let staging = staging_dir(&plugins_dir, &plan.tx_id);
    std::fs::create_dir_all(&staging)?;

    // 1. download + verify everything first
    let mut staged: Vec<(usize, PathBuf, JarHashes)> = Vec::new();
    for (i, item) in plan.items.iter().enumerate() {
        progress(Progress::Step(format!("downloading {} {}", item.name, item.to.version_number)));
        let src = sources.get(item.project.source).ok_or_else(|| Error::Msg(format!("source {} disabled", item.project.source)))?;
        let dest = staging.join(&item.file.name);
        let name = item.name.clone();
        let p = progress.clone();
        let hashes = src
            .download(&item.file, &dest, Box::new(move |done, total| p(Progress::Download { name: name.clone(), done, total })))
            .await
            .map_err(|e| abort(&plugins_dir, &staging, &plan.tx_id, format!("{}: download failed: {e}", item.name)))?;
        if let Err(e) = verify(item, &hashes, &dest, platform) {
            return Err(abort(&plugins_dir, &staging, &plan.tx_id, e.to_string()));
        }
        staged.push((i, dest, hashes));
    }

    // 2. write the manifest (intent log) before touching the plugin dir
    progress(Progress::Step("swapping jars".into()));
    let rb = rollback_dir(&plugins_dir, &plan.tx_id);
    std::fs::create_dir_all(&rb)?;
    let mut manifest = Manifest { tx_id: plan.tx_id.clone(), time: chrono::Utc::now(), items: Vec::new(), completed: false };
    for (i, dest, hashes) in &staged {
        let item = &plan.items[*i];
        let new_entry = PluginEntry {
            name: item.name.clone(),
            file: item.file.name.clone(),
            descriptor_version: read_descriptor_version(dest, platform),
            hashes: hashes.clone(),
            channel: item.from.as_ref().map(|f| f.channel).unwrap_or_default(),
            pinned: item.from.as_ref().is_some_and(|f| f.pinned),
            ignored_versions: item.from.as_ref().map(|f| f.ignored_versions.clone()).unwrap_or_default(),
            compat: item.from.as_ref().map(|f| f.compat).unwrap_or_default(),
            installed_at: Some(chrono::Utc::now()),
            source: preserve_github_glob(source_ref(&item.project, &item.to), item.from.as_ref()),
        };
        manifest.items.push(ManifestItem { name: item.name.clone(), old_file: item.from.as_ref().map(|f| f.file.clone()), new_file: item.file.name.clone(), old_entry: item.from.clone(), new_entry });
    }
    write_manifest(&plugins_dir, &manifest)?;

    // 3. swap, rolling back on failure
    let mut done: Vec<usize> = Vec::new();
    for (k, (_, dest, _)) in staged.iter().enumerate() {
        let m = &manifest.items[k];
        let result = (|| -> std::io::Result<()> {
            if let Some(old) = &m.old_file {
                let old_path = plugins_dir.join(old);
                if old_path.exists() {
                    std::fs::rename(&old_path, rb.join(old))?;
                }
            }
            std::fs::rename(dest, plugins_dir.join(&m.new_file))?;
            match_owner(&plugins_dir, &plugins_dir.join(&m.new_file));
            Ok(())
        })();
        match result {
            Ok(()) => done.push(k),
            Err(e) => {
                for &j in done.iter().rev() {
                    undo_swap(&plugins_dir, &rb, &manifest.items[j]);
                }
                let _ = std::fs::remove_dir_all(&staging);
                let _ = journal::append(&plugins_dir, &JournalEntry { time: chrono::Utc::now(), tx_id: plan.tx_id.clone(), action: "aborted".into(), outcome: format!("swap failed for {}: {e}", m.name), items: vec![], note: None });
                return Err(Error::Msg(format!("{}: swap failed: {e} (rolled back)", m.name)));
            }
        }
    }
    manifest.completed = true;
    write_manifest(&plugins_dir, &manifest)?;
    let _ = std::fs::remove_dir_all(&staging);

    // 4. lockfile
    for m in &manifest.items {
        lock.plugins.retain(|p| p.name != m.name);
        lock.plugins.push(m.new_entry.clone());
    }
    lock.plugins.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    lock.save(&plugins_dir)?;

    // 5. journal
    let _ = journal::append(&plugins_dir, &JournalEntry {
        time: chrono::Utc::now(),
        tx_id: plan.tx_id.clone(),
        action: if plan.items.iter().all(|i| i.from.is_none()) { "install".into() } else { "update".into() },
        outcome: "applied".into(),
        items: manifest.items.iter().map(|m| JournalItem { name: m.name.clone(), from: m.old_entry.as_ref().map(installed_label), to: installed_label(&m.new_entry), old_file: m.old_file.clone(), new_file: m.new_file.clone() }).collect(),
        note: None,
    });
    prune_rollbacks(&plugins_dir, 5);
    Ok(TxOutcome { tx_id: plan.tx_id.clone(), applied: manifest.items })
}

/// Clean up a transaction that failed before any swap: nothing on the server changed.
fn abort(plugins_dir: &Path, staging: &Path, tx: &str, why: String) -> Error {
    let _ = std::fs::remove_dir_all(staging);
    let _ = journal::append(plugins_dir, &JournalEntry { time: chrono::Utc::now(), tx_id: tx.to_string(), action: "aborted".into(), outcome: why.clone(), items: vec![], note: Some("nothing was changed".into()) });
    Error::Msg(why)
}

fn verify<P: Platform>(item: &super::PlanItem, got: &JarHashes, path: &Path, platform: &P) -> Result<()> {
    // Test hook: MCPLUG_FAULT=hash makes every checksum mismatch, so the abort path can be exercised.
    if std::env::var("MCPLUG_FAULT").as_deref() == Ok("hash") {
        return Err(Error::Msg(format!("{}: checksum mismatch (injected by MCPLUG_FAULT)", item.name)));
    }
    let expected = [(&item.file.sha512, &got.sha512), (&item.file.sha256, &got.sha256), (&item.file.sha1, &got.sha1)];
    for (want, have) in expected {
        if let Some(w) = want {
            if !w.eq_ignore_ascii_case(have) {
                return Err(Error::Msg(format!("{}: checksum mismatch for {} (expected {}…, got {}…)", item.name, item.file.name, &w[..12], &have[..12])));
            }
        }
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| Error::Msg(format!("{}: downloaded file is not a jar: {e}", item.name)))?;
    let d = platform.read_descriptor(&mut zip).ok_or_else(|| Error::Msg(format!("{}: {} has no plugin descriptor for this platform", item.name, item.file.name)))?;
    if let Some(from) = &item.from {
        if !d.name.eq_ignore_ascii_case(&from.name) {
            return Err(Error::Msg(format!("{}: downloaded jar identifies itself as {:?} — wrong file?", item.name, d.name)));
        }
    }
    Ok(())
}

fn read_descriptor_version<P: Platform>(path: &Path, platform: &P) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(f).ok()?;
    platform.read_descriptor(&mut zip)?.version
}

fn preserve_github_glob(new: crate::lockfile::SourceRef, from: Option<&PluginEntry>) -> crate::lockfile::SourceRef {
    use crate::lockfile::SourceRef;
    match (new, from.map(|f| &f.source)) {
        (SourceRef::GitHub { owner, repo, tag, asset_name, .. }, Some(SourceRef::GitHub { asset_glob, .. })) => SourceRef::GitHub { owner, repo, asset_glob: asset_glob.clone(), tag, asset_name },
        (n, _) => n,
    }
}

fn write_manifest(plugins_dir: &Path, m: &Manifest) -> Result<()> {
    let p = manifest_path(plugins_dir, &m.tx_id);
    std::fs::write(&p, toml::to_string_pretty(m)?)?;
    Ok(())
}

pub fn read_manifest(plugins_dir: &Path, tx: &str) -> Result<Manifest> {
    let text = std::fs::read_to_string(manifest_path(plugins_dir, tx)).map_err(|e| Error::Msg(format!("no rollback data for transaction {tx}: {e}")))?;
    Ok(toml::from_str(&text)?)
}

fn undo_swap(plugins_dir: &Path, rb: &Path, m: &ManifestItem) {
    let new_path = plugins_dir.join(&m.new_file);
    if new_path.exists() {
        let _ = std::fs::remove_file(&new_path);
    }
    if let Some(old) = &m.old_file {
        let saved = rb.join(old);
        if saved.exists() {
            let _ = std::fs::rename(saved, plugins_dir.join(old));
        }
    }
}

/// Put things back the way they were before `tx`.
pub fn revert(server: &Server, lock: &mut LockFile, tx: &str) -> Result<Vec<String>> {
    let plugins_dir = server.plugins_dir();
    let manifest = read_manifest(&plugins_dir, tx)?;
    let rb = rollback_dir(&plugins_dir, tx);
    let mut names = Vec::new();
    for m in manifest.items.iter().rev() {
        undo_swap(&plugins_dir, &rb, m);
        lock.plugins.retain(|p| p.name != m.name);
        if let Some(old) = &m.old_entry {
            lock.plugins.push(old.clone());
        }
        names.push(m.name.clone());
    }
    lock.plugins.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    lock.save(&plugins_dir)?;
    let _ = std::fs::remove_dir_all(&rb);
    let _ = journal::append(&plugins_dir, &JournalEntry {
        time: chrono::Utc::now(),
        tx_id: tx.to_string(),
        action: "revert".into(),
        outcome: "reverted".into(),
        items: manifest.items.iter().map(|m| JournalItem { name: m.name.clone(), from: Some(installed_label(&m.new_entry)), to: m.old_entry.as_ref().map(installed_label).unwrap_or_else(|| "removed".into()), old_file: Some(m.new_file.clone()), new_file: m.old_file.clone().unwrap_or_default() }).collect(),
        note: None,
    });
    Ok(names)
}

/// Transactions whose manifest never got `completed = true` (crash mid-swap).
pub fn incomplete_transactions(plugins_dir: &Path) -> Vec<String> {
    let root = super::mcplug_dir(plugins_dir).join("rollback");
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let tx = e.file_name().to_string_lossy().to_string();
            let m = read_manifest(plugins_dir, &tx).ok()?;
            (!m.completed).then_some(tx)
        })
        .collect();
    out.sort();
    out
}

fn prune_rollbacks(plugins_dir: &Path, keep: usize) {
    let root = super::mcplug_dir(plugins_dir).join("rollback");
    let Ok(rd) = std::fs::read_dir(&root) else { return };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    while dirs.len() > keep {
        let _ = std::fs::remove_dir_all(dirs.remove(0));
    }
}

#[cfg(unix)]
fn match_owner(reference: &Path, target: &Path) {
    use std::os::unix::fs::MetadataExt;
    if let Ok(md) = std::fs::metadata(reference) {
        let _ = std::os::unix::fs::chown(target, Some(md.uid()), Some(md.gid()));
    }
}

#[cfg(not(unix))]
fn match_owner(_reference: &Path, _target: &Path) {}
