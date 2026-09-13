//! Plugin sources: Modrinth, Hangar, GitHub releases, GeyserMC downloads.

pub mod geysermc;
pub mod github;
pub mod hangar;
pub mod http;
pub mod identify;
pub mod model;
pub mod modrinth;

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;

pub use model::*;

use crate::plugins::JarHashes;
use crate::Result;

/// Progress callback for downloads: (bytes so far, total if known).
pub type ProgressSink = Box<dyn Fn(u64, Option<u64>) + Send + Sync>;

#[async_trait]
pub trait Source: Send + Sync {
    fn kind(&self) -> SourceKind;

    /// Recognise a pasted URL for this source.
    fn parse_url(&self, url: &url::Url) -> Option<ProjectLocator>;

    /// Bulk reverse lookup by hash. Sources that can't (Hangar, GitHub) return an empty map.
    async fn identify_by_hashes(&self, hashes: &[JarHashes]) -> Result<HashMap<String, (ProjectRef, ResolvedVersion)>>;

    /// Name search. Implementations may verify candidates by hash and upgrade confidence.
    async fn search(&self, query: &str, ctx: &CompatCtx, hashes: Option<&JarHashes>) -> Result<Vec<Candidate>>;

    async fn project(&self, id: &str) -> Result<ProjectRef>;

    /// All versions of a project, newest first, already filtered to the server's loader/platform
    /// (game-version compatibility is left to the caller so lenient modes can be applied).
    async fn versions(&self, project_id: &str, ctx: &CompatCtx) -> Result<Vec<ResolvedVersion>>;

    /// Stream a file to `dest`, reporting progress. Returns the hashes of what was written.
    async fn download(&self, file: &VersionFile, dest: &Path, progress: ProgressSink) -> Result<JarHashes> {
        download_file(&self.http(), file, dest, progress).await
    }

    fn http(&self) -> reqwest::Client;
}

/// Shared streaming download used by every source.
pub async fn download_file(http: &reqwest::Client, file: &VersionFile, dest: &Path, progress: ProgressSink) -> Result<JarHashes> {
    use futures::StreamExt;
    use sha1::Sha1;
    use sha2::{Digest, Sha256, Sha512};
    use tokio::io::AsyncWriteExt;

    let resp = http.get(&file.url).send().await?.error_for_status()?;
    let total = resp.content_length().or(file.size);
    let mut out = tokio::fs::File::create(dest).await?;
    let (mut h1, mut h256, mut h512) = (Sha1::new(), Sha256::new(), Sha512::new());
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        h1.update(&chunk);
        h256.update(&chunk);
        h512.update(&chunk);
        out.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    out.flush().await?;
    Ok(JarHashes { sha1: hex::encode(h1.finalize()), sha256: hex::encode(h256.finalize()), sha512: hex::encode(h512.finalize()), size: done })
}

/// The enabled sources, in identification priority order.
pub struct Sources {
    pub list: Vec<Box<dyn Source>>,
}

impl Sources {
    pub fn get(&self, kind: SourceKind) -> Option<&dyn Source> {
        self.list.iter().find(|s| s.kind() == kind).map(|b| b.as_ref())
    }

    pub fn locate(&self, url: &str) -> Option<ProjectLocator> {
        let u = url::Url::parse(url).ok()?;
        self.list.iter().find_map(|s| s.parse_url(&u))
    }
}
