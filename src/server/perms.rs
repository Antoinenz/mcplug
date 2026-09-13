use std::path::Path;

/// Whether mcplug can write into a server's plugin directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    ReadWrite,
    ReadOnly { reason: String },
    Missing,
}

impl Access {
    pub fn probe(plugins_dir: &Path) -> Access {
        if !plugins_dir.is_dir() {
            return Access::Missing;
        }
        let marker = plugins_dir.join(".mcplug");
        match std::fs::create_dir_all(&marker) {
            Ok(()) => Access::ReadWrite,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let hint = if cfg!(unix) && !crate::config::paths::is_root() {
                    " (try running with sudo)"
                } else {
                    ""
                };
                Access::ReadOnly { reason: format!("{} not writable{hint}", plugins_dir.display()) }
            }
            Err(e) => Access::ReadOnly { reason: e.to_string() },
        }
    }

    pub fn writable(&self) -> bool {
        matches!(self, Access::ReadWrite)
    }
}
