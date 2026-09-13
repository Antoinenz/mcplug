use std::io::{Read, Seek};

use super::{DescriptorCompat, Platform, PluginDescriptor};
use crate::server::PlatformKind;
use crate::util::McVersion;

/// Paper / Purpur / Folia / Pufferfish / Spigot: `plugins/*.jar` with `plugin.yml`
/// (or `paper-plugin.yml` for Paper-native plugins).
pub struct Bukkit {
    pub kind: PlatformKind,
}

impl Platform for Bukkit {
    fn kind(&self) -> PlatformKind {
        self.kind
    }

    fn plugin_dir_name(&self) -> &'static str {
        "plugins"
    }

    fn read_descriptor<R: Read + Seek>(&self, zip: &mut zip::ZipArchive<R>) -> Option<PluginDescriptor> {
        // Paper plugins may ship both; paper-plugin.yml is the authoritative one on Paper.
        let paper = read_yaml(zip, "paper-plugin.yml");
        let bukkit = read_yaml(zip, "plugin.yml");
        let doc = match (self.kind, paper, bukkit) {
            (PlatformKind::Spigot, _, Some(b)) => b,
            (_, Some(p), _) => p,
            (_, None, Some(b)) => b,
            _ => return None,
        };
        descriptor_from_yaml(&doc)
    }

    fn modrinth_loaders(&self) -> &'static [&'static str] {
        match self.kind {
            PlatformKind::Folia => &["folia", "paper", "purpur", "spigot", "bukkit"],
            PlatformKind::Purpur => &["purpur", "paper", "spigot", "bukkit"],
            PlatformKind::Spigot => &["spigot", "bukkit"],
            _ => &["paper", "purpur", "spigot", "bukkit"],
        }
    }

    fn hangar_platform(&self) -> Option<&'static str> {
        Some("PAPER")
    }

    fn ignored_subdirs(&self) -> &'static [&'static str] {
        &[".paper-remapped", ".mcplug", "update", "disabled"]
    }

    fn descriptor_compat(&self, d: &PluginDescriptor, mc: &McVersion) -> DescriptorCompat {
        match d.api_version.as_deref().and_then(McVersion::parse) {
            Some(api) if &api > mc => DescriptorCompat::TooNew,
            Some(_) => DescriptorCompat::Ok,
            None => DescriptorCompat::Unknown,
        }
    }
}

fn read_yaml<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<serde_yaml::Value> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    serde_yaml::from_str(&s).ok()
}

/// plugin.yml files are hand-written and sloppy (numbers as versions, single strings where
/// lists are expected), so pull each key leniently instead of deserialising a struct.
fn descriptor_from_yaml(doc: &serde_yaml::Value) -> Option<PluginDescriptor> {
    let name = scalar(doc.get("name"))?;
    Some(PluginDescriptor {
        name,
        version: scalar(doc.get("version")),
        api_version: scalar(doc.get("api-version")),
        authors: {
            let mut a = list(doc.get("authors"));
            if a.is_empty() {
                a.extend(scalar(doc.get("author")));
            }
            a
        },
        website: scalar(doc.get("website")),
        depend: list(doc.get("depend")),
        softdepend: list(doc.get("softdepend")),
    })
}

fn scalar(v: Option<&serde_yaml::Value>) -> Option<String> {
    match v? {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn list(v: Option<&serde_yaml::Value>) -> Vec<String> {
    match v {
        Some(serde_yaml::Value::Sequence(s)) => s.iter().filter_map(|x| scalar(Some(x))).collect(),
        Some(other) => scalar(Some(other)).into_iter().collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_yaml() {
        let doc: serde_yaml::Value = serde_yaml::from_str(
            "name: Vault\nversion: 1.7.3\napi-version: 1.13\nauthor: Sleaker\ndepend: WorldEdit\nsoftdepend: [LuckPerms, Essentials]\n",
        )
        .unwrap();
        let d = descriptor_from_yaml(&doc).unwrap();
        assert_eq!(d.name, "Vault");
        assert_eq!(d.version.as_deref(), Some("1.7.3"));
        assert_eq!(d.api_version.as_deref(), Some("1.13"));
        assert_eq!(d.authors, vec!["Sleaker"]);
        assert_eq!(d.depend, vec!["WorldEdit"]);
        assert_eq!(d.softdepend, vec!["LuckPerms", "Essentials"]);
        let b = Bukkit { kind: PlatformKind::Paper };
        assert_eq!(b.descriptor_compat(&d, &McVersion::parse("26.2").unwrap()), DescriptorCompat::Ok);
        let newer = PluginDescriptor { api_version: Some("26.3".into()), ..d };
        assert_eq!(b.descriptor_compat(&newer, &McVersion::parse("26.2").unwrap()), DescriptorCompat::TooNew);
    }
}
