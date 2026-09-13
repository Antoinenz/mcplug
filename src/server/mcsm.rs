//! Minimal MCSManager panel API client (v10).

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceStatus {
    Busy,
    Stopped,
    Stopping,
    Starting,
    Running,
    Unknown(i64),
}

impl InstanceStatus {
    fn from_code(c: i64) -> Self {
        match c {
            -1 => Self::Busy,
            0 => Self::Stopped,
            1 => Self::Stopping,
            2 => Self::Starting,
            3 => Self::Running,
            n => Self::Unknown(n),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mcsm {
    http: reqwest::Client,
    url: String,
    api_key: String,
    daemon_id: OnceCell<String>,
}

#[derive(Deserialize)]
struct Envelope {
    status: i64,
    data: Option<Value>,
}

impl Mcsm {
    pub fn new(http: reqwest::Client, url: &str, api_key: &str) -> Self {
        Self { http, url: url.trim_end_matches('/').to_string(), api_key: api_key.to_string(), daemon_id: OnceCell::new() }
    }

    async fn call(&self, method: reqwest::Method, path: &str, query: &[(&str, &str)], body: Option<Value>) -> Result<Value> {
        let mut req = self
            .http
            .request(method, format!("{}{}", self.url, path))
            .query(query)
            .query(&[("apikey", self.api_key.as_str())])
            .header("X-Requested-With", "XMLHttpRequest");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let env: Envelope = req.send().await?.error_for_status()?.json().await?;
        if env.status != 200 {
            return Err(Error::Msg(format!("MCSManager {path}: status {} {}", env.status, env.data.unwrap_or(Value::Null))));
        }
        Ok(env.data.unwrap_or(Value::Null))
    }

    /// The first remote daemon's uuid, cached.
    pub async fn daemon_id(&self) -> Result<String> {
        self.daemon_id
            .get_or_try_init(|| async {
                let d = self.call(reqwest::Method::GET, "/api/overview", &[], None).await?;
                d["remote"][0]["uuid"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| Error::Msg("MCSManager: no daemon in /api/overview".into()))
            })
            .await
            .cloned()
    }

    pub async fn status(&self, uuid: &str) -> Result<InstanceStatus> {
        let d = self.daemon_id().await?;
        let v = self.call(reqwest::Method::GET, "/api/instance", &[("uuid", uuid), ("daemonId", &d)], None).await?;
        Ok(InstanceStatus::from_code(v["status"].as_i64().unwrap_or(99)))
    }

    /// Player count as reported by the panel's own ping, if it has one.
    pub async fn panel_players(&self, uuid: &str) -> Result<Option<u32>> {
        let d = self.daemon_id().await?;
        let v = self.call(reqwest::Method::GET, "/api/instance", &[("uuid", uuid), ("daemonId", &d)], None).await?;
        Ok(v["info"]["currentPlayers"].as_i64().filter(|n| *n >= 0).map(|n| n as u32))
    }

    pub async fn command(&self, uuid: &str, cmd: &str) -> Result<()> {
        let d = self.daemon_id().await?;
        self.call(reqwest::Method::POST, "/api/protected_instance/command", &[("uuid", uuid), ("daemonId", &d), ("command", cmd)], None).await?;
        Ok(())
    }

    async fn simple(&self, uuid: &str, action: &str) -> Result<()> {
        let d = self.daemon_id().await?;
        self.call(reqwest::Method::POST, &format!("/api/protected_instance/{action}"), &[("uuid", uuid), ("daemonId", &d)], None).await?;
        Ok(())
    }

    pub async fn start(&self, uuid: &str) -> Result<()> {
        self.simple(uuid, "open").await
    }
    pub async fn stop(&self, uuid: &str) -> Result<()> {
        self.simple(uuid, "stop").await
    }
    pub async fn restart(&self, uuid: &str) -> Result<()> {
        self.simple(uuid, "restart").await
    }
    pub async fn kill(&self, uuid: &str) -> Result<()> {
        self.simple(uuid, "kill").await
    }

    pub async fn set_start_command(&self, uuid: &str, start_command: &str) -> Result<()> {
        let d = self.daemon_id().await?;
        self.call(
            reqwest::Method::PUT,
            "/api/instance",
            &[("uuid", uuid), ("daemonId", &d)],
            Some(serde_json::json!({ "startCommand": start_command })),
        )
        .await?;
        Ok(())
    }
}
