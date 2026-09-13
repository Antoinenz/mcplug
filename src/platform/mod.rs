//! Per-platform knowledge: where plugins live, how to read their descriptors, which
//! loaders/platform names to use when talking to plugin sources.

pub mod bukkit;

use std::io::{Read, Seek};

use crate::server::PlatformKind;
use crate::util::McVersion;

/// What a plugin says about itself (`plugin.yml`, `paper-plugin.yml`, …).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: Option<String>,
    /// Bukkit `api-version`; the minimum server version the plugin targets.
    pub api_version: Option<String>,
    pub authors: Vec<String>,
    pub website: Option<String>,
    pub depend: Vec<String>,
    pub softdepend: Vec<String>,
}

pub trait Platform: Send + Sync {
    fn kind(&self) -> PlatformKind;
    fn plugin_dir_name(&self) -> &'static str;
    fn read_descriptor<R: Read + Seek>(&self, zip: &mut zip::ZipArchive<R>) -> Option<PluginDescriptor>
    where
        Self: Sized;
    /// Modrinth loader names, most specific first.
    fn modrinth_loaders(&self) -> &'static [&'static str];
    fn hangar_platform(&self) -> Option<&'static str>;
    /// Subdirectories of the plugin dir that are never scanned.
    fn ignored_subdirs(&self) -> &'static [&'static str];
    /// Does the descriptor claim compatibility with this server version?
    fn descriptor_compat(&self, d: &PluginDescriptor, mc: &McVersion) -> DescriptorCompat;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorCompat {
    /// Declared minimum is at or below the server version.
    Ok,
    /// Declared minimum is newer than the server.
    TooNew,
    Unknown,
}

pub fn for_kind(kind: PlatformKind) -> Option<bukkit::Bukkit> {
    kind.is_bukkit().then(|| bukkit::Bukkit { kind })
}
