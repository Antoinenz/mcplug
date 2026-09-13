//! Talking to a running server: status, console commands, restarts.

pub mod command;
pub mod mcsm;
pub mod ping;
pub mod rcon;
pub mod restart;

use async_trait::async_trait;

use crate::config::ControlConfig;
use crate::server::Server;
use crate::Result;

pub use restart::{execute_restart, RestartPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Busy,
    Unknown,
}

#[async_trait]
pub trait ServerControl: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_console(&self) -> bool;
    fn can_restart(&self) -> bool;
    async fn status(&self) -> Result<ServerStatus>;
    async fn send_command(&self, cmd: &str) -> Result<()>;
    /// Chat broadcast; default goes through `say`.
    async fn broadcast(&self, msg: &str) -> Result<()> {
        self.send_command(&format!("say {msg}")).await
    }
    async fn stop(&self) -> Result<()>;
    async fn start(&self) -> Result<()>;
    async fn restart(&self) -> Result<()> {
        self.stop().await?;
        wait_for(self, ServerStatus::Stopped, std::time::Duration::from_secs(120)).await?;
        self.start().await
    }
    /// Players online, `None` when unknown. Pings the server-list port only while running.
    async fn player_count(&self) -> Result<Option<u32>>;
    /// Fully started and accepting players (not just "process exists").
    async fn ready(&self) -> Result<bool> {
        Ok(self.status().await? == ServerStatus::Running)
    }
    async fn set_start_command(&self, _cmd: &str) -> Result<()> {
        Err(crate::Error::Msg(format!("{} cannot change the start command", self.name())))
    }
}

pub async fn wait_ready(c: &(impl ServerControl + ?Sized), timeout: std::time::Duration) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        if c.ready().await? {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(crate::Error::Msg("timed out waiting for the server to finish starting".into()));
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

pub async fn wait_for(c: &(impl ServerControl + ?Sized), want: ServerStatus, timeout: std::time::Duration) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        if c.status().await? == want {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(crate::Error::Msg(format!("timed out waiting for server to be {want:?}")));
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Pick the right control for a server.
pub fn for_server(server: &Server, mcsm: Option<crate::server::mcsm::Mcsm>, secrets: &crate::config::Secrets) -> Box<dyn ServerControl> {
    if let (Some(uuid), Some(m)) = (server.mcsm_uuid(), mcsm) {
        return Box::new(mcsm::McsmControl { mcsm: m, uuid: uuid.to_string(), port: server.ping_port });
    }
    match &server.control {
        ControlConfig::Rcon { host, port, password_ref, restart_command } => Box::new(rcon::RconControl {
            host: host.clone(),
            port: *port,
            password: secrets.rcon.get(password_ref).cloned().unwrap_or_default(),
            restart_command: restart_command.clone(),
            ping_port: server.ping_port,
            cwd: server.root.clone(),
        }),
        ControlConfig::Command { restart_command } => Box::new(command::CommandControl { restart_command: restart_command.clone(), ping_port: server.ping_port, cwd: server.root.clone() }),
        ControlConfig::None => Box::new(command::NoControl { ping_port: server.ping_port }),
    }
}
