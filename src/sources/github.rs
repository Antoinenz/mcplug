//! GitHub releases. No hashes, no game-version metadata: the user supplies the repo (and an
//! asset glob when a release ships several jars); versions are tags.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::http::Api;
use super::model::*;
use super::Source;
use crate::lockfile::Channel;
use crate::plugins::JarHashes;
use crate::Result;

const API: &str = "https://api.github.com";

pub struct GitHub {
    api: Api,
}

impl GitHub {
    pub fn new(http: reqwest::Client, token: Option<String>) -> Self {
        let auth = token.filter(|t| !t.is_empty()).map(|t| format!("Bearer {t}"));
        Self { api: Api::new(http, 30, auth) }
    }

    /// Only jar assets matching `glob` (default `*.jar`) are offered as files.
    pub fn filter_assets(v: &mut ResolvedVersion, glob: &str) {
        v.files.retain(|f| f.name.ends_with(".jar") && glob_match::glob_match(glob, &f.name));
        if let Some(first) = v.files.first_mut() {
            first.primary = true;
        }
    }
}

#[async_trait]
impl Source for GitHub {
    fn kind(&self) -> SourceKind {
        SourceKind::GitHub
    }

    fn http(&self) -> reqwest::Client {
        self.api.http.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn parse_url(&self, url: &url::Url) -> Option<ProjectLocator> {
        if url.host_str()? != "github.com" {
            return None;
        }
        let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
        if segs.len() < 2 {
            return None;
        }
        Some(ProjectLocator { source: SourceKind::GitHub, id: format!("{}/{}", segs[0], segs[1].trim_end_matches(".git")) })
    }

    async fn identify_by_hashes(&self, _hashes: &[JarHashes]) -> Result<HashMap<String, (ProjectRef, ResolvedVersion)>> {
        Ok(HashMap::new())
    }

    async fn search(&self, query: &str, _ctx: &CompatCtx, _hashes: Option<&JarHashes>) -> Result<Vec<Candidate>> {
        // Accept "owner/repo" typed into the search box; general code search isn't useful here.
        if query.matches('/').count() == 1 && !query.contains(' ') {
            if let Ok(p) = self.project(query).await {
                return Ok(vec![Candidate { project: p, confidence: Confidence::ExactName, version: None }]);
            }
        }
        Ok(vec![])
    }

    async fn project(&self, id: &str) -> Result<ProjectRef> {
        let raw: GhRepo = self.api.get_json(&format!("{API}/repos/{id}"), &[]).await?;
        Ok(ProjectRef {
            source: SourceKind::GitHub,
            id: raw.full_name.clone(),
            slug: raw.name,
            name: raw.full_name.clone(),
            author: Some(raw.owner.login),
            page_url: raw.html_url,
            description: raw.description.unwrap_or_default(),
            downloads: None,
            icon_url: None,
        })
    }

    async fn versions(&self, project_id: &str, _ctx: &CompatCtx) -> Result<Vec<ResolvedVersion>> {
        let raw: Vec<GhRelease> = self.api.get_json(&format!("{API}/repos/{project_id}/releases"), &[("per_page", "20".into())]).await?;
        let mut out: Vec<ResolvedVersion> = raw
            .into_iter()
            .filter(|r| !r.draft)
            .map(|r| ResolvedVersion {
                source: SourceKind::GitHub,
                project_id: project_id.to_string(),
                version_id: r.tag_name.clone(),
                version_number: r.name.filter(|n| !n.is_empty()).unwrap_or_else(|| r.tag_name.clone()),
                channel: if r.prerelease { Channel::Beta } else { Channel::Release },
                game_versions: vec![],
                loaders: vec![],
                published: r.published_at.unwrap_or_default(),
                files: r
                    .assets
                    .into_iter()
                    .map(|a| VersionFile { name: a.name, url: a.browser_download_url, size: a.size, sha512: None, sha256: None, sha1: None, primary: false })
                    .collect(),
                dependencies: vec![],
                changelog: r.body,
            })
            .collect();
        for v in &mut out {
            Self::filter_assets(v, "*.jar");
        }
        out.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(out)
    }
}

#[derive(Deserialize)]
struct GhRepo {
    name: String,
    full_name: String,
    html_url: String,
    description: Option<String>,
    owner: GhOwner,
}

#[derive(Deserialize)]
struct GhOwner {
    login: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    draft: bool,
    prerelease: bool,
    published_at: Option<chrono::DateTime<chrono::Utc>>,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}
