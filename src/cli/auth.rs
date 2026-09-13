use crate::{Error, Result};

use super::Ctx;

/// `mcplug auth modrinth <token>` etc. Stored in secrets.toml (0600).
pub async fn set(ctx: &Ctx, what: &str, value: Option<&str>) -> Result<()> {
    let mut loaded = ctx.loaded.clone();
    let value = value.map(str::to_string).filter(|v| !v.is_empty());
    match what {
        "modrinth" => loaded.secrets.modrinth_token = value,
        "github" => loaded.secrets.github_token = value,
        "mcsm" | "mcsmanager" => loaded.secrets.mcsm_api_key = value,
        other => {
            if let Some(name) = other.strip_prefix("rcon/") {
                match value {
                    Some(v) => {
                        loaded.secrets.rcon.insert(name.to_string(), v);
                    }
                    None => {
                        loaded.secrets.rcon.remove(name);
                    }
                }
            } else {
                return Err(Error::Msg(format!("unknown secret {what:?}: use modrinth, github, mcsm or rcon/<server-id>")));
            }
        }
    }
    loaded.save_secrets()?;
    println!("saved to {}", loaded.dir.join("secrets.toml").display());
    if what == "modrinth" {
        if let Some(t) = &loaded.secrets.modrinth_token {
            let m = crate::sources::modrinth::Modrinth::new(ctx.http.clone(), Some(t.clone()));
            match m.my_collections().await {
                Ok(c) => println!("token works: {} collection(s) visible", c.len()),
                Err(e) => println!("warning: could not list collections with this token ({e}); it needs COLLECTION_READ + USER_READ"),
            }
        }
    }
    Ok(())
}

pub fn status(ctx: &Ctx) -> Result<()> {
    let s = &ctx.loaded.secrets;
    let yn = |o: &Option<String>| if o.as_ref().is_some_and(|v| !v.is_empty()) { "set" } else { "-" };
    println!("config dir: {}", ctx.loaded.dir.display());
    println!("modrinth token: {}", yn(&s.modrinth_token));
    println!("github token:   {}", yn(&s.github_token));
    println!(
        "mcsm api key:   {}",
        if ctx.loaded.mcsm_api_key().is_some() {
            if s.mcsm_api_key.is_some() {
                "set"
            } else {
                "set (from mcbackup env)"
            }
        } else {
            "-"
        }
    );
    for k in s.rcon.keys() {
        println!("rcon/{k}:        set");
    }
    Ok(())
}
