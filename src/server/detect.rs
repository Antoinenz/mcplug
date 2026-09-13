//! Identify the server software and Minecraft version from the server jar.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::util::McVersion;
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformKind {
    Paper,
    Purpur,
    Folia,
    Pufferfish,
    Spigot,
    Vanilla,
    Fabric,
    Velocity,
    Unknown,
}

impl PlatformKind {
    /// Bukkit-API servers share the `plugins/` + `plugin.yml` model.
    pub fn is_bukkit(self) -> bool {
        matches!(
            self,
            Self::Paper | Self::Purpur | Self::Folia | Self::Pufferfish | Self::Spigot
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Purpur => "purpur",
            Self::Folia => "folia",
            Self::Pufferfish => "pufferfish",
            Self::Spigot => "spigot",
            Self::Vanilla => "vanilla",
            Self::Fabric => "fabric",
            Self::Velocity => "velocity",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for PlatformKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// What we could read out of the server jar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerJarInfo {
    pub file_name: String,
    pub platform: PlatformKind,
    pub mc_version: Option<McVersion>,
    /// From `version.json` — the Java the bundled Minecraft build wants.
    pub java_min: Option<u32>,
    pub sha256: String,
    pub size: u64,
    /// Build number when derivable from the filename (`paper-26.2-123.jar` → 123).
    pub build_hint: Option<u32>,
}

#[derive(Deserialize)]
struct VersionJson {
    id: Option<String>,
    java_version: Option<u32>,
}

pub fn inspect_server_jar(path: &Path) -> Result<ServerJarInfo> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let size = bytes.len() as u64;
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)?;

    let version_json: Option<VersionJson> =
        read_entry(&mut zip, "version.json").and_then(|s| serde_json::from_str(&s).ok());
    let manifest = read_entry(&mut zip, "META-INF/MANIFEST.MF").unwrap_or_default();
    let main_class = manifest
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let versions_list = read_entry(&mut zip, "META-INF/versions.list").unwrap_or_default();
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let platform = detect_platform(&main_class, &versions_list, &names, &file_name);
    let mc_version = version_json
        .as_ref()
        .and_then(|v| v.id.as_deref())
        .and_then(McVersion::parse);
    let java_min = version_json.as_ref().and_then(|v| v.java_version);

    Ok(ServerJarInfo {
        build_hint: build_from_filename(&file_name),
        file_name,
        platform,
        mc_version,
        java_min,
        sha256,
        size,
    })
}

fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn detect_platform(main_class: &str, versions_list: &str, names: &[String], file_name: &str) -> PlatformKind {
    // Brand is in the bundled server jar name inside a Paperclip jar
    // (`META-INF/versions.list` → `26.2/paper-26.2.jar`, `purpur-26.2.jar`, `folia-…`).
    let hay = format!(
        "{}\n{}\n{}",
        versions_list.to_ascii_lowercase(),
        names.iter().filter(|n| n.starts_with("META-INF/versions/")).cloned().collect::<Vec<_>>().join("\n").to_ascii_lowercase(),
        file_name.to_ascii_lowercase()
    );
    let brand = |s: &str| hay.contains(s);
    if main_class.starts_with("io.papermc.paperclip") {
        if brand("purpur") {
            return PlatformKind::Purpur;
        }
        if brand("folia") {
            return PlatformKind::Folia;
        }
        if brand("pufferfish") {
            return PlatformKind::Pufferfish;
        }
        return PlatformKind::Paper;
    }
    if main_class.starts_with("com.velocitypowered") {
        return PlatformKind::Velocity;
    }
    if main_class.starts_with("net.fabricmc") || names.iter().any(|n| n == "fabric-server-launch.properties") {
        return PlatformKind::Fabric;
    }
    if main_class.starts_with("org.bukkit.craftbukkit") || main_class.starts_with("org.spigotmc") {
        return PlatformKind::Spigot;
    }
    if main_class.starts_with("net.minecraft") {
        return PlatformKind::Vanilla;
    }
    PlatformKind::Unknown
}

/// `paper-26.2-123.jar` → 123, `purpur-1.21.11-2450.jar` → 2450.
fn build_from_filename(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".jar")?;
    let last = stem.rsplit('-').next()?;
    let n: u32 = last.parse().ok()?;
    // Needs a version segment before it, otherwise "-123.jar" alone is not a build.
    (stem.matches('-').count() >= 2).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_detection() {
        let names = vec!["META-INF/versions/26.2/purpur-26.2.jar".to_string()];
        assert_eq!(detect_platform("io.papermc.paperclip.Main", "", &names, "server.jar"), PlatformKind::Purpur);
        assert_eq!(detect_platform("io.papermc.paperclip.Main", "abc\t26.2/paper-26.2.jar\t26.2", &[], "x.jar"), PlatformKind::Paper);
        assert_eq!(detect_platform("com.velocitypowered.proxy.Velocity", "", &[], "velocity.jar"), PlatformKind::Velocity);
        assert_eq!(detect_platform("org.bukkit.craftbukkit.Main", "", &[], "spigot.jar"), PlatformKind::Spigot);
        assert_eq!(detect_platform("net.minecraft.bundler.Main", "", &[], "server.jar"), PlatformKind::Vanilla);
    }

    #[test]
    fn build_numbers() {
        assert_eq!(build_from_filename("paper-26.2-123.jar"), Some(123));
        assert_eq!(build_from_filename("purpur-1.21.11-2450.jar"), Some(2450));
        assert_eq!(build_from_filename("paper-1.21.1-37.jar"), Some(37));
        assert_eq!(build_from_filename("server.jar"), None);
        assert_eq!(build_from_filename("pufferfish-paperclip-1.21.8-R0.1-SNAPSHOT-mojmap.jar"), None);
    }
}
