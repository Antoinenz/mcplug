//! Localhost HTTP endpoint for McplugBridge's `/mcplug` command. Minimal HTTP/1.1 over tokio —
//! one route, JSON in, JSON out — so no web framework is needed.
//!
//! Security model: each server's bridge has its own token. A request's token selects the server
//! it may act on; there is no way to name another server. The listener binds 127.0.0.1 only.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::cli::Ctx;
use crate::control::companion::BridgeConfig;
use crate::control::RestartPolicy;
use crate::lockfile::LockFile;
use crate::ops;
use crate::server::{discover, Server};
use crate::transaction::{self, PlanRequest, Progress};

/// Servers the daemon is currently working on (tick or API), so the two never overlap.
pub type Busy = Arc<Mutex<HashSet<String>>>;

pub async fn serve(ctx: Arc<Ctx>, busy: Busy, port: u16) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("daemon api: cannot listen on 127.0.0.1:{port}: {e}");
            return;
        }
    };
    tracing::info!("daemon api listening on 127.0.0.1:{port}");
    loop {
        let Ok((sock, _)) = listener.accept().await else { continue };
        let ctx = ctx.clone();
        let busy = busy.clone();
        tokio::spawn(async move {
            let _ = handle(sock, ctx, busy).await;
        });
    }
}

async fn handle(mut sock: tokio::net::TcpStream, ctx: Arc<Ctx>, busy: Busy) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 16 * 1024 {
            return respond(&mut sock, 431, r#"{"error":"headers too large"}"#).await;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut auth = None;
    let mut len = 0usize;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "authorization" => auth = v.trim().strip_prefix("Bearer ").map(str::to_string),
                "content-length" => len = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if len > 64 * 1024 {
        return respond(&mut sock, 413, r#"{"error":"body too large"}"#).await;
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    if !request_line.starts_with("POST /v1/command") {
        return respond(&mut sock, 404, r#"{"error":"not found"}"#).await;
    }
    let Some(token) = auth else {
        return respond(&mut sock, 401, r#"{"error":"missing token"}"#).await;
    };
    let servers = discover(&ctx.loaded.config);
    let Some(server) = servers
        .iter()
        .find(|s| BridgeConfig::load(&s.plugins_dir()).is_some_and(|c| c.token == token))
        .cloned()
    else {
        return respond(&mut sock, 401, r#"{"error":"unknown token"}"#).await;
    };
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let action = req["action"].as_str().unwrap_or("status").to_string();
    let target = req["target"].as_str().unwrap_or("").to_string();
    let by = req["by"].as_str().unwrap_or("someone").to_string();
    tracing::info!("[{}] /mcplug {action} {target} by {by}", server.id);
    match command(&ctx, busy, server, servers, &action, &target, &by).await {
        Ok(msg) => respond(&mut sock, 200, &serde_json::json!({ "message": msg }).to_string()).await,
        Err(e) => respond(&mut sock, 500, &serde_json::json!({ "error": e.to_string() }).to_string()).await,
    }
}

async fn respond(sock: &mut tokio::net::TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    sock.write_all(
        format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .await?;
    sock.shutdown().await
}

async fn command(ctx: &Arc<Ctx>, busy: Busy, server: Server, _all: Vec<Server>, action: &str, target: &str, by: &str) -> crate::Result<String> {
    let policy = ctx.loaded.config.policy.for_server(&server.id);
    match action {
        "status" => {
            let st = super::DaemonState::load(&super::state::path());
            let s = st.servers.get(&server.id).cloned().unwrap_or_default();
            let lock = LockFile::load(&server.plugins_dir())?.unwrap_or_default();
            let mut lines = vec![format!(
                "{}: {} plugins managed by mcplug",
                server.name,
                lock.plugins.iter().filter(|p| p.source.is_managed()).count()
            )];
            match &s.last_check {
                Some(t) => lines.push(format!("last check {} — {}", humantime_ago(*t), s.last_result.clone().unwrap_or_default())),
                None => lines.push("not checked yet — /mcplug check".into()),
            }
            for u in s.updates_available.iter().take(8) {
                lines.push(format!("  ↑ {u}"));
            }
            if let Some(at) = s.pending_restart {
                lines.push(format!("restart scheduled for {}", at.with_timezone(&chrono::Local).format("%H:%M")));
            }
            Ok(lines.join("\n"))
        }
        "check" => {
            let sources = ctx.sources();
            let out = ops::scan_server(&server, &sources, false).await?;
            let report = ops::check_server(&server, &sources, &out.lock).await?;
            if report.updates.is_empty() {
                return Ok(format!("all {} managed plugins are up to date", report.up_to_date.len()));
            }
            let mut lines = vec![format!(
                "{} update(s) available — /mcplug update all, or /mcplug update <plugin>",
                report.updates.len()
            )];
            for u in &report.updates {
                lines.push(format!(
                    "  ↑ {} {} → {}{}",
                    u.name,
                    u.installed,
                    u.latest.version_number,
                    if u.untested { " (untested)" } else { "" }
                ));
            }
            Ok(lines.join("\n"))
        }
        "update" => {
            {
                let mut b = busy.lock().await;
                if !b.insert(server.id.clone()) {
                    return Ok("mcplug is already working on this server; try again in a minute".into());
                }
            }
            let sources = Arc::new(ctx.sources());
            let result: crate::Result<String> = async {
                let out = ops::scan_server(&server, &sources, false).await?;
                let lock = out.lock;
                let report = ops::check_server(&server, &sources, &lock).await?;
                let names: Vec<String> = if target.is_empty() || target == "all" {
                    report.updates.iter().filter(|u| !u.untested).map(|u| u.name.clone()).collect()
                } else {
                    let t = target.to_ascii_lowercase();
                    lock.plugins
                        .iter()
                        .filter(|p| p.name.to_ascii_lowercase() == t)
                        .map(|p| p.name.clone())
                        .collect()
                };
                if names.is_empty() {
                    return Ok(if target.is_empty() || target == "all" {
                        "nothing to update".to_string()
                    } else {
                        format!("no managed plugin called {target:?}")
                    });
                }
                let (_, cctx) = ops::server_platform(&server)?;
                let plan = transaction::build_plan(
                    &server,
                    &lock,
                    &sources,
                    &cctx,
                    names.into_iter().map(|name| PlanRequest::UpdateLatest { name }).collect(),
                    false,
                )
                .await?;
                if plan.is_empty() {
                    return Ok(plan.notes.first().cloned().unwrap_or_else(|| "nothing to update".into()));
                }
                let summary = plan.summary();
                let restart = RestartPolicy::WhenEmpty {
                    max_wait: std::time::Duration::from_secs(4 * 3600),
                    countdown_secs: policy.countdown.min(30),
                };
                let opts = ops::ApplyOptions {
                    restart,
                    backup: if policy.backup { ctx.mcbackup() } else { None },
                    control: ctx.control(&server),
                };
                let server2 = server.clone();
                let sources2 = sources.clone();
                let busy2 = busy.clone();
                let by = by.to_string();
                tokio::spawn(async move {
                    let mut lock = lock;
                    let sid = server2.id.clone();
                    let progress: transaction::ProgressFn = Arc::new(move |p| {
                        if let Progress::Step(s) = p {
                            tracing::info!("[{sid}] {s}");
                        }
                    });
                    let r = ops::apply_plan(&server2, &mut lock, &sources2, &plan, &opts, progress).await;
                    let msg = match r {
                        Ok(o) => format!("update by {by} done: {} ({} plugin(s))", o.tx_id, o.applied.len()),
                        Err(e) => format!("update by {by} failed: {e}"),
                    };
                    tracing::info!("[{}] {msg}", server2.id);
                    if let Some(cfg) = BridgeConfig::load(&server2.plugins_dir()) {
                        let c = crate::control::companion::Companion::wrap(ctx_control(&server2), cfg, reqwest::Client::new());
                        let _ = c.notify_ops(&msg).await;
                    }
                    busy2.lock().await.remove(&server2.id);
                });
                Ok(format!("updating {summary}. The server restarts once nobody is online (you'll be told)."))
            }
            .await;
            if result.is_err() {
                busy.lock().await.remove(&server.id);
            }
            result
        }
        "restart" => {
            let control = ctx.control(&server);
            if !control.can_restart() {
                return Ok("mcplug cannot restart this server (no panel or restart command configured)".into());
            }
            let secs = policy.countdown.min(60);
            let by = by.to_string();
            tokio::spawn(async move {
                let log = |s: String| tracing::info!("{s}");
                let _ = crate::control::execute_restart(
                    control.as_ref(),
                    &RestartPolicy::Now { countdown_secs: secs },
                    &format!("requested by {by}"),
                    &log,
                )
                .await;
            });
            Ok(format!("restarting in {secs}s"))
        }
        other => Ok(format!("unknown action {other:?}")),
    }
}

fn ctx_control(server: &Server) -> Box<dyn crate::control::ServerControl> {
    Box::new(crate::control::command::NoControl { ping_port: server.ping_port })
}

fn humantime_ago(t: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - t).num_seconds().max(0) as u64;
    format!("{} ago", humantime::format_duration(std::time::Duration::from_secs(secs - secs % 60)))
}
