use crate::server::{discover, Access, Server};
use crate::Result;

use super::Ctx;

pub async fn run(ctx: &Ctx) -> Result<()> {
    let servers = discover(&ctx.loaded.config);
    let mcsm = ctx.mcsm();
    let mut rows = Vec::new();
    for s in &servers {
        let status = match (&mcsm, s.mcsm_uuid()) {
            (Some(m), Some(uuid)) => match m.status(uuid).await {
                Ok(st) => format!("{st:?}").to_lowercase(),
                Err(e) => format!("api error: {e}"),
            },
            _ => "-".into(),
        };
        let java = match s
            .start_command
            .as_deref()
            .map(crate::server::start_command::StartCommand::parse)
            .and_then(|c| c.java().map(str::to_string))
        {
            Some(j) => crate::server::java::java_major(&j, Some(&s.root))
                .await
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into()),
            None => "?".into(),
        };
        rows.push((s, status, java));
    }
    if ctx.json {
        let v: Vec<serde_json::Value> = rows
            .iter()
            .map(|(s, status, java)| {
                let mut v = serde_json::to_value(s).unwrap_or_default();
                v["status"] = serde_json::Value::String(status.clone());
                v["java"] = serde_json::Value::String(java.clone());
                v
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!(
        "{:<24}{:<12}{:<9}{:<7}{:<6}{:<10}{:<9}path",
        "server", "platform", "mc", "build", "java", "status", "access"
    );
    for (s, status, java) in &rows {
        println!(
            "{:<24}{:<12}{:<9}{:<7}{:<6}{:<10}{:<9}{}",
            s.id,
            s.platform,
            s.jar
                .as_ref()
                .and_then(|j| j.mc_version.as_ref())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into()),
            s.jar.as_ref().and_then(|j| j.build_hint).map(|b| b.to_string()).unwrap_or_else(|| "?".into()),
            java,
            status,
            access_badge(s),
            s.root.display()
        );
        for n in &s.notes {
            println!("{:<24}  note: {n}", "");
        }
    }
    Ok(())
}

fn access_badge(s: &Server) -> &'static str {
    match s.access {
        Access::ReadWrite => "rw",
        Access::ReadOnly { .. } => "ro",
        Access::Missing => "none",
    }
}
