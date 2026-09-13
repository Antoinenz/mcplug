//! Hangar (PaperMC's plugin repository). No reverse hash lookup, but every version lists
//! its file's sha256, so a name-search candidate can be confirmed against the installed jar.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::http::Api;
use super::model::*;
use super::Source;
use crate::lockfile::Channel;
use crate::plugins::JarHashes;
use crate::Result;

const V1: &str = "https://hangar.papermc.io/api/v1";

pub struct Hangar {
    api: Api,
}

impl Hangar {
    pub fn new(http: reqwest::Client) -> Self {
        Self { api: Api::new(http, 120, None) }
    }
}

#[async_trait]
impl Source for Hangar {
    fn kind(&self) -> SourceKind {
        SourceKind::Hangar
    }

    fn http(&self) -> reqwest::Client {
        self.api.http.clone()
    }

    fn parse_url(&self, url: &url::Url) -> Option<ProjectLocator> {
        if url.host_str()? != "hangar.papermc.io" {
            return None;
        }
        let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
        if segs.len() < 2 || matches!(segs[0], "api" | "staff" | "tools" | "guidelines") {
            return None;
        }
        Some(ProjectLocator { source: SourceKind::Hangar, id: segs[1].to_string() })
    }

    async fn identify_by_hashes(&self, _hashes: &[JarHashes]) -> Result<HashMap<String, (ProjectRef, ResolvedVersion)>> {
        Ok(HashMap::new())
    }

    async fn search(&self, query: &str, ctx: &CompatCtx, hashes: Option<&JarHashes>) -> Result<Vec<Candidate>> {
        let raw: HgList<HgProject> = self.api.get_json(&format!("{V1}/projects"), &[("q", query.to_string()), ("limit", "10".into())]).await?;
        let platform = ctx.hangar_platform.clone().unwrap_or_else(|| "PAPER".into());
        let q = query.to_ascii_lowercase();
        let mut out = Vec::new();
        for p in raw.result {
            if !p.supported_platforms.as_ref().is_none_or(|m| m.contains_key(&platform)) {
                continue;
            }
            let mut confidence = if p.name.to_ascii_lowercase() == q { Confidence::ExactName } else if p.name.to_ascii_lowercase().contains(&q) { Confidence::NameMatch } else { Confidence::Weak };
            let project: ProjectRef = p.into();
            let mut version = None;
            // Only the strong candidates are worth a second request to confirm by hash.
            if let (Some(h), true) = (hashes, confidence >= Confidence::NameMatch) {
                if let Ok(versions) = self.versions(&project.id, ctx).await {
                    if let Some(v) = versions.into_iter().find(|v| v.files.iter().any(|f| f.sha256.as_deref() == Some(&h.sha256))) {
                        confidence = Confidence::HashConfirmed;
                        version = Some(v);
                    }
                }
            }
            out.push(Candidate { project, confidence, version });
        }
        Ok(out)
    }

    async fn project(&self, id: &str) -> Result<ProjectRef> {
        let raw: HgProject = self.api.get_json(&format!("{V1}/projects/{id}"), &[]).await?;
        Ok(raw.into())
    }

    async fn versions(&self, project_id: &str, ctx: &CompatCtx) -> Result<Vec<ResolvedVersion>> {
        let platform = ctx.hangar_platform.clone().unwrap_or_else(|| "PAPER".into());
        let raw: HgList<HgVersion> = self
            .api
            .get_json(&format!("{V1}/projects/{project_id}/versions"), &[("limit", "25".into()), ("platform", platform.clone())])
            .await?;
        let mut out = Vec::new();
        for v in raw.result {
            let Some(dl) = v.downloads.get(&platform) else { continue };
            let url = dl.download_url.clone().or_else(|| dl.external_url.clone());
            let Some(url) = url else { continue };
            let deps = v.plugin_dependencies.get(&platform).cloned().unwrap_or_default();
            out.push(ResolvedVersion {
                source: SourceKind::Hangar,
                project_id: project_id.to_string(),
                version_id: v.name.clone(),
                version_number: v.name,
                channel: channel_from_name(&v.channel.name),
                game_versions: v.platform_dependencies.get(&platform).cloned().unwrap_or_default(),
                loaders: vec![platform.to_ascii_lowercase()],
                published: v.created_at,
                files: vec![VersionFile {
                    name: dl.file_info.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| format!("{project_id}.jar")),
                    url,
                    size: dl.file_info.as_ref().and_then(|f| f.size_bytes),
                    sha512: None,
                    sha256: dl.file_info.as_ref().and_then(|f| f.sha256_hash.clone()),
                    sha1: None,
                    primary: true,
                }],
                dependencies: deps
                    .into_iter()
                    .map(|d| Dependency {
                        project_id: d.namespace.as_ref().map(|n| n.slug.clone()),
                        version_id: None,
                        name: Some(d.name),
                        kind: if d.required { DependencyKind::Required } else { DependencyKind::Optional },
                    })
                    .collect(),
                changelog: v.description,
            });
        }
        out.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(out)
    }
}

fn channel_from_name(name: &str) -> Channel {
    let n = name.to_ascii_lowercase();
    if n.contains("release") || n == "stable" {
        Channel::Release
    } else if n.contains("beta") || n.contains("snapshot") || n.contains("dev") {
        Channel::Beta
    } else {
        Channel::Alpha
    }
}

#[derive(Deserialize)]
struct HgList<T> {
    result: Vec<T>,
}

#[derive(Deserialize)]
struct HgNamespace {
    owner: String,
    slug: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HgProject {
    name: String,
    namespace: HgNamespace,
    description: Option<String>,
    stats: Option<HgStats>,
    avatar_url: Option<String>,
    supported_platforms: Option<HashMap<String, Vec<String>>>,
}

#[derive(Deserialize)]
struct HgStats {
    downloads: Option<u64>,
}

impl From<HgProject> for ProjectRef {
    fn from(p: HgProject) -> Self {
        ProjectRef {
            source: SourceKind::Hangar,
            page_url: format!("https://hangar.papermc.io/{}/{}", p.namespace.owner, p.namespace.slug),
            id: p.namespace.slug.clone(),
            slug: p.namespace.slug,
            name: p.name,
            author: Some(p.namespace.owner),
            description: p.description.unwrap_or_default(),
            downloads: p.stats.and_then(|s| s.downloads),
            icon_url: p.avatar_url,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HgVersion {
    name: String,
    created_at: chrono::DateTime<chrono::Utc>,
    channel: HgChannel,
    description: Option<String>,
    #[serde(default)]
    downloads: HashMap<String, HgDownload>,
    #[serde(default)]
    platform_dependencies: HashMap<String, Vec<String>>,
    #[serde(default)]
    plugin_dependencies: HashMap<String, Vec<HgDep>>,
}

#[derive(Deserialize)]
struct HgChannel {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HgDownload {
    download_url: Option<String>,
    external_url: Option<String>,
    file_info: Option<HgFileInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HgFileInfo {
    name: String,
    size_bytes: Option<u64>,
    sha256_hash: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HgDep {
    name: String,
    required: bool,
    namespace: Option<HgNamespaceOpt>,
}

#[derive(Deserialize, Clone)]
struct HgNamespaceOpt {
    slug: String,
}
