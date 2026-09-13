//! PurpurMC v2 API.

use async_trait::async_trait;
use serde::Deserialize;

use super::{BuildInfo, ServerJarProvider};
use crate::server::PlatformKind;
use crate::sources::http::Api;
use crate::util::McVersion;
use crate::Result;

const BASE: &str = "https://api.purpurmc.org/v2/purpur";

pub struct Purpur {
    pub api: Api,
}

#[derive(Deserialize)]
struct Project {
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct Version {
    builds: Builds,
}

#[derive(Deserialize)]
struct Builds {
    all: Vec<String>,
}

#[derive(Deserialize)]
struct Build {
    build: String,
    md5: Option<String>,
    timestamp: Option<i64>,
    result: Option<String>,
}

#[async_trait]
impl ServerJarProvider for Purpur {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Purpur
    }

    async fn mc_versions(&self) -> Result<Vec<McVersion>> {
        let p: Project = self.api.get_json(BASE, &[]).await?;
        let mut v: Vec<McVersion> = p.versions.iter().filter_map(|s| McVersion::parse(s)).collect();
        v.sort();
        v.reverse();
        Ok(v)
    }

    async fn builds(&self, mc: &McVersion) -> Result<Vec<BuildInfo>> {
        let v: Version = self.api.get_json(&format!("{BASE}/{mc}"), &[]).await?;
        // Purpur has no bulk build metadata; fetch the newest few (builds are numbered, newest last).
        let mut ids: Vec<u32> = v.builds.all.iter().filter_map(|b| b.parse().ok()).collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        let mut out = Vec::new();
        for id in ids.into_iter().take(15) {
            let b: Build = match self.api.get_json(&format!("{BASE}/{mc}/{id}"), &[]).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            if b.result.as_deref() == Some("FAILURE") {
                continue;
            }
            out.push(BuildInfo {
                mc: mc.clone(),
                build: b.build.parse().unwrap_or(id),
                file_name: format!("purpur-{mc}-{id}.jar"),
                url: format!("{BASE}/{mc}/{id}/download"),
                sha256: None,
                md5: b.md5,
                java_min: None,
                channel: "default".into(),
                time: b.timestamp.and_then(|t| chrono::DateTime::from_timestamp_millis(t)),
            });
        }
        Ok(out)
    }
}
