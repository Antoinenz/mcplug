use std::sync::Arc;

use crate::lockfile::LockFile;
use crate::serverjar::{self, ops as jarops};
use crate::sources::Compat;
use crate::{Error, Result};

use super::update::FlowArgs;
use super::Ctx;

pub struct JarArgs {
    pub server: String,
    pub update: bool,
    pub mc: Option<String>,
    pub check_only: bool,
    pub yes: bool,
    pub flow: FlowArgs,
}

pub async fn run(ctx: &Ctx, a: &JarArgs) -> Result<()> {
    let server = ctx.server(&a.server)?;
    let provider = serverjar::for_platform(ctx.http.clone(), server.platform)
        .ok_or_else(|| Error::Msg(format!("{}: no build provider for {}", server.name, server.platform)))?;
    let st = jarops::status(&server, provider.as_ref()).await?;
    println!(
        "{}: {} {} — {} {}",
        server.name,
        server.platform,
        st.mc,
        st.file,
        match &st.installed {
            Some(b) => format!(
                "(build {}{})",
                b.build,
                b.time.map(|t| format!(", {}", t.format("%Y-%m-%d"))).unwrap_or_default()
            ),
            None => "(build not recognised — not a published build of this version?)".into(),
        }
    );
    if let Some(l) = &st.latest_same_mc {
        let cur = st.installed.as_ref().map(|b| b.build);
        if cur == Some(l.build) {
            println!("  build: up to date");
        } else {
            println!(
                "  build: {} available{} — `mcplug jar {} --update`",
                l.build,
                l.time.map(|t| format!(" ({})", t.format("%Y-%m-%d"))).unwrap_or_default(),
                server.id
            );
        }
    }
    if let Some(n) = &st.newest_mc {
        if n > &st.mc {
            println!(
                "  minecraft: {n} available — `mcplug jar {} --mc {n} --check-only` to see plugin compatibility",
                server.id
            );
        }
    }
    println!("  java: {}", st.java_major.map(|j| j.to_string()).unwrap_or_else(|| "?".into()));

    let target: Option<serverjar::BuildInfo> = if let Some(mc) = &a.mc {
        let mc = crate::util::McVersion::parse(mc).ok_or_else(|| Error::Msg(format!("{mc}: not a Minecraft version")))?;
        let lock = LockFile::load(&server.plugins_dir())?.unwrap_or_default();
        let (_, cctx) = crate::ops::server_platform(&server)?;
        let sources = ctx.sources();
        println!("\nplugin compatibility with {mc}:");
        let compat = jarops::plugin_compat(&lock, &sources, &cctx, &mc).await;
        let mut blockers = 0;
        for c in &compat {
            let (mark, note) = match c.compat {
                Compat::Exact => ("✓", "declares support"),
                Compat::Lenient | Compat::SameLineOnly => ("~", "same line, untested"),
                Compat::Unknown => ("?", "no version metadata"),
                Compat::Incompatible => {
                    blockers += 1;
                    ("✗", "no compatible version")
                }
            };
            println!("  {mark} {:<22} {:<20} {:<20} {note}", c.name, c.current, c.best.clone().unwrap_or_default());
        }
        let build = provider.latest_build(&mc).await?;
        if let Some(problem) = jarops::java_check(&server, build.java_min).await? {
            println!("\n  ! {problem}");
            if !a.check_only {
                return Err(Error::Msg("fix the Java version first".into()));
            }
        }
        if blockers > 0 {
            println!("\n  {blockers} plugin(s) have no version for {mc}; they may still work, or break. Your call.");
        }
        if a.check_only {
            return Ok(());
        }
        Some(build)
    } else if a.update {
        match st.latest_same_mc {
            Some(l) if st.installed.as_ref().map(|b| b.build) != Some(l.build) => Some(l),
            _ => {
                println!("nothing to do");
                return Ok(());
            }
        }
    } else {
        None
    };
    let Some(build) = target else { return Ok(()) };

    let opts = super::update::apply_options(ctx, &server, &a.flow)?;
    println!("\n  {} → {} build {} ({})", st.file, build.mc, build.build, build.file_name);
    if !a.yes && !super::update::confirm("apply?")? {
        return Ok(());
    }
    let progress: crate::transaction::ProgressFn = Arc::new(|p| {
        if let crate::transaction::Progress::Step(s) = p {
            eprintln!("· {s}");
        }
    });
    let summary = format!("{} {} build {}", server.platform, build.mc, build.build);
    if let Some(b) = &opts.backup {
        eprintln!("· mcbackup checkpoint {}", server.backup_slug);
        b.checkpoint(&server.backup_slug, &format!("before: {summary}")).await?;
    }
    let tx = jarops::install_build(&server, opts.control.as_ref(), &build, &ctx.http, progress.clone()).await?;
    let log = |s: String| eprintln!("· {s}");
    crate::control::execute_restart(opts.control.as_ref(), &opts.restart, &summary, &log).await?;
    if let (Some(b), false) = (&opts.backup, opts.restart == crate::control::RestartPolicy::Never) {
        eprintln!("· mcbackup backup {}", server.backup_slug);
        let _ = b.backup(&server.backup_slug, &format!("after: {summary}")).await;
    }
    println!("done ({tx}).");
    Ok(())
}
