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

/// Messages from background jobs.
pub enum Msg {
    Status { id: String, status: String, players: Option<u32> },
    ScanDone { id: String, result: Result<ScanOutcome> },
    CheckDone { id: String, result: Result<CheckReport> },
    Log(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Servers,
    Detail,
    Identify,
    Help,
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
        Self { server, lock, check: None, undecided: Vec::new(), status: "…".into(), players: None, busy: None, last_error: None }
    }

    pub fn updates(&self) -> usize {
        self.check.as_ref().map(|c| c.updates.len()).unwrap_or(0)
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
