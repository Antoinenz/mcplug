use std::path::PathBuf;

/// Resolution order for the config directory:
/// 1. `--config` (handled by the caller)  2. `$MCPLUG_CONFIG_DIR`
/// 3. `/etc/mcplug` when running as root and it exists (daemon installs)
/// 4. the invoking user's home when run via `sudo` (so `sudo mcplug` doesn't get an empty
///    config in `/root`)  5. the platform config dir (`~/.config/mcplug`).
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MCPLUG_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    #[cfg(unix)]
    {
        let etc = PathBuf::from("/etc/mcplug");
        if is_root() && etc.join("config.toml").exists() {
            return etc;
        }
        if let Some(home) = sudo_user_home() {
            return home.join(".config").join("mcplug");
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcplug")
}

/// Per-machine mutable state (daemon heartbeat, caches).
pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MCPLUG_STATE_DIR") {
        return PathBuf::from(d);
    }
    #[cfg(unix)]
    if is_root() {
        return PathBuf::from("/var/lib/mcplug");
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcplug")
}

#[cfg(unix)]
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc_geteuid() == 0 }
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    false
}

#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}

/// Home directory of `$SUDO_USER`, if we were started through sudo.
#[cfg(unix)]
fn sudo_user_home() -> Option<PathBuf> {
    let user = std::env::var("SUDO_USER").ok()?;
    if user == "root" {
        return None;
    }
    // /etc/passwd is the portable way without pulling in a users crate.
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let mut f = l.split(':');
        if f.next()? != user {
            return None;
        }
        f.nth(4).map(PathBuf::from)
    })
}
