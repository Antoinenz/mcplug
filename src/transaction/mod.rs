//! Applying plugin changes safely: nothing on the live server is touched until every
//! file is downloaded and verified, every swap is recorded before it happens, and a
//! transaction can be reverted from its manifest.

pub mod apply;
pub mod journal;
pub mod plan;

pub use apply::{apply, revert, Progress, ProgressFn, TxOutcome};
pub use plan::{build_plan, PlanItem, PlanRequest, UpdatePlan};

use std::path::{Path, PathBuf};

pub fn mcplug_dir(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join(crate::lockfile::DIR)
}

pub fn staging_dir(plugins_dir: &Path, tx: &str) -> PathBuf {
    mcplug_dir(plugins_dir).join("staging").join(tx)
}

pub fn rollback_dir(plugins_dir: &Path, tx: &str) -> PathBuf {
    mcplug_dir(plugins_dir).join("rollback").join(tx)
}

pub fn new_tx_id() -> String {
    chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string()
}
