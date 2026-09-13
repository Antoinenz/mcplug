pub mod servers;

/// Shared context for CLI commands.
pub struct Ctx {
    pub loaded: crate::config::Loaded,
    pub http: reqwest::Client,
    pub json: bool,
}

impl Ctx {
    pub fn mcsm(&self) -> Option<crate::server::mcsm::Mcsm> {
        let key = self.loaded.mcsm_api_key()?;
        Some(crate::server::mcsm::Mcsm::new(self.http.clone(), &self.loaded.config.mcsmanager.url, &key))
    }
}
