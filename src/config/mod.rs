pub mod model;
pub mod paths;
pub mod secrets;

use std::path::{Path, PathBuf};

pub use model::*;
pub use secrets::Secrets;

use crate::error::Result;

/// Config + secrets + where they came from.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub dir: PathBuf,
    pub config: Config,
    pub secrets: Secrets,
}

impl Loaded {
    pub fn load(dir: Option<PathBuf>) -> Result<Self> {
        let dir = dir.unwrap_or_else(paths::config_dir);
        let config = load_config(&dir.join("config.toml"))?;
        let secrets = Secrets::load(&dir.join("secrets.toml"))?;
        Ok(Self { dir, config, secrets })
    }

    pub fn save_config(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let text = toml::to_string_pretty(&self.config)?;
        let path = self.dir.join("config.toml");
        std::fs::write(path.with_extension("toml.tmp"), text)?;
        std::fs::rename(path.with_extension("toml.tmp"), path)?;
        Ok(())
    }

    pub fn save_secrets(&self) -> Result<()> {
        self.secrets.save(&self.dir.join("secrets.toml"))
    }

    pub fn mcsm_api_key(&self) -> Option<String> {
        self.secrets.mcsm_key_with_fallback(self.config.mcsmanager.api_key_env_file.as_deref())
    }
}

fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}
