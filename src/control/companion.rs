//! McplugBridge: a tiny HTTP listener inside the server. Wraps another control (MCSManager,
//! RCON…) and upgrades what it can: titled countdowns, ops-only notices, player/TPS data.

use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

use super::{ServerControl, ServerStatus};
use crate::Result;

pub const PLUGIN_NAME: &str = "McplugBridge";
pub const JAR_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/McplugBridge.jar"));
pub const JAR_VERSION: &str = "0.1.0";
pub const FIRST_PORT: u16 = 25580;

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeConfig {
    pub port: u16,
    pub token: String,
    #[serde(rename = "daemon-url", default)]
    pub daemon_url: String,
}

impl BridgeConfig {
    pub fn path(plugins_dir: &Path) -> std::path::PathBuf {
        plugins_dir.join(PLUGIN_NAME).join("config.yml")
    }

    /// The bridge config, if the plugin is installed and has generated its token.
    pub fn load(plugins_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(plugins_dir)).ok()?;
        let c: BridgeConfig = serde_yaml::from_str(&text).ok()?;
        (!c.token.is_empty()).then_some(c)
    }

    pub fn write(plugins_dir: &Path, port: u16, token: &str, daemon_port: u16) -> std::io::Result<()> {
        let p = Self::path(plugins_dir);
        std::fs::create_dir_all(p.parent().expect("plugin dir"))?;
        std::fs::write(
            p,
            format!(
                "# Written by mcplug. The bridge listens on 127.0.0.1 only; the token authorises both\n# mcplug -> server calls and the in-game /mcplug command -> mcplug daemon.\nport: {port}\ntoken: \"{token}\"\ndaemon-url: http://127.0.0.1:{daemon_port}\n"
            ),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeStatus {
    pub players: u32,
    #[serde(default)]
    pub names: Vec<String>,
    pub tps: f64,
    pub mspt: f64,
    pub version: String,
    pub bridge: String,
}

pub struct Companion {
    pub inner: Box<dyn ServerControl>,
    pub cfg: BridgeConfig,
    http: reqwest::Client,
}

impl Companion {
    pub fn wrap(inner: Box<dyn ServerControl>, cfg: BridgeConfig, http: reqwest::Client) -> Self {
        Self { inner, cfg, http }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.cfg.port, path)
    }

    pub async fn bridge_status(&self) -> Result<BridgeStatus> {
        let r = self
            .http
            .get(self.url("/status"))
            .bearer_auth(&self.cfg.token)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        Ok(r.json().await?)
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<()> {
        self.http
            .post(self.url(path))
            .bearer_auth(&self.cfg.token)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn notify_ops(&self, message: &str) -> Result<()> {
        self.post("/notify", serde_json::json!({ "message": message })).await
    }
}

#[async_trait]
impl ServerControl for Companion {
    fn name(&self) -> &'static str {
        "bridge"
    }
    fn can_console(&self) -> bool {
        true
    }
    fn can_restart(&self) -> bool {
        self.inner.can_restart()
    }
    async fn status(&self) -> Result<ServerStatus> {
        match self.inner.status().await {
            Ok(ServerStatus::Unknown) | Err(_) => Ok(if self.bridge_status().await.is_ok() {
                ServerStatus::Running
            } else {
                ServerStatus::Stopped
            }),
            other => other,
        }
    }
    async fn send_command(&self, cmd: &str) -> Result<()> {
        self.inner.send_command(cmd).await
    }
    async fn broadcast(&self, msg: &str) -> Result<()> {
        if self.post("/broadcast", serde_json::json!({ "message": msg })).await.is_ok() {
            return Ok(());
        }
        self.inner.broadcast(msg).await
    }
    async fn countdown(&self, seconds: u32, reason: &str) -> Result<()> {
        if self
            .post("/countdown", serde_json::json!({ "seconds": seconds, "reason": reason }))
            .await
            .is_ok()
        {
            return Ok(());
        }
        self.inner.countdown(seconds, reason).await
    }
    async fn stop(&self) -> Result<()> {
        self.inner.stop().await
    }
    async fn start(&self) -> Result<()> {
        self.inner.start().await
    }
    async fn restart(&self) -> Result<()> {
        self.inner.restart().await
    }
    async fn player_count(&self) -> Result<Option<u32>> {
        match self.bridge_status().await {
            Ok(s) => Ok(Some(s.players)),
            Err(_) => self.inner.player_count().await,
        }
    }
    async fn ready(&self) -> Result<bool> {
        Ok(self.bridge_status().await.is_ok())
    }
    async fn set_start_command(&self, cmd: &str) -> Result<()> {
        self.inner.set_start_command(cmd).await
    }
}

/// Pick a port no other known bridge uses.
pub fn free_port(used: &[u16]) -> u16 {
    (FIRST_PORT..FIRST_PORT + 200).find(|p| !used.contains(p)).unwrap_or(FIRST_PORT)
}

pub fn new_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // 32 bytes from the OS RNG via getrandom-free fallback: /dev/urandom on unix, time+pid elsewhere.
    let mut bytes = [0u8; 32];
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let _ = f.read_exact(&mut bytes);
        }
    }
    if bytes.iter().all(|b| *b == 0) {
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) ^ (std::process::id() as u128);
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (seed >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(31);
        }
    }
    hex::encode(bytes)
}
