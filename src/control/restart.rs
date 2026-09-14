//! Restart policies: how and when to bounce the server after an update.

use std::time::Duration;

use super::{wait_for, ServerControl, ServerStatus};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Announce a countdown, then restart.
    Now { countdown_secs: u32 },
    /// Wait until nobody is online (polling), then a short countdown and restart.
    WhenEmpty { max_wait: Duration, countdown_secs: u32 },
    /// Leave the jars staged; the next restart picks them up.
    Never,
}

impl RestartPolicy {
    pub fn parse(s: &str, countdown_secs: u32) -> Option<Self> {
        Some(match s {
            "now" => Self::Now { countdown_secs },
            "when-empty" | "empty" => Self::WhenEmpty {
                max_wait: Duration::from_secs(12 * 3600),
                countdown_secs,
            },
            "never" | "no" | "none" => Self::Never,
            _ => return None,
        })
    }
}

/// Runs the policy. `log` receives human-readable progress lines.
pub async fn execute_restart(control: &dyn ServerControl, policy: &RestartPolicy, reason: &str, log: &(dyn Fn(String) + Send + Sync)) -> Result<()> {
    let countdown = match policy {
        RestartPolicy::Never => {
            log("restart: skipped (staged; takes effect on the next restart)".into());
            return Ok(());
        }
        RestartPolicy::Now { countdown_secs } => *countdown_secs,
        RestartPolicy::WhenEmpty { max_wait, countdown_secs } => {
            let start = std::time::Instant::now();
            loop {
                match control.player_count().await? {
                    Some(0) | None => break,
                    Some(n) => {
                        if start.elapsed() > *max_wait {
                            log(format!(
                                "restart: still {n} online after {}; restarting anyway",
                                humantime::format_duration(*max_wait)
                            ));
                            break;
                        }
                        log(format!("restart: waiting for {n} player(s) to leave"));
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                }
            }
            (*countdown_secs).min(15)
        }
    };
    if control.status().await? != ServerStatus::Running {
        log("restart: server is not running; starting it".into());
        control.start().await?;
        return super::wait_ready(control, Duration::from_secs(300)).await;
    }
    if control.can_console() && countdown > 0 {
        let marks: Vec<u32> = [60, 30, 10, 5, 3, 2, 1].into_iter().filter(|m| *m <= countdown).collect();
        let mut remaining = countdown;
        let _ = control.countdown(remaining, reason).await;
        for m in marks {
            if m < remaining {
                tokio::time::sleep(Duration::from_secs((remaining - m) as u64)).await;
                remaining = m;
                let _ = control.countdown(m, reason).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(remaining as u64)).await;
    }
    log("restart: restarting".into());
    control.restart().await?;
    // MCSManager reports Stopping→Starting→Running; give it a moment to leave Running first.
    tokio::time::sleep(Duration::from_secs(5)).await;
    wait_for(control, ServerStatus::Running, Duration::from_secs(300)).await?;
    log("restart: process started, waiting for the server to finish loading".into());
    super::wait_ready(control, Duration::from_secs(300)).await?;
    log("restart: server is up".into());
    Ok(())
}
