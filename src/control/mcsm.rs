use async_trait::async_trait;

use super::{ServerControl, ServerStatus};
use crate::server::mcsm::{InstanceStatus, Mcsm};
use crate::Result;

pub struct McsmControl {
    pub mcsm: Mcsm,
    pub uuid: String,
    pub port: Option<u16>,
}

#[async_trait]
impl ServerControl for McsmControl {
    fn name(&self) -> &'static str {
        "mcsmanager"
    }
    fn can_console(&self) -> bool {
        true
    }
    fn can_restart(&self) -> bool {
        true
    }
    async fn status(&self) -> Result<ServerStatus> {
        Ok(match self.mcsm.status(&self.uuid).await? {
            InstanceStatus::Running => ServerStatus::Running,
            InstanceStatus::Stopped => ServerStatus::Stopped,
            InstanceStatus::Starting => ServerStatus::Starting,
            InstanceStatus::Stopping => ServerStatus::Stopping,
            InstanceStatus::Busy => ServerStatus::Busy,
            InstanceStatus::Unknown(_) => ServerStatus::Unknown,
        })
    }
    async fn send_command(&self, cmd: &str) -> Result<()> {
        self.mcsm.command(&self.uuid, cmd).await
    }
    async fn stop(&self) -> Result<()> {
        self.mcsm.stop(&self.uuid).await
    }
    async fn start(&self) -> Result<()> {
        self.mcsm.start(&self.uuid).await
    }
    async fn restart(&self) -> Result<()> {
        self.mcsm.restart(&self.uuid).await
    }
    async fn player_count(&self) -> Result<Option<u32>> {
        if self.status().await? != ServerStatus::Running {
            return Ok(Some(0));
        }
        match self.port {
            Some(p) => Ok(super::ping::player_count(p).await),
            None => self.mcsm.panel_players(&self.uuid).await,
        }
    }
    async fn ready(&self) -> Result<bool> {
        if self.status().await? != ServerStatus::Running {
            return Ok(false);
        }
        Ok(match self.port {
            Some(p) => super::ping::player_count(p).await.is_some(),
            None => true,
        })
    }
    async fn set_start_command(&self, cmd: &str) -> Result<()> {
        self.mcsm.set_start_command(&self.uuid, cmd).await
    }
}
