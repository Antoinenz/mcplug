//! Find out which Java a start command will run, and which Javas are installed.

use std::path::{Path, PathBuf};

/// Major version reported by `<java> -version` (`21.0.11` → 21, `25.0.4` → 25, `1.8.0_392` → 8).
pub async fn java_major(java: &str, cwd: Option<&Path>) -> Option<u32> {
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-version");
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    parse_java_version_output(&text)
}

pub fn parse_java_version_output(text: &str) -> Option<u32> {
    let line = text.lines().find(|l| l.contains("version"))?;
    let quoted = line.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// Candidate JVMs on this machine (Linux `/usr/lib/jvm/*`, plus whatever is on PATH).
pub fn installed_javas() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/usr/lib/jvm") {
        for e in rd.flatten() {
            let bin = e.path().join("bin").join("java");
            if bin.exists() && !e.path().is_symlink() {
                out.push(bin);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_versions() {
        use super::parse_java_version_output as p;
        assert_eq!(p("openjdk version \"21.0.11\" 2026-04-21 LTS\nOpenJDK Runtime"), Some(21));
        assert_eq!(p("openjdk version \"25.0.4\" 2026-07-21"), Some(25));
        assert_eq!(p("java version \"1.8.0_392\""), Some(8));
        assert_eq!(p("nonsense"), None);
    }
}
