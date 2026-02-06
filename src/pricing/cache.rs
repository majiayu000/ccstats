use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub(super) fn get_cache_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".cache").join("ccstats").join("pricing.json"))
}

pub(super) fn load_raw_cache() -> Option<HashMap<String, serde_json::Value>> {
    let path = get_cache_path()?;
    let file = File::open(&path).ok()?;
    serde_json::from_reader(file).ok()
}

pub(super) fn load_raw_cache_if_fresh(
    ttl: Duration,
) -> Option<(HashMap<String, serde_json::Value>, Duration)> {
    let path = get_cache_path()?;
    let meta = std::fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > ttl {
        return None;
    }
    let file = File::open(&path).ok()?;
    let data = serde_json::from_reader(file).ok()?;
    Some((data, age))
}

pub(super) fn save_raw_cache(raw_data: &HashMap<String, serde_json::Value>) {
    let Some(path) = get_cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = File::create(&path) {
        let _ = serde_json::to_writer(&mut file, raw_data);
    }
}
