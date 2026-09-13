//! Optional integration with mcbackup (https://github.com/Antoinenz/mcbackup): a pinned
//! checkpoint before an update and a normal snapshot after a successful restart, so a bad
//! plugin update can always be rolled back to a known-good world.

use std::path::PathBuf;
use std::time::Duration;

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Mcbackup {
    pub bin: PathBuf,
}

impl Mcbackup {
    /// Honour the config: `"off"`, `"auto"` (search), or an explicit path.
    pub fn detect(setting: &str) -> Option<Self> {
        match setting {
            "off" | "" => None,
            "auto" => {
                let candidates = ["/usr/local/bin/mcbackup", "/usr/bin/mcbackup"];
                candidates.iter().map(PathBuf::from).find(|p| p.exists()).or_else(|| which("mcbackup")).map(|bin| Self { bin })
            }
            path => Some(Self { bin: PathBuf::from(path) }),
        }
    }

    async fn run(&self, args: &[&str], timeout: Duration) -> Result<String> {
        let mut cmd = tokio::process::Command::new(&self.bin);
        cmd.args(args);
        let out = tokio::time::timeout(timeout, cmd.output()).await.map_err(|_| Error::Msg(format!("mcbackup {} timed out", args[0])))??;
        if !out.status.success() {
            return Err(Error::Msg(format!("mcbackup {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Pinned snapshot that retention never removes.
    pub async fn checkpoint(&self, slug: &str, message: &str) -> Result<String> {
        self.run(&["checkpoint", slug, "-m", message], Duration::from_secs(20 * 60)).await
    }

    pub async fn backup(&self, slug: &str, message: &str) -> Result<String> {
        self.run(&["backup", slug, "-m", message], Duration::from_secs(20 * 60)).await
    }

    /// Source names mcbackup knows, so we can warn when a server isn't covered.
    pub async fn sources(&self) -> Result<Vec<String>> {
        let out = self.run(&["discover"], Duration::from_secs(60)).await?;
        Ok(out.lines().filter(|l| !l.starts_with(' ')).filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect())
    }
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| PathBuf::from(d).join(name)).find(|p| p.exists())
}
