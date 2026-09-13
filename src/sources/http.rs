//! Thin helpers over reqwest: JSON get/post with a per-source minimum spacing between
//! requests (Modrinth allows 300/min; we stay well under it) and Retry-After handling.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::error::{Error, Result};

#[derive(Clone)]
pub struct Api {
    pub http: reqwest::Client,
    min_gap: Duration,
    last: Arc<Mutex<Option<Instant>>>,
    pub auth: Option<String>,
}

impl Api {
    pub fn new(http: reqwest::Client, per_minute: u32, auth: Option<String>) -> Self {
        Self { http, min_gap: Duration::from_millis(60_000 / per_minute.max(1) as u64), last: Arc::new(Mutex::new(None)), auth }
    }

    async fn pace(&self) {
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            let elapsed = t.elapsed();
            if elapsed < self.min_gap {
                tokio::time::sleep(self.min_gap - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Some(a) => req.header(reqwest::header::AUTHORIZATION, a),
            None => req,
        }
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str, query: &[(&str, String)]) -> Result<T> {
        for attempt in 0..3 {
            self.pace().await;
            let resp = self.apply_auth(self.http.get(url).query(query)).send().await?;
            match resp.status().as_u16() {
                429 => {
                    let wait = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(5);
                    tracing::warn!(url, wait, "rate limited");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
                s if s >= 500 && attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                    continue;
                }
                _ => {}
            }
            let resp = resp.error_for_status().map_err(|e| Error::Msg(format!("{url}: {e}")))?;
            return Ok(resp.json().await?);
        }
        Err(Error::Msg(format!("{url}: gave up after retries")))
    }

    pub async fn post_json<T: DeserializeOwned, B: serde::Serialize>(&self, url: &str, body: &B) -> Result<T> {
        for attempt in 0..3 {
            self.pace().await;
            let resp = self.apply_auth(self.http.post(url).json(body)).send().await?;
            match resp.status().as_u16() {
                429 => {
                    let wait = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(5);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
                s if s >= 500 && attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                    continue;
                }
                _ => {}
            }
            let resp = resp.error_for_status().map_err(|e| Error::Msg(format!("{url}: {e}")))?;
            return Ok(resp.json().await?);
        }
        Err(Error::Msg(format!("{url}: gave up after retries")))
    }
}
