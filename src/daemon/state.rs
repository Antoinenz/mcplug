use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DaemonState {
    pub heartbeat: Option<chrono::DateTime<chrono::Utc>>,
    pub pid: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerState>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub last_check: Option<chrono::DateTime<chrono::Utc>>,
    pub last_result: Option<String>,
    #[serde(default)]
    pub updates_available: Vec<String>,
    pub pending_restart: Option<chrono::DateTime<chrono::Utc>>,
    pub last_warning: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn path() -> PathBuf {
    crate::config::paths::state_dir().join("daemon.json")
}

impl DaemonState {
    pub fn load(p: &Path) -> Self {
        std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, p: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(p.parent().expect("state dir"))?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, p)
    }

    /// "running (tick 40s ago)" / "not running" for status bars.
    pub fn describe(&self) -> String {
        match self.heartbeat {
            Some(h) => {
                let age = chrono::Utc::now() - h;
                if age < chrono::Duration::minutes(3) {
                    format!("daemon: running (tick {}s ago)", age.num_seconds())
                } else {
                    format!(
                        "daemon: stale (last tick {} ago)",
                        humantime::format_duration(std::time::Duration::from_secs(age.num_seconds().max(0) as u64))
                    )
                }
            }
            None => "daemon: not running".into(),
        }
    }
}

/// One daemon per machine.
pub fn acquire_lock() -> std::io::Result<std::fs::File> {
    let p = crate::config::paths::state_dir().join("daemon.lock");
    std::fs::create_dir_all(p.parent().expect("state dir"))?;
    let f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&p)?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        extern "C" {
            fn flock(fd: i32, op: i32) -> i32;
        }
        // LOCK_EX | LOCK_NB
        if unsafe { flock(f.as_raw_fd(), 2 | 4) } != 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "another mcplug daemon is already running"));
        }
    }
    Ok(f)
}
