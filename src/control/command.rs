use std::path::PathBuf;

use async_trait::async_trait;

use super::{ServerControl, ServerStatus};
use crate::{Error, Result};

/// A restart command and nothing else (e.g. `systemctl restart mc-survival`).
pub struct CommandControl {
    pub restart_command: String,
    pub ping_port: Option<u16>,
    pub cwd: PathBuf,
}

pub async fn run_shell(cmd: &str, cwd: &std::path::Path) -> Result<()> {
    let out = tokio::process::Command::new("sh").arg("-c").arg(cmd).current_dir(cwd).output().await?;
    if !out.status.success() {
        return Err(Error::Msg(format!("`{cmd}` failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(())
}

#[async_trait]
impl ServerControl for CommandControl {
    fn name(&self) -> &'static str {
        "command"
    }
    fn can_console(&self) -> bool {
        false
    }
    fn can_restart(&self) -> bool {
        true
    }
    async fn status(&self) -> Result<ServerStatus> {
        Ok(match self.ping_port.map(super::ping::player_count_sync) {
            Some(Some(_)) => ServerStatus::Running,
            Some(None) => ServerStatus::Stopped,
            None => ServerStatus::Unknown,
        })
    }
    async fn send_command(&self, _cmd: &str) -> Result<()> {
        Err(Error::Msg("no console access for this server (configure RCON)".into()))
    }
    async fn broadcast(&self, _msg: &str) -> Result<()> {
        Ok(()) // silently skipped: nothing to announce through
    }
    async fn stop(&self) -> Result<()> {
        Err(Error::Msg("no stop command configured".into()))
    }
    async fn start(&self) -> Result<()> {
        Err(Error::Msg("no start command configured".into()))
    }
    async fn restart(&self) -> Result<()> {
        run_shell(&self.restart_command, &self.cwd).await
    }
    async fn player_count(&self) -> Result<Option<u32>> {
        Ok(match self.ping_port {
            Some(p) => super::ping::player_count(p).await.or(Some(0)),
            None => None,
        })
    }
}

/// No way to talk to the server at all: updates are staged, restarting is the user's job.
pub struct NoControl {
    pub ping_port: Option<u16>,
}

#[async_trait]
impl ServerControl for NoControl {
    fn name(&self) -> &'static str {
        "none"
    }
    fn can_console(&self) -> bool {
        false
    }
    fn can_restart(&self) -> bool {
        false
    }
    async fn status(&self) -> Result<ServerStatus> {
        Ok(match self.ping_port.map(super::ping::player_count_sync) {
            Some(Some(_)) => ServerStatus::Running,
            Some(None) => ServerStatus::Stopped,
            None => ServerStatus::Unknown,
        })
    }
    async fn send_command(&self, _cmd: &str) -> Result<()> {
        Err(Error::Msg("no console access for this server".into()))
    }
    async fn broadcast(&self, _msg: &str) -> Result<()> {
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        Err(Error::Msg("no control configured for this server".into()))
    }
    async fn start(&self) -> Result<()> {
        Err(Error::Msg("no control configured for this server".into()))
    }
    async fn restart(&self) -> Result<()> {
        Err(Error::Msg("no control configured for this server — restart it yourself".into()))
    }
    async fn player_count(&self) -> Result<Option<u32>> {
        Ok(match self.ping_port {
            Some(p) => super::ping::player_count(p).await,
            None => None,
        })
    }
}
