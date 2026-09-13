use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::backend::Backend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use super::screens;
use super::state::{Msg, Screen, ServerView, State};
use crate::cli::Ctx;
use crate::jobs::JobRunner;
use crate::lockfile::SourceRef;
use crate::ops;
use crate::server::discover;
use crate::server::mcsm::InstanceStatus;
use crate::sources::identify;
use crate::Result;

pub struct App {
    pub state: State,
    pub(super) jobs: JobRunner<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
}

impl App {
    pub fn new(ctx: Ctx) -> Self {
        let (jobs, rx) = JobRunner::new();
        let servers = discover(&ctx.loaded.config).into_iter().map(ServerView::new).collect();
        let mcsm = ctx.mcsm().map(Arc::new);
        let loaded = Arc::new(ctx.loaded.clone());
        let state = State {
            servers,
            selected: 0,
            plugin_selected: 0,
            candidate_selected: 0,
            screen: Screen::Servers,
            log: Default::default(),
            sources: Arc::new(ctx.sources()),
            mcsm,
            should_quit: false,
            toast: None,
            loaded,
            flow: Default::default(),
        };
        Self { state, jobs, rx }
    }

    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<()> {
        self.refresh_status();
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut status_tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            terminal.draw(|f| screens::render(f, &mut self.state))?;
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => self.on_key(k),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => break,
                },
                Some(msg) = self.rx.recv() => self.on_msg(msg),
                _ = tick.tick() => {
                    if self.state.toast.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4)) {
                        self.state.toast = None;
                    }
                }
                _ = status_tick.tick() => self.refresh_status(),
            }
            if self.state.should_quit {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn refresh_status(&self) {
        let Some(mcsm) = self.state.mcsm.clone() else { return };
        for v in &self.state.servers {
            let Some(uuid) = v.server.mcsm_uuid().map(str::to_string) else { continue };
            let id = v.server.id.clone();
            let port = v.server.ping_port;
            let m = mcsm.clone();
            self.jobs.spawn(async move {
                match m.status(&uuid).await {
                    Ok(st) => {
                        let players = if st == InstanceStatus::Running {
                            port.and_then(crate::control::ping::player_count_sync)
                        } else {
                            None
                        };
                        Msg::Status {
                            id,
                            status: format!("{st:?}").to_lowercase(),
                            players,
                        }
                    }
                    Err(e) => Msg::Status {
                        id,
                        status: format!("api error: {e}"),
                        players: None,
                    },
                }
            });
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Status { id, status, players } => {
                if let Some(v) = self.state.by_id_mut(&id) {
                    v.status = status;
                    v.players = players;
                }
            }
            Msg::ScanDone { id, result } => {
                let mut note = None;
                if let Some(v) = self.state.by_id_mut(&id) {
                    v.busy = None;
                    match result {
                        Ok(out) => {
                            note = Some(format!(
                                "{}: {} plugins, {} identified, {} need a decision{}",
                                v.server.name,
                                out.lock.plugins.len(),
                                out.report.identified.len(),
                                out.report.undecided.len(),
                                if out.saved { "" } else { " (read-only: lock not saved)" }
                            ));
                            v.lock = Some(out.lock);
                            v.undecided = out.report.undecided;
                            v.last_error = out.report.errors.first().cloned();
                        }
                        Err(e) => {
                            v.last_error = Some(e.to_string());
                            note = Some(format!("scan failed: {e}"));
                        }
                    }
                }
                if let Some(n) = note {
                    self.state.toast(n);
                }
            }
            Msg::CheckDone { id, result } => {
                let mut note = None;
                if let Some(v) = self.state.by_id_mut(&id) {
                    v.busy = None;
                    match result {
                        Ok(r) => {
                            note = Some(format!("{}: {} update(s) available", v.server.name, r.updates.len()));
                            v.check = Some(r);
                        }
                        Err(e) => note = Some(format!("check failed: {e}")),
                    }
                }
                if let Some(n) = note {
                    self.state.toast(n);
                }
            }
            Msg::PlanBuilt { id, result } => self.on_plan_built(id, result),
            Msg::VersionsLoaded { result } => {
                self.state.flow.versions_loading = false;
                match result {
                    Ok(v) => self.state.flow.versions = v,
                    Err(e) => self.state.toast(format!("could not load versions: {e}")),
                }
            }
            Msg::SearchDone { result } => {
                self.state.flow.searching = false;
                self.state.flow.search_selected = 0;
                match result {
                    Ok(r) => self.state.flow.search_results = r,
                    Err(e) => self.state.toast(format!("search failed: {e}")),
                }
            }
            Msg::CollectionsLoaded { result } => self.on_collections_loaded(result),
            Msg::JarStatus { result } => {
                self.state.flow.jar_loading = false;
                match result {
                    Ok(s) => self.state.flow.jar = Some(s),
                    Err(e) => self.state.toast(format!("server jar: {e}")),
                }
            }
            Msg::JarDone { result } => {
                self.state.flow.applying = false;
                self.state.flow.apply_log.push(match result {
                    Ok(tx) => format!("done: server jar replaced ({tx}). restart the server to run it."),
                    Err(e) => format!("FAILED: {e}"),
                });
                // the server's jar info is stale now; rediscover
                let cfg = self.state.loaded.config.clone();
                for fresh in discover(&cfg) {
                    if let Some(v) = self.state.by_id_mut(&fresh.id) {
                        v.server = fresh;
                    }
                }
            }
            Msg::ApplyProgress(line) => {
                // "<name>: 42%" lines replace the previous one for the same download
                let prefix = line.split_once(": ").map(|(p, _)| format!("{p}: "));
                let log = &mut self.state.flow.apply_log;
                match (prefix, log.last()) {
                    (Some(p), Some(last)) if last.starts_with(&p) && (line.ends_with('%') || line.ends_with(" KB")) => {
                        *log.last_mut().expect("non-empty") = line
                    }
                    _ => log.push(line),
                }
            }
            Msg::ApplyDone { id, result, lock } => self.on_apply_done(id, result, lock),
            Msg::RevertDone { id, result, lock } => self.on_revert_done(id, result, lock),
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.state.should_quit = true;
            return;
        }
        match self.state.screen {
            Screen::Help => {
                self.state.screen = Screen::Servers;
            }
            Screen::Servers => match k.code {
                KeyCode::Char('q') | KeyCode::Esc => self.state.should_quit = true,
                KeyCode::Char('?') => self.state.screen = Screen::Help,
                KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
                KeyCode::Enter | KeyCode::Char('l') => {
                    self.state.plugin_selected = 0;
                    self.state.screen = Screen::Detail;
                }
                KeyCode::Char('s') => self.scan_current(),
                KeyCode::Char('c') => self.check_current(),
                KeyCode::Char('r') => self.refresh_status(),
                KeyCode::Char('C') => {
                    for i in 0..self.state.servers.len() {
                        self.state.selected = i;
                        self.check_current();
                    }
                }
                _ => {}
            },
            Screen::Detail => match k.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') => self.state.screen = Screen::Servers,
                KeyCode::Char('?') => self.state.screen = Screen::Help,
                KeyCode::Char('j') | KeyCode::Down => self.move_plugin(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_plugin(-1),
                KeyCode::Char('s') => self.scan_current(),
                KeyCode::Char('c') => self.check_current(),
                KeyCode::Char('r') => self.refresh_status(),
                KeyCode::Char('i') => self.open_identify(),
                KeyCode::Char('m') => self.toggle_unmanaged(),
                KeyCode::Char('p') => self.toggle_pin(),
                KeyCode::Char('x') => self.ignore_latest(),
                KeyCode::Char('u') => self.start_update_review(false),
                KeyCode::Char('U') => self.start_update_review(true),
                KeyCode::Char('v') => self.open_versions_for_selected(),
                KeyCode::Char('n') | KeyCode::Char('/') => self.open_search(),
                KeyCode::Char('l') => self.load_journal(),
                KeyCode::Char('J') => self.open_jar(),
                _ => {}
            },
            Screen::Jar => self.on_key_jar(k),
            Screen::Review => self.on_key_review(k),
            Screen::Restart => self.on_key_restart(k),
            Screen::Applying => self.on_key_applying(k),
            Screen::Versions => self.on_key_versions(k),
            Screen::Search => self.on_key_search(k),
            Screen::Journal => self.on_key_journal(k),
            Screen::Identify => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => self.state.screen = Screen::Detail,
                KeyCode::Char('j') | KeyCode::Down => self.state.candidate_selected = self.state.candidate_selected.saturating_add(1),
                KeyCode::Char('k') | KeyCode::Up => self.state.candidate_selected = self.state.candidate_selected.saturating_sub(1),
                KeyCode::Enter => self.accept_candidate(),
                KeyCode::Char('m') => {
                    self.toggle_unmanaged();
                    self.state.screen = Screen::Detail;
                }
                _ => {}
            },
        }
    }

    fn move_sel(&mut self, d: i32) {
        let n = self.state.servers.len();
        if n == 0 {
            return;
        }
        self.state.selected = (self.state.selected as i32 + d).rem_euclid(n as i32) as usize;
    }

    fn move_plugin(&mut self, d: i32) {
        let n = self.state.current().and_then(|v| v.lock.as_ref()).map(|l| l.plugins.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        self.state.plugin_selected = (self.state.plugin_selected as i32 + d).rem_euclid(n as i32) as usize;
    }

    fn scan_current(&mut self) {
        let Some(v) = self.state.current_mut() else { return };
        if v.busy.is_some() {
            return;
        }
        v.busy = Some("scanning");
        let server = v.server.clone();
        let id = server.id.clone();
        let sources = self.state.sources.clone();
        self.jobs.spawn(async move {
            let result = ops::scan_server(&server, &sources, false).await;
            Msg::ScanDone { id, result }
        });
    }

    fn check_current(&mut self) {
        let Some(v) = self.state.current_mut() else { return };
        if v.busy.is_some() {
            return;
        }
        let Some(lock) = v.lock.clone() else {
            self.state.toast("scan first (s)");
            return;
        };
        v.busy = Some("checking");
        let server = v.server.clone();
        let id = server.id.clone();
        let sources = self.state.sources.clone();
        self.jobs.spawn(async move {
            let result = ops::check_server(&server, &sources, &lock).await;
            Msg::CheckDone { id, result }
        });
    }

    pub(super) fn selected_plugin_name(&self) -> Option<String> {
        self.state
            .current()?
            .lock
            .as_ref()?
            .plugins
            .get(self.state.plugin_selected)
            .map(|p| p.name.clone())
    }

    fn open_identify(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let has = self
            .state
            .current()
            .is_some_and(|v| v.undecided.iter().any(|u| u.jar.descriptor.as_ref().is_some_and(|d| d.name == name)));
        if !has {
            self.state.toast(format!("{name}: no candidates — run a scan (s) to search again"));
            return;
        }
        self.state.candidate_selected = 0;
        self.state.screen = Screen::Identify;
    }

    fn accept_candidate(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let sel = self.state.candidate_selected;
        let Some(v) = self.state.current_mut() else { return };
        let Some(u) = v.undecided.iter().find(|u| u.jar.descriptor.as_ref().is_some_and(|d| d.name == name)) else {
            return;
        };
        let Some(c) = u.candidates.get(sel) else { return };
        let version = c.version.clone().unwrap_or_else(|| identify_placeholder(&c.project));
        let source = identify::source_ref(&c.project, &version);
        let Some(lock) = v.lock.as_mut() else { return };
        let msg = match ops::set_source(&v.server, lock, &name, source) {
            Ok(()) => format!("{name} → {} ({})", c.project.name, c.project.source),
            Err(e) => format!("could not save: {e}"),
        };
        v.undecided.retain(|u| u.jar.descriptor.as_ref().is_some_and(|d| d.name != name));
        self.state.toast(msg);
        self.state.screen = Screen::Detail;
    }

    fn toggle_unmanaged(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let Some(v) = self.state.current_mut() else { return };
        let Some(lock) = v.lock.as_mut() else { return };
        let Some(e) = lock.get_mut(&name) else { return };
        let new = if matches!(e.source, SourceRef::Unmanaged) {
            SourceRef::Unidentified
        } else {
            SourceRef::Unmanaged
        };
        let label = if matches!(new, SourceRef::Unmanaged) { "unmanaged" } else { "unidentified" };
        let msg = match ops::set_source(&v.server, lock, &name, new) {
            Ok(()) => format!("{name}: now {label}"),
            Err(e) => format!("could not save: {e}"),
        };
        self.state.toast(msg);
    }

    fn toggle_pin(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let Some(v) = self.state.current_mut() else { return };
        let Some(lock) = v.lock.as_mut() else { return };
        let Some(e) = lock.get_mut(&name) else { return };
        e.pinned = !e.pinned;
        let pinned = e.pinned;
        let msg = match lock.save(&v.server.plugins_dir()) {
            Ok(()) => format!("{name}: {}", if pinned { "pinned" } else { "unpinned" }),
            Err(e) => format!("could not save: {e}"),
        };
        self.state.toast(msg);
    }

    fn ignore_latest(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let Some(v) = self.state.current_mut() else { return };
        let Some(latest) = v
            .check
            .as_ref()
            .and_then(|c| c.updates.iter().find(|u| u.name == name))
            .map(|u| u.latest.clone())
        else {
            self.state.toast(format!("{name}: no pending update to ignore"));
            return;
        };
        let Some(lock) = v.lock.as_mut() else { return };
        let Some(e) = lock.get_mut(&name) else { return };
        if !e.ignored_versions.contains(&latest.version_id) {
            e.ignored_versions.push(latest.version_id.clone());
        }
        let msg = match lock.save(&v.server.plugins_dir()) {
            Ok(()) => {
                if let Some(c) = v.check.as_mut() {
                    c.updates.retain(|u| u.name != name);
                }
                format!("{name}: ignoring {}", latest.version_number)
            }
            Err(e) => format!("could not save: {e}"),
        };
        self.state.toast(msg);
    }
}

fn identify_placeholder(p: &crate::sources::ProjectRef) -> crate::sources::ResolvedVersion {
    crate::sources::ResolvedVersion {
        source: p.source,
        project_id: p.id.clone(),
        version_id: String::new(),
        version_number: "unknown".into(),
        channel: crate::lockfile::Channel::Release,
        game_versions: vec![],
        loaders: vec![],
        published: Default::default(),
        files: vec![],
        dependencies: vec![],
        changelog: None,
    }
}
