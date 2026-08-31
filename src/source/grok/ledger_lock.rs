use std::fs::{self, File, OpenOptions};
use std::path::Path;

use fs4::FileExt;

const LEDGER_LOCK_FILE: &str = "inference-v1.lock";

pub(super) fn acquire(ledger_path: &Path) -> Result<File, String> {
    let parent = ledger_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let lock_path = parent.join(LEDGER_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("failed to open {}: {error}", lock_path.display()))?;
    FileExt::lock(&lock)
        .map_err(|error| format!("failed to lock {}: {error}", lock_path.display()))?;
    Ok(lock)
}
