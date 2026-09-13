//! Append-only `plugins/.mcplug/journal.jsonl`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub time: chrono::DateTime<chrono::Utc>,
    pub tx_id: String,
    /// `update`, `install`, `revert`, `aborted`, `restart`, `backup`
    pub action: String,
    pub outcome: String,
    pub items: Vec<JournalItem>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalItem {
    pub name: String,
    pub from: Option<String>,
    pub to: String,
    pub old_file: Option<String>,
    pub new_file: String,
}

pub fn path(plugins_dir: &Path) -> std::path::PathBuf {
    super::mcplug_dir(plugins_dir).join("journal.jsonl")
}

pub fn append(plugins_dir: &Path, entry: &JournalEntry) -> std::io::Result<()> {
    use std::io::Write;
    let p = path(plugins_dir);
    std::fs::create_dir_all(p.parent().expect("journal dir"))?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    writeln!(f, "{}", serde_json::to_string(entry)?)
}

pub fn read(plugins_dir: &Path) -> Vec<JournalEntry> {
    let Ok(text) = std::fs::read_to_string(path(plugins_dir)) else {
        return vec![];
    };
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}
