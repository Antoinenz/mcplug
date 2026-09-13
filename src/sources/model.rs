//! Source-agnostic view of projects and versions.

use serde::{Deserialize, Serialize};

use crate::lockfile::{Channel, CompatMode};
use crate::util::McVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Modrinth,
    Hangar,
    GitHub,
    #[serde(rename = "geysermc")]
    GeyserMc,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::Hangar => "hangar",
            Self::GitHub => "github",
            Self::GeyserMc => "geysermc",
        }
    }
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRef {
    pub source: SourceKind,
    /// Modrinth project id, Hangar `owner/slug`, GitHub `owner/repo`, GeyserMC project name.
    pub id: String,
    pub slug: String,
    pub name: String,
    pub author: Option<String>,
    pub page_url: String,
    pub description: String,
    pub downloads: Option<u64>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
    pub sha512: Option<String>,
    pub sha256: Option<String>,
    pub sha1: Option<String>,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVersion {
    pub source: SourceKind,
    pub project_id: String,
    /// Modrinth version id | Hangar version name | GitHub tag | GeyserMC build number.
    pub version_id: String,
    pub version_number: String,
    pub channel: Channel,
    /// Empty when the source doesn't know (GitHub).
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub published: chrono::DateTime<chrono::Utc>,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<Dependency>,
    pub changelog: Option<String>,
}

impl ResolvedVersion {
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or_else(|| self.files.first())
    }

    pub fn compat(&self, ctx: &CompatCtx) -> Compat {
        if self.game_versions.is_empty() {
            return Compat::Unknown;
        }
        let exact = self.game_versions.iter().any(|g| g == &ctx.mc_version.to_string());
        if exact {
            return Compat::Exact;
        }
        let same_line = self.game_versions.iter().filter_map(|g| McVersion::parse(g)).any(|g| g.same_line(&ctx.mc_version));
        match ctx.mode {
            CompatMode::Any => Compat::Lenient,
            CompatMode::Lenient | CompatMode::Inherit if same_line => Compat::Lenient,
            _ if same_line => Compat::SameLineOnly,
            _ => Compat::Incompatible,
        }
    }
}

/// What the server is, for filtering versions.
#[derive(Debug, Clone)]
pub struct CompatCtx {
    pub mc_version: McVersion,
    pub loaders: Vec<String>,
    pub hangar_platform: Option<String>,
    /// Effective mode after resolving `Inherit` against the server default.
    pub mode: CompatMode,
}

impl CompatCtx {
    pub fn with_mode(&self, plugin_mode: CompatMode, server_default: CompatMode) -> CompatCtx {
        let mode = match plugin_mode {
            CompatMode::Inherit => match server_default {
                CompatMode::Inherit => CompatMode::Strict,
                m => m,
            },
            m => m,
        };
        CompatCtx { mode, ..self.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compat {
    /// Lists the exact server version.
    Exact,
    /// Same line and the mode allows it.
    Lenient,
    /// Same line but the mode is strict — shown, not auto-applied.
    SameLineOnly,
    /// Source doesn't declare game versions.
    Unknown,
    Incompatible,
}

impl Compat {
    pub fn acceptable(self) -> bool {
        matches!(self, Compat::Exact | Compat::Lenient)
    }
}

/// A search hit or identification candidate.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub project: ProjectRef,
    pub confidence: Confidence,
    /// The version the installed jar matches, if hash-confirmed.
    pub version: Option<ResolvedVersion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Weak,
    NameMatch,
    ExactName,
    HashConfirmed,
}

/// Where a pasted URL points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLocator {
    pub source: SourceKind,
    pub id: String,
}
