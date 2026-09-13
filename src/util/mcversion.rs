//! Minecraft version parsing and ordering.
//!
//! Handles both the classic `1.21.8` scheme and the year-based `26.2` / `26.2.1` scheme
//! introduced in 2026, plus `-pre1` / `-rc2` snapshots. Versions compare numerically
//! component by component, so `26.2 > 1.21.11` holds without special casing.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McVersion {
    pub parts: Vec<u32>,
    pub pre: Option<PreRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PreRelease {
    Pre(u32),
    Rc(u32),
}

impl McVersion {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (nums, pre) = match s.split_once('-') {
            Some((n, p)) => (n, Some(p)),
            None => (s, None),
        };
        let parts: Vec<u32> = nums.split('.').map(|p| p.parse::<u32>().ok()).collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }
        let pre = match pre {
            None => None,
            Some(p) => {
                let p = p.to_ascii_lowercase();
                if let Some(n) = p.strip_prefix("pre") {
                    Some(PreRelease::Pre(n.parse().ok()?))
                } else {
                    let n = p.strip_prefix("rc")?;
                    Some(PreRelease::Rc(n.parse().ok()?))
                }
            }
        };
        Some(Self { parts, pre })
    }

    /// `true` for `26.2`, `1.21.8`; `false` for snapshots and pre-releases.
    pub fn is_release(&self) -> bool {
        self.pre.is_none()
    }

    /// The "line" a version belongs to, for lenient compatibility ("a plugin built for a
    /// nearby version probably runs"). Classic scheme: major.minor (`1.21` covers 1.21–1.21.11).
    /// Year scheme: the year (`26` covers 26.1–26.x), since each `26.x` drop is what a
    /// `1.21.x` drop used to be.
    pub fn line(&self) -> McVersion {
        let n = if self.parts.first().copied().unwrap_or(0) > 1 { 1 } else { 2 };
        McVersion {
            parts: self.parts.iter().take(n).copied().collect(),
            pre: None,
        }
    }

    pub fn same_line(&self, other: &McVersion) -> bool {
        self.line() == other.line()
    }

    fn part(&self, i: usize) -> u32 {
        self.parts.get(i).copied().unwrap_or(0)
    }
}

impl Ord for McVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.parts.len().max(other.parts.len());
        for i in 0..n {
            match self.part(i).cmp(&other.part(i)) {
                Ordering::Equal => continue,
                o => return o,
            }
        }
        // release > rc > pre, then by number
        match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(PreRelease::Rc(a)), Some(PreRelease::Rc(b))) => a.cmp(b),
            (Some(PreRelease::Pre(a)), Some(PreRelease::Pre(b))) => a.cmp(b),
            (Some(PreRelease::Rc(_)), Some(PreRelease::Pre(_))) => Ordering::Greater,
            (Some(PreRelease::Pre(_)), Some(PreRelease::Rc(_))) => Ordering::Less,
        }
    }
}

impl PartialOrd for McVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for McVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nums: Vec<String> = self.parts.iter().map(|p| p.to_string()).collect();
        write!(f, "{}", nums.join("."))?;
        match &self.pre {
            Some(PreRelease::Pre(n)) => write!(f, "-pre{n}"),
            Some(PreRelease::Rc(n)) => write!(f, "-rc{n}"),
            None => Ok(()),
        }
    }
}

impl FromStr for McVersion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        McVersion::parse(s).ok_or_else(|| format!("not a Minecraft version: {s:?}"))
    }
}

impl serde::Serialize for McVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for McVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> McVersion {
        McVersion::parse(s).unwrap_or_else(|| panic!("parse {s}"))
    }

    #[test]
    fn parses_both_schemes() {
        assert_eq!(v("1.21.8").parts, vec![1, 21, 8]);
        assert_eq!(v("26.2").parts, vec![26, 2]);
        assert_eq!(v("26.2.1").parts, vec![26, 2, 1]);
        assert_eq!(v("26.3-pre1").pre, Some(PreRelease::Pre(1)));
        assert_eq!(v("26.3-rc2").pre, Some(PreRelease::Rc(2)));
        assert!(McVersion::parse("26w14a").is_none());
        assert!(McVersion::parse("latest").is_none());
        assert!(McVersion::parse("").is_none());
    }

    #[test]
    fn orders_across_schemes() {
        assert!(v("26.2") > v("1.21.11"));
        assert!(v("26.2.1") > v("26.2"));
        assert!(v("26.1.2") < v("26.2"));
        assert!(v("1.21.11") > v("1.21.8"));
        assert!(v("1.21") < v("1.21.1"));
        assert!(v("26.3-pre1") < v("26.3"));
        assert!(v("26.3-pre2") < v("26.3-rc1"));
        assert!(v("26.3-rc1") < v("26.3"));
        assert_eq!(v("1.21").cmp(&v("1.21.0")), Ordering::Equal);
    }

    #[test]
    fn lines() {
        assert!(v("26.2.1").same_line(&v("26.2")));
        assert!(v("1.21.8").same_line(&v("1.21.11")));
        assert!(v("26.1.2").same_line(&v("26.2")));
        assert!(!v("26.2").same_line(&v("27.1")));
        assert!(!v("1.20.6").same_line(&v("1.21")));
        assert!(!v("1.21.11").same_line(&v("26.1")));
        assert_eq!(v("1.21.8").line().to_string(), "1.21");
        assert_eq!(v("26.2.1").line().to_string(), "26");
    }

    #[test]
    fn round_trips_display() {
        for s in ["1.21.8", "26.2", "26.2.1", "26.3-pre1", "26.3-rc2"] {
            assert_eq!(v(s).to_string(), s);
        }
    }
}
