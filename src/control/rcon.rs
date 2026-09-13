//! Source RCON protocol (what Minecraft speaks): 4-byte LE length, id, type, payload, 2 NULs.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{ServerControl, ServerStatus};
use crate::{Error, Result};

pub struct RconControl {
    pub host: String,
    pub port: u16,
    pub password: String,
    pub restart_command: Option<String>,
    pub ping_port: Option<u16>,
    pub cwd: PathBuf,
}

const AUTH: i32 = 3;
const EXEC: i32 = 2;

async fn send(s: &mut TcpStream, id: i32, kind: i32, payload: &str) -> Result<()> {
    let body_len = 4 + 4 + payload.len() + 2;
    let mut buf = Vec::with_capacity(4 + body_len);
    buf.extend_from_slice(&(body_len as i32).to_le_bytes());
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(&kind.to_le_bytes());
    buf.extend_from_slice(payload.as_bytes());
    buf.extend_from_slice(&[0, 0]);
    s.write_all(&buf).await?;
    Ok(())
}

async fn recv(s: &mut TcpStream) -> Result<(i32, i32, String)> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await?;
    let len = i32::from_le_bytes(len).max(10) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await?;
    let id = i32::from_le_bytes(body[0..4].try_into().expect("4 bytes"));
    let kind = i32::from_le_bytes(body[4..8].try_into().expect("4 bytes"));
    let payload = String::from_utf8_lossy(&body[8..len - 2]).to_string();
    Ok((id, kind, payload))
}

impl RconControl {
    pub async fn exec(&self, cmd: &str) -> Result<String> {
        let mut s = tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect((self.host.as_str(), self.port)))
            .await
            .map_err(|_| Error::Msg("rcon: connect timed out".into()))??;
        send(&mut s, 1, AUTH, &self.password).await?;
        let (id, _, _) = recv(&mut s).await?;
        if id == -1 {
            return Err(Error::Msg("rcon: authentication failed (check the password)".into()));
        }
        send(&mut s, 2, EXEC, cmd).await?;
        let (_, _, out) = recv(&mut s).await?;
        Ok(out)
    }
}

#[async_trait]
impl ServerControl for RconControl {
    fn name(&self) -> &'static str {
        "rcon"
    }
    fn can_console(&self) -> bool {
        true
    }
    fn can_restart(&self) -> bool {
        self.restart_command.is_some()
    }
    async fn status(&self) -> Result<ServerStatus> {
        Ok(if self.exec("list").await.is_ok() {
            ServerStatus::Running
        } else {
            ServerStatus::Stopped
        })
    }
    async fn send_command(&self, cmd: &str) -> Result<()> {
        self.exec(cmd).await.map(|_| ())
    }
    async fn stop(&self) -> Result<()> {
        self.exec("stop").await.map(|_| ())
    }
    async fn start(&self) -> Result<()> {
        match &self.restart_command {
            Some(c) => super::command::run_shell(c, &self.cwd).await,
            None => Err(Error::Msg("rcon can stop the server but no restart_command is configured to start it".into())),
        }
    }
    async fn player_count(&self) -> Result<Option<u32>> {
        if let Some(p) = self.ping_port {
            if let Some(n) = super::ping::player_count(p).await {
                return Ok(Some(n));
            }
        }
        // `There are 3 of a max of 20 players online: …`
        let out = self.exec("list").await?;
        Ok(out.split_whitespace().nth(2).and_then(|n| n.parse().ok()))
    }
}
