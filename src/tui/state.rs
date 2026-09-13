use std::collections::VecDeque;
use std::sync::Arc;

use crate::lockfile::LockFile;
use crate::ops::ScanOutcome;
use crate::server::mcsm::Mcsm;
use crate::server::Server;
use crate::sources::identify::Undecided;
use crate::sources::resolve::CheckReport;
use crate::sources::Sources;
use crate::Result;

use crate::sources::{Candidate, ProjectLocator, ResolvedVersion};
use crate::transaction::journal::JournalEntry;
use crate::transaction::{TxOutcome, UpdatePlan};

/// Messages from background jobs.
pub enum Msg {
    Status {
        id: String,
        status: String,
        players: Option<u32>,
    },
    ScanDone {
        id: String,
        result: Result<ScanOutcome>,
    },
    CheckDone {
        id: String,
        result: Result<CheckReport>,
    },
    PlanBuilt {
        id: String,
        result: Result<UpdatePlan>,
    },
    VersionsLoaded {
        result: Result<Vec<ResolvedVersion>>,
    },
    SearchDone {
        result: Result<Vec<Candidate>>,
    },
    CollectionsLoaded {
        result: Result<Vec<crate::sources::modrinth::Collection>>,
    },
    ApplyProgress(String),
    ApplyDone {
        id: String,
        result: Result<TxOutcome>,
        lock: LockFile,
    },
    RevertDone {
        id: String,
        result: Result<Vec<String>>,
        lock: LockFile,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Servers,
    Detail,
    Identify,
    Help,
    Review,
    Restart,
    Applying,
    Versions,
    Search,
    Journal,
}

/// What the version picker is choosing a version for.
#[derive(Debug, Clone)]
pub enum VersionTarget {
    /// Change the version of a plan item (index into plan.items).
    PlanItem(usize),
    /// Update/downgrade an installed plugin.
    Plugin(String),
    /// Install a searched project.
    Install(ProjectLocator, String),
}

#[derive(Default)]
pub struct Flow {
    pub plan: Option<UpdatePlan>,
    pub plan_selected: usize,
    pub restart_choice: usize, // 0 now, 1 when-empty, 2 never
    pub countdown: u32,
    pub backup: bool,
    pub apply_log: Vec<String>,
    pub applying: bool,
    pub last_tx: Option<String>,
    pub versions: Vec<ResolvedVersion>,
    pub versions_loading: bool,
    pub version_selected: usize,
    pub version_target: Option<VersionTarget>,
    pub search_query: String,
    pub search_results: Vec<Candidate>,
    pub search_selected: usize,
    pub searching: bool,
    pub collections: Vec<crate::sources::modrinth::Collection>,
    pub collections_mode: bool,
    pub journal: Vec<JournalEntry>,
    pub journal_selected: usize,
}

pub struct ServerView {
    pub server: Server,
    pub lock: Option<LockFile>,
    pub check: Option<CheckReport>,
    pub undecided: Vec<Undecided>,
    pub status: String,
    pub players: Option<u32>,
    pub busy: Option<&'static str>,
    pub last_error: Option<String>,
}

impl ServerView {
    pub fn new(server: Server) -> Self {
        let lock = LockFile::load(&server.plugins_dir()).ok().flatten();
        Self {
            server,
            lock,
            check: None,
            undecided: Vec::new(),
            status: "…".into(),
            players: None,
            busy: None,
            last_error: None,
        }
    }
}

pub struct State {
    pub servers: Vec<ServerView>,
    pub selected: usize,
    pub plugin_selected: usize,
    pub candidate_selected: usize,
    pub screen: Screen,
    pub log: VecDeque<String>,
    pub sources: Arc<Sources>,
    pub mcsm: Option<Arc<Mcsm>>,
    pub should_quit: bool,
    pub toast: Option<(String, std::time::Instant)>,
    pub loaded: Arc<crate::config::Loaded>,
    pub flow: Flow,
}

impl State {
    pub fn current(&self) -> Option<&ServerView> {
        self.servers.get(self.selected)
    }

    pub fn current_mut(&mut self) -> Option<&mut ServerView> {
        self.servers.get_mut(self.selected)
    }

    pub fn by_id_mut(&mut self, id: &str) -> Option<&mut ServerView> {
        self.servers.iter_mut().find(|v| v.server.id == id)
    }

    pub fn log(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.log.push_back(format!("{} {s}", chrono::Local::now().format("%H:%M:%S")));
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.log(s.clone());
        self.toast = Some((s, std::time::Instant::now()));
    }
}
