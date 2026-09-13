//! PaperMC "Fill" v3 API.

use async_trait::async_trait;
use serde::Deserialize;

use super::{BuildInfo, ServerJarProvider};
use crate::server::PlatformKind;
use crate::sources::http::Api;
use crate::util::McVersion;
use crate::Result;

const BASE: &str = "https://fill.papermc.io/v3/projects/paper";

pub struct Paper {
    pub api: Api,
}

#[derive(Deserialize)]
struct Project {
    versions: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Build {
    id: u32,
    time: Option<chrono::DateTime<chrono::Utc>>,
    channel: String,
    downloads: std::collections::HashMap<String, Download>,
}

#[derive(Deserialize)]
struct Download {
    name: String,
    url: String,
    checksums: Checksums,
}

#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}

#[derive(Deserialize)]
struct VersionInfo {
    java: Option<Java>,
}

#[derive(Deserialize)]
struct Java {
    version: Option<JavaVersion>,
}

#[derive(Deserialize)]
struct JavaVersion {
    minimum: Option<u32>,
}

#[async_trait]
impl ServerJarProvider for Paper {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Paper
    }

    async fn mc_versions(&self) -> Result<Vec<McVersion>> {
        let p: Project = self.api.get_json(BASE, &[]).await?;
        let mut v: Vec<McVersion> = p
            .versions
            .values()
            .flatten()
            .filter_map(|s| McVersion::parse(s))
            .filter(|v| v.is_release())
            .collect();
        v.sort();
        v.reverse();
        Ok(v)
    }

    async fn builds(&self, mc: &McVersion) -> Result<Vec<BuildInfo>> {
        let info: VersionInfo = self
            .api
            .get_json(&format!("{BASE}/versions/{mc}"), &[])
            .await
            .unwrap_or(VersionInfo { java: None });
        let java_min = info.java.and_then(|j| j.version).and_then(|v| v.minimum);
        let builds: Vec<Build> = self.api.get_json(&format!("{BASE}/versions/{mc}/builds"), &[]).await?;
        let mut out: Vec<BuildInfo> = builds
            .into_iter()
            .filter_map(|b| {
                let d = b.downloads.get("server:default")?;
                Some(BuildInfo {
                    mc: mc.clone(),
                    build: b.id,
                    file_name: d.name.clone(),
                    url: d.url.clone(),
                    sha256: Some(d.checksums.sha256.clone()),
                    md5: None,
                    java_min,
                    channel: b.channel.to_ascii_lowercase(),
                    time: b.time,
                })
            })
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.build));
        Ok(out)
    }
}
