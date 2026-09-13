//! GeyserMC's download API for Geyser and floodgate. Builds are frequent and not on a
//! semver train, so identification is by sha256 against the recent build list.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::http::Api;
use super::model::*;
use super::Source;
use crate::lockfile::Channel;
use crate::plugins::JarHashes;
use crate::Result;

const BASE: &str = "https://download.geysermc.org/v2";
const PROJECTS: &[&str] = &["geyser", "floodgate"];

pub struct GeyserMc {
    api: Api,
}

impl GeyserMc {
    pub fn new(http: reqwest::Client) -> Self {
        Self { api: Api::new(http, 60, None) }
    }

    async fn builds(&self, project: &str) -> Result<(String, Vec<GBuild>)> {
        let raw: GBuilds = self.api.get_json(&format!("{BASE}/projects/{project}/versions/latest/builds"), &[]).await?;
        Ok((raw.version, raw.builds))
    }

    fn to_version(project: &str, version: &str, b: &GBuild, download: &str) -> Option<ResolvedVersion> {
        let d = b.downloads.get(download)?;
        Some(ResolvedVersion {
            source: SourceKind::GeyserMc,
            project_id: project.to_string(),
            version_id: b.build.to_string(),
            version_number: format!("{version} build {}", b.build),
            channel: if b.channel.as_deref() == Some("experimental") {
                Channel::Beta
            } else {
                Channel::Release
            },
            game_versions: vec![],
            loaders: vec![download.to_string()],
            published: b.time,
            files: vec![VersionFile {
                name: d.name.clone(),
                url: format!("{BASE}/projects/{project}/versions/{version}/builds/{}/downloads/{download}", b.build),
                size: None,
                sha512: None,
                sha256: Some(d.sha256.clone()),
                sha1: None,
                primary: true,
            }],
            dependencies: vec![],
            changelog: Some(b.changes.iter().map(|c| format!("- {}", c.summary)).collect::<Vec<_>>().join("\n")),
        })
    }

    fn project_ref(project: &str) -> ProjectRef {
        let (name, desc) = match project {
            "geyser" => ("Geyser", "Lets Bedrock Edition players join Java servers"),
            _ => ("Floodgate", "Lets Bedrock players join without a Java account"),
        };
        ProjectRef {
            source: SourceKind::GeyserMc,
            id: project.to_string(),
            slug: project.to_string(),
            name: name.to_string(),
            author: Some("GeyserMC".into()),
            page_url: "https://geysermc.org/download".into(),
            description: desc.to_string(),
            downloads: None,
            icon_url: None,
        }
    }
}

#[async_trait]
impl Source for GeyserMc {
    fn kind(&self) -> SourceKind {
        SourceKind::GeyserMc
    }

    fn http(&self) -> reqwest::Client {
        self.api.http.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn parse_url(&self, url: &url::Url) -> Option<ProjectLocator> {
        let host = url.host_str()?;
        if !host.ends_with("geysermc.org") {
            return None;
        }
        let path = url.path().to_ascii_lowercase();
        let id = if path.contains("floodgate") { "floodgate" } else { "geyser" };
        Some(ProjectLocator {
            source: SourceKind::GeyserMc,
            id: id.into(),
        })
    }

    async fn identify_by_hashes(&self, hashes: &[JarHashes]) -> Result<HashMap<String, (ProjectRef, ResolvedVersion)>> {
        let mut out = HashMap::new();
        if hashes.is_empty() {
            return Ok(out);
        }
        for project in PROJECTS {
            let (version, builds) = self.builds(project).await?;
            for b in &builds {
                for (dl, d) in &b.downloads {
                    if let Some(h) = hashes.iter().find(|h| h.sha256 == d.sha256) {
                        if let Some(v) = Self::to_version(project, &version, b, dl) {
                            out.insert(h.sha512.clone(), (Self::project_ref(project), v));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    async fn search(&self, query: &str, _ctx: &CompatCtx, _hashes: Option<&JarHashes>) -> Result<Vec<Candidate>> {
        let q = query.to_ascii_lowercase();
        Ok(PROJECTS
            .iter()
            .filter(|p| p.contains(&q) || q.contains(*p))
            .map(|p| Candidate {
                project: Self::project_ref(p),
                confidence: Confidence::NameMatch,
                version: None,
            })
            .collect())
    }

    async fn project(&self, id: &str) -> Result<ProjectRef> {
        Ok(Self::project_ref(id))
    }

    async fn versions(&self, project_id: &str, ctx: &CompatCtx) -> Result<Vec<ResolvedVersion>> {
        let (version, builds) = self.builds(project_id).await?;
        let download = if ctx.loaders.iter().any(|l| l == "velocity") { "velocity" } else { "spigot" };
        let mut out: Vec<ResolvedVersion> = builds.iter().filter_map(|b| Self::to_version(project_id, &version, b, download)).collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.published));
        Ok(out)
    }
}

#[derive(Deserialize)]
struct GBuilds {
    version: String,
    builds: Vec<GBuild>,
}

#[derive(Deserialize)]
struct GBuild {
    build: u32,
    time: chrono::DateTime<chrono::Utc>,
    channel: Option<String>,
    #[serde(default)]
    changes: Vec<GChange>,
    #[serde(default)]
    downloads: HashMap<String, GDownload>,
}

#[derive(Deserialize)]
struct GChange {
    summary: String,
}

#[derive(Deserialize)]
struct GDownload {
    name: String,
    sha256: String,
}
