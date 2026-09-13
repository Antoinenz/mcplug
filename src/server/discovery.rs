//! Build the list of servers from MCSManager's instance files plus manual config entries.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::detect::{inspect_server_jar, PlatformKind, ServerJarInfo};
use super::perms::Access;
use super::start_command::StartCommand;
use crate::config::{Config, ControlConfig};
use crate::util::slug::slugify;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ServerOrigin {
    Mcsm { uuid: String },
    Manual,
}

#[derive(Debug, Clone, Serialize)]
pub struct Server {
    /// Stable identifier: the mcbackup-compatible slug of the name.
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub origin: ServerOrigin,
    pub start_command: Option<String>,
    pub jar: Option<ServerJarInfo>,
    pub platform: PlatformKind,
    pub ping_port: Option<u16>,
    pub control: ControlConfig,
    pub backup_slug: String,
    pub access: Access,
    /// Problems found while discovering (missing jar, unreadable dir…). Non-fatal.
    pub notes: Vec<String>,
}

impl Server {
    pub fn plugins_dir(&self) -> PathBuf {
        match self.platform {
            PlatformKind::Fabric => self.root.join("mods"),
            _ => self.root.join("plugins"),
        }
    }

    pub fn mcsm_uuid(&self) -> Option<&str> {
        match &self.origin {
            ServerOrigin::Mcsm { uuid } => Some(uuid),
            ServerOrigin::Manual => None,
        }
    }

    pub fn jar_path(&self) -> Option<PathBuf> {
        self.jar.as_ref().map(|j| self.root.join(&j.file_name))
    }
}

#[derive(Deserialize)]
struct McsmInstanceFile {
    nickname: String,
    cwd: String,
    #[serde(rename = "startCommand")]
    start_command: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "pingConfig")]
    ping: Option<PingConfig>,
}

#[derive(Deserialize)]
struct PingConfig {
    port: Option<u16>,
}

/// Discover every server. Cheap enough to call on each refresh; jar inspection is the only
/// real work (one hash per server jar).
pub fn discover(config: &Config) -> Vec<Server> {
    let mut out = Vec::new();
    if config.mcsmanager.enabled {
        out.extend(discover_mcsm(config));
    }
    for m in &config.manual_servers {
        let start = m.start_command.clone();
        let mut s = build_server(
            slugify(&m.id),
            m.name.clone(),
            m.path.clone(),
            ServerOrigin::Manual,
            start,
            m.ping_port,
            m.control.clone(),
        );
        s.backup_slug = m.backup_slug.clone().unwrap_or_else(|| slugify(&m.name));
        out.push(s);
    }
    out
}

fn discover_mcsm(config: &Config) -> Vec<Server> {
    let dir = &config.mcsmanager.instance_config_dir;
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    entries.sort();
    let mut out = Vec::new();
    for path in entries {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(inst) = serde_json::from_str::<McsmInstanceFile>(&text) else { continue };
        let uuid = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if config.mcsmanager.ignore.iter().any(|i| i == &inst.nickname || i == &uuid) {
            continue;
        }
        if inst.kind.as_deref() == Some("universal") || inst.cwd.is_empty() || inst.cwd == "/" {
            continue;
        }
        let root = find_server_root(Path::new(&inst.cwd));
        let mut s = build_server(
            slugify(&inst.nickname),
            inst.nickname.clone(),
            root.clone(),
            ServerOrigin::Mcsm { uuid },
            inst.start_command,
            inst.ping.and_then(|p| p.port).or_else(|| server_port(&root)),
            ControlConfig::None,
        );
        if root != Path::new(&inst.cwd) {
            s.notes.push(format!("server files are nested in {}", root.display()));
        }
        out.push(s);
    }
    out
}

fn build_server(
    id: String,
    name: String,
    root: PathBuf,
    origin: ServerOrigin,
    start_command: Option<String>,
    ping_port: Option<u16>,
    control: ControlConfig,
) -> Server {
    let mut notes = Vec::new();
    let jar_name = start_command
        .as_deref()
        .map(StartCommand::parse)
        .and_then(|c| c.jar().map(str::to_string))
        .or_else(|| single_jar(&root));
    let jar = match jar_name {
        Some(name) => match inspect_server_jar(&root.join(&name)) {
            Ok(j) => Some(j),
            Err(e) => {
                notes.push(format!("cannot read server jar {name}: {e}"));
                None
            }
        },
        None => {
            notes.push("no server jar found in start command or directory".into());
            None
        }
    };
    let platform = jar.as_ref().map(|j| j.platform).unwrap_or(PlatformKind::Unknown);
    let plugins_dir = match platform {
        PlatformKind::Fabric => root.join("mods"),
        _ => root.join("plugins"),
    };
    let access = Access::probe(&plugins_dir);
    let backup_slug = id.clone();
    Server { id, name, root, origin, start_command, jar, platform, ping_port, control, backup_slug, access, notes }
}

/// `server.properties` in `cwd`, or one level down (zip imports often keep a top folder).
fn find_server_root(cwd: &Path) -> PathBuf {
    if cwd.join("server.properties").exists() {
        return cwd.to_path_buf();
    }
    if let Ok(rd) = std::fs::read_dir(cwd) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("server.properties").exists()).collect();
        dirs.sort();
        if let Some(d) = dirs.into_iter().next() {
            return d;
        }
    }
    cwd.to_path_buf()
}

fn server_port(root: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(root.join("server.properties")).ok()?;
    text.lines().find_map(|l| l.strip_prefix("server-port=")).and_then(|v| v.trim().parse().ok())
}

fn single_jar(root: &Path) -> Option<String> {
    let mut jars: Vec<String> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.ends_with(".jar").then_some(n)
        })
        .collect();
    jars.sort();
    (jars.len() == 1).then(|| jars.remove(0))
}
