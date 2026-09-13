pub mod check;
pub mod scan;
pub mod servers;

use crate::config::Loaded;
use crate::error::{Error, Result};
use crate::lockfile::CompatMode;
use crate::platform::{self, Platform};
use crate::server::{discover, Server};
use crate::sources::{CompatCtx, Sources};

/// Shared context for CLI commands.
pub struct Ctx {
    pub loaded: Loaded,
    pub http: reqwest::Client,
    pub json: bool,
}

impl Ctx {
    pub fn mcsm(&self) -> Option<crate::server::mcsm::Mcsm> {
        let key = self.loaded.mcsm_api_key()?;
        Some(crate::server::mcsm::Mcsm::new(self.http.clone(), &self.loaded.config.mcsmanager.url, &key))
    }

    pub fn sources(&self) -> Sources {
        let c = &self.loaded.config.sources;
        let s = &self.loaded.secrets;
        let mut list: Vec<Box<dyn crate::sources::Source>> = Vec::new();
        if c.modrinth {
            list.push(Box::new(crate::sources::modrinth::Modrinth::new(self.http.clone(), s.modrinth_token.clone())));
        }
        if c.geysermc {
            list.push(Box::new(crate::sources::geysermc::GeyserMc::new(self.http.clone())));
        }
        if c.hangar {
            list.push(Box::new(crate::sources::hangar::Hangar::new(self.http.clone())));
        }
        if c.github {
            list.push(Box::new(crate::sources::github::GitHub::new(self.http.clone(), s.github_token.clone())));
        }
        Sources { list }
    }

    /// Find a server by id, or by unambiguous prefix of its id or name.
    pub fn server(&self, query: &str) -> Result<Server> {
        let servers = discover(&self.loaded.config);
        if let Some(s) = servers.iter().find(|s| s.id == query || s.mcsm_uuid() == Some(query)) {
            return Ok(s.clone());
        }
        let q = query.to_ascii_lowercase();
        let m: Vec<&Server> = servers.iter().filter(|s| s.id.starts_with(&q) || s.name.to_ascii_lowercase().starts_with(&q)).collect();
        match m.len() {
            1 => Ok(m[0].clone()),
            0 => Err(Error::Msg(format!("no server matches {query:?} (see `mcplug servers`)"))),
            _ => Err(Error::Msg(format!("{query:?} is ambiguous: {}", m.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")))),
        }
    }
}

/// Platform + compatibility context for a server, or an error if we can't manage it.
pub fn server_platform(server: &Server) -> Result<(platform::bukkit::Bukkit, CompatCtx)> {
    let p = platform::for_kind(server.platform).ok_or_else(|| Error::Msg(format!("{}: platform {} is not supported yet", server.id, server.platform)))?;
    let mc = server
        .jar
        .as_ref()
        .and_then(|j| j.mc_version.clone())
        .ok_or_else(|| Error::Msg(format!("{}: could not determine the Minecraft version from the server jar", server.id)))?;
    let ctx = CompatCtx {
        mc_version: mc,
        loaders: p.modrinth_loaders().iter().map(|s| s.to_string()).collect(),
        hangar_platform: p.hangar_platform().map(str::to_string),
        mode: CompatMode::Strict,
    };
    Ok((p, ctx))
}
