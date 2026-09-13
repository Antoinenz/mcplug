//! Walk a server's plugin directory and describe every jar in it.

use std::path::{Path, PathBuf};

use super::hash::JarHashes;
use crate::platform::{Platform, PluginDescriptor};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScannedJar {
    pub file: String,
    pub path: PathBuf,
    pub hashes: JarHashes,
    /// `None` when the jar has no descriptor for this platform (a Fabric mod dropped into
    /// `plugins/`, a library jar, a corrupt file).
    pub descriptor: Option<PluginDescriptor>,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ScanResult {
    pub jars: Vec<ScannedJar>,
    /// Jars sitting in Bukkit's `update/` folder — someone else has staged an update.
    pub staged_updates: Vec<String>,
    /// Count of jars in `disabled/` (user convention).
    pub disabled: usize,
    /// Descriptor names that appear on more than one jar.
    pub duplicates: Vec<String>,
    pub errors: Vec<String>,
}

pub fn scan_plugins<P: Platform>(platform: &P, plugins_dir: &Path) -> ScanResult {
    let mut result = ScanResult::default();
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(plugins_dir) {
        Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
        Err(e) => {
            result.errors.push(format!("{}: {e}", plugins_dir.display()));
            return result;
        }
    };
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "update") {
                result.staged_updates = jars_in(&path);
            } else if path.file_name().is_some_and(|n| n == "disabled") {
                result.disabled = jars_in(&path).len();
            }
            continue;
        }
        if path.extension().is_none_or(|e| e != "jar") {
            continue;
        }
        match scan_one(platform, &path) {
            Ok(j) => result.jars.push(j),
            Err(e) => result.errors.push(format!("{}: {e}", path.display())),
        }
    }
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for j in &result.jars {
        if let Some(d) = &j.descriptor {
            *seen.entry(d.name.clone()).or_default() += 1;
        }
    }
    result.duplicates = seen.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect();
    result.duplicates.sort();
    result
}

fn scan_one<P: Platform>(platform: &P, path: &Path) -> std::io::Result<ScannedJar> {
    let hashes = JarHashes::of_file(path)?;
    let descriptor = std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .and_then(|mut z| platform.read_descriptor(&mut z));
    Ok(ScannedJar {
        file: path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        path: path.to_path_buf(),
        hashes,
        descriptor,
        modified: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
    })
}

fn jars_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.ends_with(".jar").then_some(n)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}
