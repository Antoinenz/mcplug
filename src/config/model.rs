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
        }
    }
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
