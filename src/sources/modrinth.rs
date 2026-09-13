//! Modrinth: v2 for projects/versions/search/hash lookups, v3 for collections.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::http::Api;
use super::model::*;
use super::Source;
use crate::lockfile::Channel;
use crate::plugins::JarHashes;
use crate::Result;

const V2: &str = "https://api.modrinth.com/v2";
const V3: &str = "https://api.modrinth.com/v3";

pub struct Modrinth {
    api: Api,
}

impl Modrinth {
    pub fn new(http: reqwest::Client, token: Option<String>) -> Self {
        Self {
            api: Api::new(http, 200, token.filter(|t| !t.is_empty())),
        }
    }

    fn loaders_facet(ctx: &CompatCtx) -> String {
        let inner: Vec<String> = ctx.loaders.iter().map(|l| format!("\"categories:{l}\"")).collect();
        format!("[[{}],[\"project_type:plugin\"]]", inner.join(","))
    }

    /// Latest compatible version per hash — one round trip for a whole server.
    pub async fn latest_for_hashes(&self, sha512s: &[String], ctx: &CompatCtx) -> Result<HashMap<String, ResolvedVersion>> {
        if sha512s.is_empty() {
            return Ok(HashMap::new());
        }
        let body = serde_json::json!({
            "hashes": sha512s,
            "algorithm": "sha512",
            "loaders": ctx.loaders,
            "game_versions": [ctx.mc_version.to_string()],
        });
        let raw: HashMap<String, MrVersion> = self.api.post_json(&format!("{V2}/version_files/update"), &body).await?;
        Ok(raw.into_iter().map(|(h, v)| (h, v.into())).collect())
    }

    /// The signed-in user's collections (needs a PAT with COLLECTION_READ + USER_READ).
    pub async fn my_collections(&self) -> Result<Vec<Collection>> {
        let me: MrUser = self.api.get_json(&format!("{V3}/user"), &[]).await?;
        let cols: Vec<MrCollection> = self.api.get_json(&format!("{V3}/user/{}/collections", me.id), &[]).await?;
        Ok(cols
            .into_iter()
            .map(|c| Collection {
                id: c.id,
                name: c.name,
                description: c.description,
                project_ids: c.projects,
            })
            .collect())
    }

    pub async fn projects(&self, ids: &[String]) -> Result<Vec<ProjectRef>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let list = serde_json::to_string(ids)?;
        let raw: Vec<MrProject> = self.api.get_json(&format!("{V2}/projects"), &[("ids", list)]).await?;
        Ok(raw.into_iter().map(Into::into).collect())
    }
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub project_ids: Vec<String>,
}

#[async_trait]
impl Source for Modrinth {
    fn kind(&self) -> SourceKind {
        SourceKind::Modrinth
    }

    fn http(&self) -> reqwest::Client {
        self.api.http.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn parse_url(&self, url: &url::Url) -> Option<ProjectLocator> {
        if url.host_str()? != "modrinth.com" {
            return None;
        }
        let mut segs = url.path_segments()?;
        let kind = segs.next()?;
        if !matches!(kind, "plugin" | "mod" | "project" | "datapack") {
            return None;
        }
        Some(ProjectLocator {
            source: SourceKind::Modrinth,
            id: segs.next()?.to_string(),
        })
    }

    async fn identify_by_hashes(&self, hashes: &[JarHashes]) -> Result<HashMap<String, (ProjectRef, ResolvedVersion)>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let sha512s: Vec<&str> = hashes.iter().map(|h| h.sha512.as_str()).collect();
        let body = serde_json::json!({ "hashes": sha512s, "algorithm": "sha512" });
        let raw: HashMap<String, MrVersion> = self.api.post_json(&format!("{V2}/version_files"), &body).await?;
        let ids: Vec<String> = raw
            .values()
            .map(|v| v.project_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let projects: HashMap<String, ProjectRef> = self.projects(&ids).await?.into_iter().map(|p| (p.id.clone(), p)).collect();
        let mut out = HashMap::new();
        for (hash, v) in raw {
            if let Some(p) = projects.get(&v.project_id) {
                out.insert(hash, (p.clone(), v.into()));
            }
        }
        Ok(out)
    }

    async fn search(&self, query: &str, ctx: &CompatCtx, _hashes: Option<&JarHashes>) -> Result<Vec<Candidate>> {
        let raw: MrSearch = self
            .api
            .get_json(
                &format!("{V2}/search"),
                &[("query", query.to_string()), ("facets", Self::loaders_facet(ctx)), ("limit", "12".into())],
            )
            .await?;
        let q = query.to_ascii_lowercase();
        Ok(raw
            .hits
            .into_iter()
            .map(|h| {
                let confidence = if h.title.to_ascii_lowercase() == q || h.slug.to_ascii_lowercase() == q {
                    Confidence::ExactName
                } else if h.title.to_ascii_lowercase().contains(&q) {
                    Confidence::NameMatch
                } else {
                    Confidence::Weak
                };
                Candidate {
                    project: h.into(),
                    confidence,
                    version: None,
                }
            })
            .collect())
    }

    async fn project(&self, id: &str) -> Result<ProjectRef> {
        let raw: MrProject = self.api.get_json(&format!("{V2}/project/{id}"), &[]).await?;
        Ok(raw.into())
    }

    async fn versions(&self, project_id: &str, ctx: &CompatCtx) -> Result<Vec<ResolvedVersion>> {
        let loaders = serde_json::to_string(&ctx.loaders)?;
        let raw: Vec<MrVersion> = self
            .api
            .get_json(&format!("{V2}/project/{project_id}/version"), &[("loaders", loaders)])
            .await?;
        let mut v: Vec<ResolvedVersion> = raw.into_iter().map(Into::into).collect();
        v.sort_by_key(|v| std::cmp::Reverse(v.published));
        Ok(v)
    }
}

// ---- wire types ----

#[derive(Deserialize)]
struct MrUser {
    id: String,
}

#[derive(Deserialize)]
struct MrCollection {
    id: String,
    name: String,
    description: Option<String>,
    #[serde(default)]
    projects: Vec<String>,
}

#[derive(Deserialize)]
struct MrSearch {
    hits: Vec<MrHit>,
}

#[derive(Deserialize)]
struct MrHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    author: Option<String>,
    downloads: Option<u64>,
    icon_url: Option<String>,
}

impl From<MrHit> for ProjectRef {
    fn from(h: MrHit) -> Self {
        ProjectRef {
            source: SourceKind::Modrinth,
            page_url: format!("https://modrinth.com/plugin/{}", h.slug),
            id: h.project_id,
            slug: h.slug,
            name: h.title,
            author: h.author,
            description: h.description,
            downloads: h.downloads,
            icon_url: h.icon_url,
        }
    }
}

#[derive(Deserialize)]
struct MrProject {
    id: String,
    slug: String,
    title: String,
    description: String,
    downloads: Option<u64>,
    icon_url: Option<String>,
    team: Option<String>,
}

impl From<MrProject> for ProjectRef {
    fn from(p: MrProject) -> Self {
        ProjectRef {
            source: SourceKind::Modrinth,
            page_url: format!("https://modrinth.com/plugin/{}", p.slug),
            id: p.id,
            slug: p.slug,
            name: p.title,
            author: p.team,
            description: p.description,
            downloads: p.downloads,
            icon_url: p.icon_url,
        }
    }
}

#[derive(Deserialize)]
struct MrVersion {
    id: String,
    project_id: String,
    version_number: String,
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    date_published: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: Vec<MrDependency>,
    changelog: Option<String>,
}

#[derive(Deserialize)]
struct MrFile {
    filename: String,
    url: String,
    size: Option<u64>,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct MrDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

impl From<MrVersion> for ResolvedVersion {
    fn from(v: MrVersion) -> Self {
        ResolvedVersion {
            source: SourceKind::Modrinth,
            project_id: v.project_id,
            version_id: v.id,
            version_number: v.version_number,
            channel: match v.version_type.as_str() {
                "release" => Channel::Release,
                "beta" => Channel::Beta,
                _ => Channel::Alpha,
            },
            game_versions: v.game_versions,
            loaders: v.loaders,
            published: v.date_published,
            files: v
                .files
                .into_iter()
                .map(|f| VersionFile {
                    name: f.filename,
                    url: f.url,
                    size: f.size,
                    sha512: f.hashes.get("sha512").cloned(),
                    sha256: None,
                    sha1: f.hashes.get("sha1").cloned(),
                    primary: f.primary,
                })
                .collect(),
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| Dependency {
                    project_id: d.project_id,
                    version_id: d.version_id,
                    name: None,
                    kind: match d.dependency_type.as_str() {
                        "required" => DependencyKind::Required,
                        "optional" => DependencyKind::Optional,
                        "incompatible" => DependencyKind::Incompatible,
                        _ => DependencyKind::Embedded,
                    },
                })
                .collect(),
            changelog: v.changelog,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let m = Modrinth::new(reqwest::Client::new(), None);
        let u = url::Url::parse("https://modrinth.com/plugin/luckperms/versions").unwrap();
        assert_eq!(m.parse_url(&u).unwrap().id, "luckperms");
        assert!(m.parse_url(&url::Url::parse("https://modrinth.com/user/lucko").unwrap()).is_none());
        assert!(m.parse_url(&url::Url::parse("https://hangar.papermc.io/x/y").unwrap()).is_none());
    }
}
