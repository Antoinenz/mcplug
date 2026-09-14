//! Bundle the companion plugin jar into the binary when it has been built
//! (`cd companion && mvn package`). Without it, `mcplug bridge` explains how to get one.
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out.join("McplugBridge.jar");
    let src = std::fs::read_dir("companion/target")
        .ok()
        .and_then(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jar")).max());
    match src {
        Some(p) => {
            std::fs::copy(&p, &dest).expect("copy bridge jar");
            println!("cargo:rerun-if-changed={}", p.display());
        }
        None => {
            std::fs::write(&dest, []).expect("placeholder");
        }
    }
    println!("cargo:rerun-if-changed=companion/target");
}
