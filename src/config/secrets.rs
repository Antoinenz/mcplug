use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// `secrets.toml`, 0600. Never merged into the lockfile or journal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Secrets {
    pub modrinth_token: Option<String>,
    pub mcsm_api_key: Option<String>,
    pub github_token: Option<String>,
    pub rcon: BTreeMap<String, String>,
}

impl Secrets {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        check_private(path)?;
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Fall back to a `KEY=VALUE` env file (e.g. mcbackup's `/opt/mcbackup/env`) for the
    /// MCSManager key so one key can serve both tools.
    pub fn mcsm_key_with_fallback(&self, env_file: Option<&Path>) -> Option<String> {
        if let Some(k) = &self.mcsm_api_key {
            if !k.is_empty() {
                return Some(k.clone());
            }
        }
        let text = std::fs::read_to_string(env_file?).ok()?;
        text.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "MCSM_APIKEY" && !v.trim().is_empty())
                .then(|| v.trim().trim_matches('"').to_string())
        })
    }
}

#[cfg(unix)]
fn check_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(Error::InsecureSecrets { path: path.to_path_buf(), mode });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_path: &Path) -> Result<()> {
    Ok(())
}
