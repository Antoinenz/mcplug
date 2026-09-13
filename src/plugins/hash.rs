use std::io::Read;
use std::path::Path;

use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

/// All three digests sources care about, computed in one read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JarHashes {
    pub sha1: String,
    pub sha256: String,
    pub sha512: String,
    pub size: u64,
}

impl JarHashes {
    pub fn of_file(path: &Path) -> std::io::Result<Self> {
        let mut f = std::fs::File::open(path)?;
        let mut buf = vec![0u8; 1 << 16];
        let (mut h1, mut h256, mut h512) = (Sha1::new(), Sha256::new(), Sha512::new());
        let mut size = 0u64;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h1.update(&buf[..n]);
            h256.update(&buf[..n]);
            h512.update(&buf[..n]);
            size += n as u64;
        }
        Ok(Self {
            sha1: hex::encode(h1.finalize()),
            sha256: hex::encode(h256.finalize()),
            sha512: hex::encode(h512.finalize()),
            size,
        })
    }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self {
            sha1: hex::encode(Sha1::digest(bytes)),
            sha256: hex::encode(Sha256::digest(bytes)),
            sha512: hex::encode(Sha512::digest(bytes)),
            size: bytes.len() as u64,
        }
    }
}
