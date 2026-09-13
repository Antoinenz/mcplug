//! `plugins/.mcplug/lock.toml` — what mcplug knows about each plugin. Lives with the server
//! so it survives reinstalling mcplug and moving the server to another host.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::plugins::JarHashes;
use crate::server::PlatformKind;
use crate::util::McVersion;
use crate::Result;

pub const DIR: &str = ".mcplug";
pub const FILE: &str = "lock.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    pub version: u32,
    pub server: LockServer,
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockServer {
    pub platform: Option<PlatformKind>,
    pub mc_version: Option<McVersion>,
    pub last_scan: Option<chrono::DateTime<chrono::Utc>>,
    pub jar: Option<LockServerJar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockServerJar {
    pub provider: String,
    pub file: String,
    pub build: Option<u32>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// Descriptor name — the identity key. Filenames change; this doesn't.
    pub name: String,
    pub file: String,
    pub descriptor_version: Option<String>,
    pub hashes: JarHashes,
    #[serde(default)]
    pub channel: Channel,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub ignored_versions: Vec<String>,
    #[serde(default)]
    pub compat: CompatMode,
    pub installed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub source: SourceRef,
}

/// Minimum release channel the user accepts for this plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Release,
    Beta,
    Alpha,
}

impl Channel {
    pub fn accepts(self, v: Channel) -> bool {
        v <= self
    }
}

/// How strictly a source's declared game versions are matched against the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompatMode {
    /// Use the server-wide default (strict).
    #[default]
    Inherit,
    /// The version must list the exact server version.
    Strict,
    /// Same line is enough (`26.2.x` for a `26.2` server).
    Lenient,
    /// Ignore declared versions entirely.
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourceRef {
    Modrinth { project_id: String, version_id: String, version_number: String },
    Hangar { slug: String, version_name: String, platform: String },
    GitHub { owner: String, repo: String, asset_glob: String, tag: String, asset_name: String },
    #[serde(rename = "geysermc")]
    GeyserMc { project: String, download: String, build: Option<u32> },
    /// Scan couldn't decide. The UI keeps offering candidates.
    Unidentified,
    /// User said "leave it alone". Stays quiet until the jar's hash changes.
    Unmanaged,
}

impl SourceRef {
    pub fn is_managed(&self) -> bool {
        !matches!(self, SourceRef::Unidentified | SourceRef::Unmanaged)
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            SourceRef::Modrinth { .. } => "modrinth",
            SourceRef::Hangar { .. } => "hangar",
            SourceRef::GitHub { .. } => "github",
            SourceRef::GeyserMc { .. } => "geysermc",
            SourceRef::Unidentified => "?",
            SourceRef::Unmanaged => "unmanaged",
        }
    }
}

impl LockFile {
    pub fn path_for(plugins_dir: &Path) -> PathBuf {
        plugins_dir.join(DIR).join(FILE)
    }

    pub fn new() -> Self {
        Self { version: 1, server: LockServer::default(), plugins: Vec::new() }
    }

    pub fn load(plugins_dir: &Path) -> Result<Option<Self>> {
        let path = Self::path_for(plugins_dir);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(Some(toml::from_str(&text)?))
    }

    pub fn load_or_new(plugins_dir: &Path) -> Result<Self> {
        Ok(Self::load(plugins_dir)?.unwrap_or_else(Self::new))
    }

    /// Atomic write: tmp file + rename, previous version kept as `lock.toml.bak`.
    pub fn save(&self, plugins_dir: &Path) -> Result<()> {
        let path = Self::path_for(plugins_dir);
        std::fs::create_dir_all(path.parent().expect("lock dir"))?;
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        if path.exists() {
            let _ = std::fs::copy(&path, path.with_extension("toml.bak"));
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&PluginEntry> {
        self.plugins.iter().find(|p| p.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut PluginEntry> {
        self.plugins.iter_mut().find(|p| p.name == name)
    }

    pub fn by_sha512(&self, sha512: &str) -> Option<&PluginEntry> {
        self.plugins.iter().find(|p| p.hashes.sha512 == sha512)
    }
}

impl Default for LockFile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempdir();
        let mut lock = LockFile::new();
        lock.server.mc_version = McVersion::parse("26.2");
        lock.server.platform = Some(PlatformKind::Paper);
        lock.plugins.push(PluginEntry {
            name: "LuckPerms".into(),
            file: "LuckPerms-Bukkit-5.5.71.jar".into(),
            descriptor_version: Some("5.5.71".into()),
            hashes: JarHashes::of_bytes(b"x"),
            channel: Channel::Release,
            pinned: false,
            ignored_versions: vec![],
            compat: CompatMode::Inherit,
            installed_at: None,
            source: SourceRef::Modrinth { project_id: "Vebnzrzj".into(), version_id: "abc".into(), version_number: "v5.5.71-bukkit".into() },
        });
        lock.plugins.push(PluginEntry {
            name: "LockIn".into(),
            file: "LockIn.jar".into(),
            descriptor_version: Some("1.0".into()),
            hashes: JarHashes::of_bytes(b"y"),
            channel: Channel::Release,
            pinned: false,
            ignored_versions: vec![],
            compat: CompatMode::Inherit,
            installed_at: None,
            source: SourceRef::Unmanaged,
        });
        lock.save(&dir).unwrap();
        lock.save(&dir).unwrap(); // second save creates the .bak
        let back = LockFile::load(&dir).unwrap().unwrap();
        assert_eq!(back.plugins.len(), 2);
        assert_eq!(back.get("LuckPerms").unwrap().source.kind_str(), "modrinth");
        assert!(!back.get("LockIn").unwrap().source.is_managed());
        assert_eq!(back.server.mc_version.unwrap().to_string(), "26.2");
        assert!(LockFile::path_for(&dir).with_extension("toml.bak").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcplug-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
