//! Key handling and jobs for the update / install / revert flow.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::state::{Msg, Screen, VersionTarget};
use crate::control::RestartPolicy;
use crate::lockfile::LockFile;
use crate::ops;
use crate::sources::{CompatCtx, ProjectLocator};
use crate::transaction::{self, PlanRequest, Progress};

impl App {
    fn cctx(&self) -> Option<CompatCtx> {
        ops::server_platform(&self.state.current()?.server).ok().map(|(_, c)| c)
    }

    /// `u` on the plugin table: plan updates for every plugin with an update (or just the selected one if it has one).
    pub fn start_update_review(&mut self, only_selected: bool) {
        let Some(v) = self.state.current() else { return };
        let Some(check) = &v.check else {
            self.state.toast("check for updates first (c)");
            return;
        };
        let selected = self.selected_plugin_name();
        let names: Vec<String> = check
            .updates
            .iter()
            .filter(|u| !only_selected || Some(&u.name) == selected.as_ref())
            .filter(|u| !u.untested || only_selected)
            .map(|u| u.name.clone())
            .collect();
        if names.is_empty() {
            self.state.toast(if only_selected { "no update available for this plugin (v picks any version)" } else { "nothing to update" });
            return;
        }
        self.build_plan(names.into_iter().map(|name| PlanRequest::UpdateLatest { name }).collect());
    }

    pub fn build_plan(&mut self, requests: Vec<PlanRequest>) {
        let Some(cctx) = self.cctx() else { return };
        let Some(v) = self.state.current_mut() else { return };
        let Some(lock) = v.lock.clone() else { return };
        v.busy = Some("planning");
        let server = v.server.clone();
        let id = server.id.clone();
        let sources = self.state.sources.clone();
        self.jobs.spawn(async move {
            let result = transaction::build_plan(&server, &lock, &sources, &cctx, requests, true).await;
            Msg::PlanBuilt { id, result }
        });
    }

    pub fn on_plan_built(&mut self, id: String, result: crate::Result<transaction::UpdatePlan>) {
        if let Some(v) = self.state.by_id_mut(&id) {
            v.busy = None;
        }
        match result {
            Ok(plan) if plan.is_empty() => self.state.toast(plan.notes.first().cloned().unwrap_or_else(|| "nothing to do".into())),
            Ok(plan) => {
                self.state.flow.plan = Some(plan);
                self.state.flow.plan_selected = 0;
                self.state.screen = Screen::Review;
            }
            Err(e) => self.state.toast(format!("plan failed: {e}")),
        }
    }

    pub fn on_key_review(&mut self, k: KeyEvent) {
        let n = self.state.flow.plan.as_ref().map(|p| p.items.len()).unwrap_or(0);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state.flow.plan = None;
                self.state.screen = Screen::Detail;
            }
            KeyCode::Char('j') | KeyCode::Down if n > 0 => self.state.flow.plan_selected = (self.state.flow.plan_selected + 1) % n,
            KeyCode::Char('k') | KeyCode::Up if n > 0 => self.state.flow.plan_selected = (self.state.flow.plan_selected + n - 1) % n,
            KeyCode::Char('-') | KeyCode::Delete if n > 0 => {
                if let Some(p) = self.state.flow.plan.as_mut() {
                    p.items.remove(self.state.flow.plan_selected);
                    if p.items.is_empty() {
                        self.state.flow.plan = None;
                        self.state.screen = Screen::Detail;
                        return;
                    }
                    self.state.flow.plan_selected = self.state.flow.plan_selected.min(p.items.len() - 1);
                }
            }
            KeyCode::Char('v') if n > 0 => {
                let idx = self.state.flow.plan_selected;
                let item = self.state.flow.plan.as_ref().map(|p| p.items[idx].clone());
                if let Some(item) = item {
                    let locator = ProjectLocator { source: item.project.source, id: item.project.id.clone() };
                    self.open_versions(locator, VersionTarget::PlanItem(idx));
                }
            }
            KeyCode::Enter => {
                self.state.flow.restart_choice = 0;
                self.state.flow.countdown = 60;
                self.state.flow.backup = crate::backup::Mcbackup::detect(&self.state.loaded.config.backup.mcbackup).is_some();
                self.state.screen = Screen::Restart;
            }
            _ => {}
        }
    }

    pub fn on_key_restart(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => self.state.screen = Screen::Review,
            KeyCode::Char('j') | KeyCode::Down => self.state.flow.restart_choice = (self.state.flow.restart_choice + 1) % 3,
            KeyCode::Char('k') | KeyCode::Up => self.state.flow.restart_choice = (self.state.flow.restart_choice + 2) % 3,
            KeyCode::Char('+') | KeyCode::Char('l') | KeyCode::Right => self.state.flow.countdown = (self.state.flow.countdown + 15).min(600),
            KeyCode::Char('-') | KeyCode::Char('h') | KeyCode::Left => self.state.flow.countdown = self.state.flow.countdown.saturating_sub(15),
            KeyCode::Char('b') | KeyCode::Char(' ') => self.state.flow.backup = !self.state.flow.backup,
            KeyCode::Enter => self.start_apply(),
            _ => {}
        }
    }

    fn start_apply(&mut self) {
        let Some(plan) = self.state.flow.plan.clone() else { return };
        let Some(v) = self.state.current() else { return };
        let Some(lock) = v.lock.clone() else { return };
        let server = v.server.clone();
        let id = server.id.clone();
        let restart = match self.state.flow.restart_choice {
            0 => RestartPolicy::Now { countdown_secs: self.state.flow.countdown },
            1 => RestartPolicy::WhenEmpty { max_wait: std::time::Duration::from_secs(12 * 3600), countdown_secs: self.state.flow.countdown },
            _ => RestartPolicy::Never,
        };
        let control = crate::control::for_server(&server, self.state.mcsm.as_deref().cloned(), &self.state.loaded.secrets);
        if restart != RestartPolicy::Never && !control.can_restart() {
            self.state.toast(format!("cannot restart this server (control: {}); choose 'never'", control.name()));
            return;
        }
        let backup = if self.state.flow.backup { crate::backup::Mcbackup::detect(&self.state.loaded.config.backup.mcbackup) } else { None };
        let opts = ops::ApplyOptions { restart, backup, control };
        let sources = self.state.sources.clone();
        let tx = self.jobs.clone();
        self.state.flow.apply_log.clear();
        self.state.flow.applying = true;
        self.state.flow.last_tx = Some(plan.tx_id.clone());
        self.state.screen = Screen::Applying;
        if let Some(v) = self.state.current_mut() {
            v.busy = Some("applying");
        }
        self.jobs.spawn(async move {
            let mut lock = lock;
            let progress: transaction::ProgressFn = Arc::new(move |p| {
                let line = match p {
                    Progress::Step(s) => s,
                    Progress::Download { name, done, total: Some(t) } => format!("{name}: {}%", done * 100 / t.max(1)),
                    Progress::Download { name, done, total: None } => format!("{name}: {} KB", done / 1000),
                };
                tx.send(Msg::ApplyProgress(line));
            });
            let result = ops::apply_plan(&server, &mut lock, &sources, &plan, &opts, progress).await;
            Msg::ApplyDone { id, result, lock }
        });
    }

    pub fn on_apply_done(&mut self, id: String, result: crate::Result<transaction::TxOutcome>, lock: LockFile) {
        self.state.flow.applying = false;
        let line = match &result {
            Ok(o) => format!("done: transaction {} applied ({} plugin(s))", o.tx_id, o.applied.len()),
            Err(e) => format!("FAILED: {e}"),
        };
        self.state.flow.apply_log.push(line);
        // Only offer "revert" when the jars were actually swapped.
        let tx = self.state.flow.last_tx.clone().unwrap_or_default();
        let applied = self.state.by_id_mut(&id).map(|v| transaction::journal::read(&v.server.plugins_dir()).iter().any(|e| e.tx_id == tx && e.outcome == "applied"));
        if applied != Some(true) {
            self.state.flow.last_tx = None;
        }
        if let Some(v) = self.state.by_id_mut(&id) {
            v.busy = None;
            v.lock = LockFile::load(&v.server.plugins_dir()).ok().flatten().or(Some(lock));
            v.check = None; // stale after changes
        }
        self.state.flow.plan = None;
        self.refresh_status();
    }

    pub fn on_key_applying(&mut self, k: KeyEvent) {
        if self.state.flow.applying {
            return; // can't interrupt a transaction from the keyboard
        }
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.state.screen = Screen::Detail,
            KeyCode::Char('r') => {
                if let Some(tx) = self.state.flow.last_tx.clone() {
                    self.revert(tx);
                }
            }
            _ => {}
        }
    }

    pub fn revert(&mut self, tx: String) {
        let Some(v) = self.state.current_mut() else { return };
        let Some(lock) = v.lock.clone() else { return };
        v.busy = Some("reverting");
        let server = v.server.clone();
        let id = server.id.clone();
        self.jobs.spawn(async move {
            let mut lock = lock;
            let result = transaction::revert(&server, &mut lock, &tx);
            Msg::RevertDone { id, result, lock }
        });
    }

    pub fn on_revert_done(&mut self, id: String, result: crate::Result<Vec<String>>, lock: LockFile) {
        let msg = match &result {
            Ok(names) => format!("reverted: {} (restart the server to load the old jars)", names.join(", ")),
            Err(e) => format!("revert failed: {e}"),
        };
        if let Some(v) = self.state.by_id_mut(&id) {
            v.busy = None;
            v.lock = Some(lock);
            v.check = None;
        }
        self.state.toast(msg);
        if self.state.screen == Screen::Journal {
            self.load_journal();
        }
    }

    // ---- version picker ----

    pub fn open_versions(&mut self, locator: ProjectLocator, target: VersionTarget) {
        let Some(cctx) = self.cctx() else { return };
        self.state.flow.versions.clear();
        self.state.flow.version_selected = 0;
        self.state.flow.versions_loading = true;
        self.state.flow.version_target = Some(target);
        self.state.screen = Screen::Versions;
        let sources = self.state.sources.clone();
        self.jobs.spawn(async move {
            let result = match sources.get(locator.source) {
                Some(src) => src.versions(&locator.id, &cctx).await,
                None => Err(crate::Error::Msg(format!("source {} is disabled", locator.source))),
            };
            Msg::VersionsLoaded { result }
        });
    }

    /// `v` on the plugin table: any version of the selected plugin.
    pub fn open_versions_for_selected(&mut self) {
        let Some(name) = self.selected_plugin_name() else { return };
        let Some(entry) = self.state.current().and_then(|v| v.lock.as_ref()).and_then(|l| l.get(&name)).cloned() else { return };
        use crate::lockfile::SourceRef;
        use crate::sources::SourceKind;
        let locator = match &entry.source {
            SourceRef::Modrinth { project_id, .. } => ProjectLocator { source: SourceKind::Modrinth, id: project_id.clone() },
            SourceRef::Hangar { slug, .. } => ProjectLocator { source: SourceKind::Hangar, id: slug.clone() },
            SourceRef::GitHub { owner, repo, .. } => ProjectLocator { source: SourceKind::GitHub, id: format!("{owner}/{repo}") },
            SourceRef::GeyserMc { project, .. } => ProjectLocator { source: SourceKind::GeyserMc, id: project.clone() },
            _ => {
                self.state.toast(format!("{name}: identify it first (i)"));
                return;
            }
        };
        self.open_versions(locator, VersionTarget::Plugin(name));
    }

    pub fn on_key_versions(&mut self, k: KeyEvent) {
        let n = self.state.flow.versions.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state.screen = match self.state.flow.version_target {
                    Some(VersionTarget::PlanItem(_)) => Screen::Review,
                    Some(VersionTarget::Install(..)) => Screen::Search,
                    _ => Screen::Detail,
                };
            }
            KeyCode::Char('j') | KeyCode::Down if n > 0 => self.state.flow.version_selected = (self.state.flow.version_selected + 1) % n,
            KeyCode::Char('k') | KeyCode::Up if n > 0 => self.state.flow.version_selected = (self.state.flow.version_selected + n - 1) % n,
            KeyCode::Enter if n > 0 => {
                let v = self.state.flow.versions[self.state.flow.version_selected].clone();
                match self.state.flow.version_target.clone() {
                    Some(VersionTarget::PlanItem(idx)) => {
                        if let Some(p) = self.state.flow.plan.as_mut() {
                            if let Some(item) = p.items.get_mut(idx) {
                                if let Some(f) = v.primary_file().cloned() {
                                    item.file = f;
                                    item.to = v;
                                }
                            }
                        }
                        self.state.screen = Screen::Review;
                    }
                    Some(VersionTarget::Plugin(name)) => self.build_plan(vec![PlanRequest::UpdateTo { name, version_id: v.version_id }]),
                    Some(VersionTarget::Install(locator, _)) => self.build_plan(vec![PlanRequest::Install { locator, version_id: Some(v.version_id) }]),
                    None => {}
                }
            }
            _ => {}
        }
    }

    // ---- search / install ----

    pub fn open_search(&mut self) {
        self.state.flow.search_query.clear();
        self.state.flow.search_results.clear();
        self.state.flow.search_selected = 0;
        self.state.flow.collections_mode = false;
        self.state.screen = Screen::Search;
    }

    /// Ctrl-L in search: list the signed-in Modrinth user's collections; Enter on one lists its projects.
    fn load_collections(&mut self) {
        let Some(token) = self.state.loaded.secrets.modrinth_token.clone().filter(|t| !t.is_empty()) else {
            self.state.toast("no Modrinth token — run `mcplug auth modrinth <token>` (needs COLLECTION_READ + USER_READ)");
            return;
        };
        self.state.flow.searching = true;
        self.state.flow.search_results.clear();
        let http = self.state.sources.get(crate::sources::SourceKind::Modrinth).map(|s| s.http()).unwrap_or_default();
        self.jobs.spawn(async move {
            let m = crate::sources::modrinth::Modrinth::new(http, Some(token));
            Msg::CollectionsLoaded { result: m.my_collections().await }
        });
    }

    pub fn on_collections_loaded(&mut self, result: crate::Result<Vec<crate::sources::modrinth::Collection>>) {
        self.state.flow.searching = false;
        match result {
            Ok(c) if c.is_empty() => self.state.toast("no collections on this Modrinth account"),
            Ok(c) => {
                self.state.flow.collections = c;
                self.state.flow.collections_mode = true;
                self.state.flow.search_selected = 0;
                self.state.flow.search_query = "(collections — Enter to open one)".into();
            }
            Err(e) => self.state.toast(format!("collections: {e}")),
        }
    }

    fn open_collection(&mut self, idx: usize) {
        let Some(c) = self.state.flow.collections.get(idx).cloned() else { return };
        let Some(token) = self.state.loaded.secrets.modrinth_token.clone() else { return };
        self.state.flow.collections_mode = false;
        self.state.flow.searching = true;
        self.state.flow.search_query = format!("collection: {}", c.name);
        let http = self.state.sources.get(crate::sources::SourceKind::Modrinth).map(|s| s.http()).unwrap_or_default();
        self.jobs.spawn(async move {
            let m = crate::sources::modrinth::Modrinth::new(http, Some(token));
            let result = m.projects(&c.project_ids).await.map(|ps| ps.into_iter().map(|p| crate::sources::Candidate { project: p, confidence: crate::sources::Confidence::NameMatch, version: None }).collect());
            Msg::SearchDone { result }
        });
    }

    pub fn on_key_search(&mut self, k: KeyEvent) {
        if self.state.flow.collections_mode {
            let n = self.state.flow.collections.len();
            match k.code {
                KeyCode::Esc => self.open_search(),
                KeyCode::Down | KeyCode::Char('j') if n > 0 => self.state.flow.search_selected = (self.state.flow.search_selected + 1) % n,
                KeyCode::Up | KeyCode::Char('k') if n > 0 => self.state.flow.search_selected = (self.state.flow.search_selected + n - 1) % n,
                KeyCode::Enter if n > 0 => self.open_collection(self.state.flow.search_selected),
                _ => {}
            }
            return;
        }
        let n = self.state.flow.search_results.len();
        if k.code == KeyCode::Char('l') && k.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
            self.load_collections();
            return;
        }
        match k.code {
            KeyCode::Esc => self.state.screen = Screen::Detail,
            KeyCode::Down => {
                if n > 0 {
                    self.state.flow.search_selected = (self.state.flow.search_selected + 1) % n;
                }
            }
            KeyCode::Up => {
                if n > 0 {
                    self.state.flow.search_selected = (self.state.flow.search_selected + n - 1) % n;
                }
            }
            KeyCode::Backspace => {
                self.state.flow.search_query.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => self.state.flow.search_query.push(c),
            KeyCode::Enter => {
                let q = self.state.flow.search_query.trim().to_string();
                if q.is_empty() {
                    return;
                }
                // A URL / owner/repo / prefixed id goes straight to the version picker.
                if q.starts_with("http") || q.contains(':') || (q.matches('/').count() == 1 && !q.contains(' ')) {
                    match crate::cli::update::locate(&self.state.sources, &q) {
                        Ok(loc) => {
                            let label = loc.id.clone();
                            self.open_versions(loc.clone(), VersionTarget::Install(loc, label));
                        }
                        Err(e) => self.state.toast(e.to_string()),
                    }
                    return;
                }
                if n > 0 && !self.state.flow.searching && self.state.flow.search_results.get(self.state.flow.search_selected).is_some() {
                    let c = self.state.flow.search_results[self.state.flow.search_selected].clone();
                    let loc = ProjectLocator { source: c.project.source, id: c.project.id.clone() };
                    self.open_versions(loc.clone(), VersionTarget::Install(loc, c.project.name.clone()));
                    return;
                }
                self.run_search(q);
            }
            KeyCode::Tab => {
                let q = self.state.flow.search_query.trim().to_string();
                if !q.is_empty() {
                    self.run_search(q);
                }
            }
            _ => {}
        }
    }

    fn run_search(&mut self, q: String) {
        let Some(cctx) = self.cctx() else { return };
        self.state.flow.searching = true;
        self.state.flow.search_results.clear();
        let sources = self.state.sources.clone();
        self.jobs.spawn(async move {
            let mut all = Vec::new();
            let mut err = None;
            for src in &sources.list {
                match src.search(&q, &cctx, None).await {
                    Ok(c) => all.extend(c),
                    Err(e) => err = Some(e),
                }
            }
            all.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(b.project.downloads.cmp(&a.project.downloads)));
            Msg::SearchDone { result: if all.is_empty() && err.is_some() { Err(err.expect("checked")) } else { Ok(all) } }
        });
    }

    // ---- journal ----

    pub fn load_journal(&mut self) {
        let Some(v) = self.state.current() else { return };
        self.state.flow.journal = transaction::journal::read(&v.server.plugins_dir());
        self.state.flow.journal.reverse();
        self.state.flow.journal_selected = 0;
        self.state.screen = Screen::Journal;
    }

    pub fn on_key_journal(&mut self, k: KeyEvent) {
        let n = self.state.flow.journal.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.state.screen = Screen::Detail,
            KeyCode::Char('j') | KeyCode::Down if n > 0 => self.state.flow.journal_selected = (self.state.flow.journal_selected + 1) % n,
            KeyCode::Char('k') | KeyCode::Up if n > 0 => self.state.flow.journal_selected = (self.state.flow.journal_selected + n - 1) % n,
            KeyCode::Char('r') if n > 0 => {
                let e = &self.state.flow.journal[self.state.flow.journal_selected];
                if e.outcome == "applied" {
                    let tx = e.tx_id.clone();
                    self.revert(tx);
                } else {
                    self.state.toast("only applied update/install entries can be reverted");
                }
            }
            _ => {}
        }
    }
}
