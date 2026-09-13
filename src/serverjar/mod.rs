//! Server software: identify the installed build, offer newer builds of the same Minecraft
//! version, and upgrade to a new Minecraft version deliberately.

pub mod ops;
pub mod paper;
pub mod purpur;

use async_trait::async_trait;

use crate::server::PlatformKind;
use crate::util::McVersion;
use crate::Result;

#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub mc: McVersion,
    pub build: u32,
    pub file_name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub md5: Option<String>,
    pub java_min: Option<u32>,
    pub channel: String,
    pub time: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait]
pub trait ServerJarProvider: Send + Sync {
    fn kind(&self) -> PlatformKind;
    /// Release versions, newest first.
    async fn mc_versions(&self) -> Result<Vec<McVersion>>;
    /// Builds for one version, newest first.
    async fn builds(&self, mc: &McVersion) -> Result<Vec<BuildInfo>>;
    async fn latest_build(&self, mc: &McVersion) -> Result<BuildInfo> {
        self.builds(mc)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| crate::Error::Msg(format!("no builds for {mc}")))
    }
}

pub fn for_platform(http: reqwest::Client, kind: PlatformKind) -> Option<Box<dyn ServerJarProvider>> {
    match kind {
        PlatformKind::Paper => Some(Box::new(paper::Paper {
            api: crate::sources::http::Api::new(http, 300, None),
        })),
        PlatformKind::Purpur => Some(Box::new(purpur::Purpur {
            api: crate::sources::http::Api::new(http, 300, None),
        })),
        _ => None,
    }
}

/// Find the installed build by hash among the provider's builds for that version.
pub async fn identify_build(provider: &dyn ServerJarProvider, mc: &McVersion, sha256: &str, md5: Option<&str>) -> Result<Option<BuildInfo>> {
    let builds = provider.builds(mc).await?;
    Ok(builds
        .into_iter()
        .find(|b| b.sha256.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(sha256)) || (md5.is_some() && b.md5.as_deref() == md5)))
}
