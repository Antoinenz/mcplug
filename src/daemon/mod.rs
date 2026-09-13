//! `mcplug daemon`: periodic checks and policy-driven automatic updates.

pub mod state;

use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, NaiveTime, Timelike};

use crate::cli::Ctx;
use crate::config::EffectivePolicy;
use crate::control::RestartPolicy;
use crate::lockfile::{LockFile, SourceRef};
use crate::ops;
use crate::server::{discover, Server};
use crate::sources::resolve::UpdateCandidate;
use crate::transaction::journal::{self, JournalEntry};
use crate::transaction::{self, PlanRequest, Progress};
use crate::Result;

pub use state::DaemonState;

pub async fn run(ctx: Ctx) -> Result<()> {
    let state_path = state::path();
    let _lock = state::acquire_lock()?;
    let mut st = DaemonState::load(&state_path);
    tracing::info!("daemon starting; state in {}", state_path.display());
    loop {
        let started = std::time::Instant::now();
        if let Err(e) = tick(&ctx, &mut st).await {
            tracing::error!("tick failed: {e}");
        }
        st.heartbeat = Some(chrono::Utc::now());
        st.pid = std::process::id();
        let _ = st.save(&state_path);
        let elapsed = started.elapsed();
        tokio::time::sleep(Duration::from_secs(60).saturating_sub(elapsed)).await;
    }
}

async fn tick(ctx: &Ctx, st: &mut DaemonState) -> Result<()> {
    let loaded = crate::config::Loaded::load(Some(ctx.loaded.dir.clone())).unwrap_or_else(|_| ctx.loaded.clone());
    let ctx = Ctx {
        loaded,
        http: ctx.http.clone(),
        json: false,
    };
    let sources = Arc::new(ctx.sources());
    let now = chrono::Utc::now();
    for server in discover(&ctx.loaded.config) {
        let policy = ctx.loaded.config.policy.for_server(&server.id);
        if !policy.enabled {
            continue;
        }
        let entry = st.servers.entry(server.id.clone()).or_default();

        // pending scheduled restart?
        if let Some(at) = entry.pending_restart {
            if now >= at && !in_quiet_hours(&policy) {
                let control = ctx.control(&server);
                let log = |s: String| tracing::info!("[{}] {s}", server.id);
                match crate::control::execute_restart(
                    control.as_ref(),
                    &RestartPolicy::Now {
                        countdown_secs: policy.countdown,
                    },
                    "scheduled restart for updates",
                    &log,
                )
                .await
                {
                    Ok(()) => note(&server, "restart", "restarted (scheduled)".into(), None),
                    Err(e) => note(&server, "restart", format!("scheduled restart failed: {e}"), None),
                }
                entry.pending_restart = None;
            }
        }

        let due = entry
            .last_check
            .is_none_or(|t| now - t >= chrono::Duration::from_std(policy.check_interval).unwrap_or(chrono::Duration::hours(6)));
        if !due {
            continue;
        }
        if !server.access.writable() {
            if entry.last_warning.is_none_or(|t| now - t > chrono::Duration::days(1)) {
                tracing::warn!("[{}] plugin dir not writable; skipping", server.id);
                entry.last_warning = Some(now);
            }
            continue;
        }
        let Ok(out) = ops::scan_server(&server, &sources, false).await else {
            continue;
        };
        let lock = out.lock;
        entry.last_check = Some(now);
        let report = match ops::check_server(&server, &sources, &lock).await {
            Ok(r) => r,
            Err(e) => {
                entry.last_result = Some(format!("check failed: {e}"));
                continue;
            }
        };
        entry.updates_available = report
            .updates
            .iter()
            .map(|u| format!("{} {}→{}", u.name, u.installed, u.latest.version_number))
            .collect();
        entry.last_result = Some(format!("{} update(s) available", report.updates.len()));
        tracing::info!("[{}] {} update(s) available", server.id, report.updates.len());
        if report.updates.is_empty() || policy.auto_apply == "none" {
            continue;
        }
        let selected: Vec<&UpdateCandidate> = report.updates.iter().filter(|u| auto_selectable(u, &lock, &policy)).collect();
        if selected.is_empty() {
            continue;
        }
        if in_quiet_hours(&policy) {
            tracing::info!("[{}] quiet hours; deferring {} update(s)", server.id, selected.len());
            entry.last_result = Some(format!("{} update(s) deferred (quiet hours)", selected.len()));
            entry.last_check = None; // re-evaluate next tick
            continue;
        }
        let (_, cctx) = ops::server_platform(&server)?;
        let requests = selected.iter().map(|u| PlanRequest::UpdateLatest { name: u.name.clone() }).collect();
        let plan = match transaction::build_plan(&server, &lock, &sources, &cctx, requests, false).await {
            Ok(p) => p,
            Err(e) => {
                note(&server, "aborted", format!("auto-update plan failed: {e}"), None);
                continue;
            }
        };
        if plan.is_empty() {
            continue;
        }
        let restart = match policy.restart.as_str() {
            "now" => RestartPolicy::Now {
                countdown_secs: policy.countdown,
            },
            "when-empty" => RestartPolicy::WhenEmpty {
                max_wait: Duration::from_secs(4 * 3600),
                countdown_secs: policy.countdown,
            },
            _ => RestartPolicy::Never,
        };
        let control = ctx.control(&server);
        let restart = if restart != RestartPolicy::Never && !control.can_restart() {
            RestartPolicy::Never
        } else {
            restart
        };
        let opts = ops::ApplyOptions {
            restart: restart.clone(),
            backup: if policy.backup { ctx.mcbackup() } else { None },
            control,
        };
        let sid = server.id.clone();
        let progress: transaction::ProgressFn = Arc::new(move |p| {
            if let Progress::Step(s) = p {
                tracing::info!("[{sid}] {s}");
            }
        });
        let mut lock = lock;
        match ops::apply_plan(&server, &mut lock, &sources, &plan, &opts, progress).await {
            Ok(o) => {
                entry.last_result = Some(format!("applied {} ({} plugin(s))", o.tx_id, o.applied.len()));
                entry.updates_available.clear();
                if policy.restart == "scheduled" {
                    let at = next_local_time(&policy.restart_at);
                    entry.pending_restart = Some(at);
                    note(
                        &server,
                        "restart",
                        format!("scheduled for {}", at.with_timezone(&Local).format("%Y-%m-%d %H:%M")),
                        None,
                    );
                }
            }
            Err(e) => entry.last_result = Some(format!("auto-update failed: {e}")),
        }
    }
    Ok(())
}

fn auto_selectable(u: &UpdateCandidate, lock: &LockFile, policy: &EffectivePolicy) -> bool {
    if u.unverified || u.latest.channel != crate::lockfile::Channel::Release {
        return false;
    }
    let is_geyser = lock.get(&u.name).is_some_and(|e| matches!(e.source, SourceRef::GeyserMc { .. })) || u.name.to_ascii_lowercase().contains("geyser");
    if is_geyser && !policy.geyser_auto {
        return false;
    }
    match policy.auto_apply.as_str() {
        "all" => true,
        "same-mc-release" => !u.untested,
        _ => false,
    }
}

fn note(server: &Server, action: &str, outcome: String, note: Option<String>) {
    let _ = journal::append(
        &server.plugins_dir(),
        &JournalEntry {
            time: chrono::Utc::now(),
            tx_id: "daemon".into(),
            action: action.into(),
            outcome,
            items: vec![],
            note,
        },
    );
}

pub fn in_quiet_hours(policy: &EffectivePolicy) -> bool {
    let now = Local::now().time();
    policy.quiet_hours.iter().any(|range| {
        let Some((a, b)) = range.split_once('-') else { return false };
        let (Ok(a), Ok(b)) = (NaiveTime::parse_from_str(a.trim(), "%H:%M"), NaiveTime::parse_from_str(b.trim(), "%H:%M")) else {
            return false;
        };
        if a <= b {
            now >= a && now < b
        } else {
            now >= a || now < b // wraps midnight
        }
    })
}

/// The next occurrence of `HH:MM` local time.
pub fn next_local_time(hhmm: &str) -> chrono::DateTime<chrono::Utc> {
    let t = NaiveTime::parse_from_str(hhmm, "%H:%M").unwrap_or_else(|_| NaiveTime::from_hms_opt(4, 30, 0).expect("valid"));
    let now = Local::now();
    let mut candidate = now.date_naive().and_time(t);
    if candidate <= now.naive_local() || (now.hour() == t.hour() && now.minute() >= t.minute()) {
        candidate += chrono::Duration::days(1);
    }
    candidate.and_local_timezone(Local).single().unwrap_or(now).with_timezone(&chrono::Utc)
}
