use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// `~/.config/mcplug/config.toml`. Everything here is safe to commit to a dotfiles repo;
/// secrets live in `secrets.toml` next to it (see [`super::secrets`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    /// Contact put into the User-Agent, as Modrinth's API terms ask for.
    pub contact: Option<String>,
    pub mcsmanager: McsmConfig,
    #[serde(rename = "servers")]
    pub manual_servers: Vec<ManualServer>,
    pub sources: SourcesConfig,
    pub backup: BackupConfig,
    pub policy: Policy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            contact: None,
            mcsmanager: McsmConfig::default(),
            manual_servers: Vec::new(),
            sources: SourcesConfig::default(),
            backup: BackupConfig::default(),
            policy: Policy::default(),
        }
    }
}

/// What the daemon does on its own. Defaults are deliberately conservative: it checks,
/// applies only same-Minecraft-version releases, restarts when the server is empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// How often to check each server, e.g. "6h", "30m".
    pub check_interval: String,
    /// none | same-mc-release | all
    pub auto_apply: String,
    /// now | when-empty | scheduled | never
    pub restart: String,
    /// Local time "HH:MM" used by `scheduled` (and as the fallback when `when-empty` waits too long).
    pub restart_at: String,
    pub countdown: u32,
    /// Local time ranges "22:00-08:00" during which nothing is applied or restarted.
    pub quiet_hours: Vec<String>,
    /// Geyser/floodgate ship builds almost daily; only auto-update them when asked to.
    pub geyser_auto: bool,
    /// mcbackup checkpoint/snapshot around automatic updates (if mcbackup is installed).
    pub backup: bool,
    /// Keep the server jar on the newest build of its Minecraft version.
    pub server_jar_builds: bool,
    /// Per-server overrides keyed by server id.
    pub per_server: std::collections::BTreeMap<String, PolicyOverride>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            check_interval: "6h".into(),
            auto_apply: "same-mc-release".into(),
            restart: "when-empty".into(),
            restart_at: "04:30".into(),
            countdown: 60,
            quiet_hours: vec![],
            geyser_auto: false,
            backup: true,
            server_jar_builds: false,
            per_server: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyOverride {
    pub enabled: Option<bool>,
    pub check_interval: Option<String>,
    pub auto_apply: Option<String>,
    pub restart: Option<String>,
    pub restart_at: Option<String>,
    pub countdown: Option<u32>,
    pub quiet_hours: Option<Vec<String>>,
    pub geyser_auto: Option<bool>,
    pub backup: Option<bool>,
    pub server_jar_builds: Option<bool>,
}

impl Policy {
    /// Effective policy for one server.
    pub fn for_server(&self, id: &str) -> EffectivePolicy {
        let o = self.per_server.get(id).cloned().unwrap_or_default();
        EffectivePolicy {
            enabled: o.enabled.unwrap_or(true),
            check_interval: humantime::parse_duration(o.check_interval.as_deref().unwrap_or(&self.check_interval)).unwrap_or(std::time::Duration::from_secs(6 * 3600)),
            auto_apply: o.auto_apply.unwrap_or_else(|| self.auto_apply.clone()),
            restart: o.restart.unwrap_or_else(|| self.restart.clone()),
            restart_at: o.restart_at.unwrap_or_else(|| self.restart_at.clone()),
            countdown: o.countdown.unwrap_or(self.countdown),
            quiet_hours: o.quiet_hours.unwrap_or_else(|| self.quiet_hours.clone()),
            geyser_auto: o.geyser_auto.unwrap_or(self.geyser_auto),
            backup: o.backup.unwrap_or(self.backup),
            server_jar_builds: o.server_jar_builds.unwrap_or(self.server_jar_builds),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    pub enabled: bool,
    pub check_interval: std::time::Duration,
    pub auto_apply: String,
    pub restart: String,
    pub restart_at: String,
    pub countdown: u32,
    pub quiet_hours: Vec<String>,
    pub geyser_auto: bool,
    pub backup: bool,
    pub server_jar_builds: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McsmConfig {
    pub enabled: bool,
    pub url: String,
    /// Where the MCSManager daemon keeps one JSON per instance.
    pub instance_config_dir: PathBuf,
    /// Instance nicknames or uuids to skip.
    pub ignore: Vec<String>,
    /// Optional `KEY=VALUE` file to read `MCSM_APIKEY` from when secrets.toml has none
    /// (mcbackup keeps its key in `/opt/mcbackup/env`, so one key can serve both tools).
    pub api_key_env_file: Option<PathBuf>,
}

impl Default for McsmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: "http://127.0.0.1:23333".into(),
            instance_config_dir: PathBuf::from("/opt/mcsmanager/daemon/data/InstanceConfig"),
            ignore: vec!["__MCSM_GLOBAL_INSTANCE__".into()],
            api_key_env_file: Some(PathBuf::from("/opt/mcbackup/env")),
        }
    }
}

/// A server that isn't managed by a panel: just a directory, plus how to talk to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualServer {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// Start command, used to find the server jar and the java binary. If absent mcplug
    /// looks for a single `*.jar` in `path`.
    pub start_command: Option<String>,
    pub ping_port: Option<u16>,
    #[serde(default)]
    pub control: ControlConfig,
    /// mcbackup source name; defaults to `slugify(name)`.
    pub backup_slug: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ControlConfig {
    /// No way to talk to the server: updates are staged, restarts are the user's job.
    #[default]
    None,
    Rcon {
        #[serde(default = "localhost")]
        host: String,
        port: u16,
        /// Key in secrets.toml `[rcon]` table.
        password_ref: String,
        restart_command: Option<String>,
    },
    Command {
        restart_command: String,
    },
}

fn localhost() -> String {
    "127.0.0.1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    pub modrinth: bool,
    pub hangar: bool,
    pub github: bool,
    pub geysermc: bool,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self { modrinth: true, hangar: true, github: true, geysermc: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupConfig {
    /// `"auto"` (use `mcbackup` if on PATH or at /usr/local/bin), `"off"`, or a path.
    pub mcbackup: String,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self { mcbackup: "auto".into() }
    }
}
