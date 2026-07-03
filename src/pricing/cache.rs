use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const APP_CACHE_DIR: &str = "ccstats";
const PRICING_CACHE_FILE: &str = "pricing.json";

#[derive(Debug)]
struct CachePaths {
    write_path: Option<PathBuf>,
    read_paths: Vec<PathBuf>,
}

fn pricing_cache_file(root: &Path) -> PathBuf {
    root.join(APP_CACHE_DIR).join(PRICING_CACHE_FILE)
}

fn legacy_cache_file(home_dir: &Path) -> PathBuf {
    home_dir
        .join(".cache")
        .join(APP_CACHE_DIR)
        .join(PRICING_CACHE_FILE)
}

fn select_cache_paths(platform_cache_dir: Option<&Path>, home_dir: Option<&Path>) -> CachePaths {
    let preferred_path = platform_cache_dir.map(pricing_cache_file);
    let legacy_path = home_dir.map(legacy_cache_file);
    let write_path = preferred_path.clone().or_else(|| legacy_path.clone());

    let mut read_paths = Vec::new();
    if let Some(path) = &write_path {
        read_paths.push(path.clone());
    }
    if let Some(path) = legacy_path
        && !read_paths.contains(&path)
    {
        read_paths.push(path);
    }

    CachePaths {
        write_path,
        read_paths,
    }
}

fn cache_paths() -> CachePaths {
    let platform_cache_dir = dirs::cache_dir();
    let home_dir = dirs::home_dir();
    select_cache_paths(platform_cache_dir.as_deref(), home_dir.as_deref())
}

pub(super) fn get_cache_path() -> Option<PathBuf> {
    cache_paths().write_path
}

fn load_raw_cache_from_paths(paths: &[PathBuf]) -> Option<HashMap<String, serde_json::Value>> {
    for path in paths {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        return serde_json::from_reader(file).ok();
    }
    None
}

fn load_raw_cache_if_fresh_from_paths(
    paths: &[PathBuf],
    ttl: Duration,
) -> Option<(HashMap<String, serde_json::Value>, Duration)> {
    for path in paths {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        let modified = meta.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;
        if age > ttl {
            return None;
        }
        let file = File::open(path).ok()?;
        let data = serde_json::from_reader(file).ok()?;
        return Some((data, age));
    }
    None
}

pub(super) fn load_raw_cache() -> Option<HashMap<String, serde_json::Value>> {
    let paths = cache_paths();
    load_raw_cache_from_paths(&paths.read_paths)
}

pub(super) fn load_raw_cache_if_fresh(
    ttl: Duration,
) -> Option<(HashMap<String, serde_json::Value>, Duration)> {
    let paths = cache_paths();
    load_raw_cache_if_fresh_from_paths(&paths.read_paths, ttl)
}

pub(super) fn save_raw_cache(raw_data: &HashMap<String, serde_json::Value>) {
    let Some(path) = get_cache_path() else {
        return;
    };
    save_raw_cache_to_path(raw_data, &path);
}

fn save_raw_cache_to_path(raw_data: &HashMap<String, serde_json::Value>, path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = File::create(path) {
        let _ = serde_json::to_writer(&mut file, raw_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn sample_raw_data(key: &str) -> HashMap<String, serde_json::Value> {
        HashMap::from([(key.to_string(), json!({"input_cost_per_token": 1.0}))])
    }

    #[test]
    fn cache_paths_prefer_platform_cache_dir() {
        let platform = PathBuf::from("platform-cache");
        let home = PathBuf::from("home-dir");
        let paths = select_cache_paths(Some(platform.as_path()), Some(home.as_path()));

        assert_eq!(
            paths.write_path,
            Some(platform.join(APP_CACHE_DIR).join(PRICING_CACHE_FILE))
        );
        assert_eq!(
            paths.read_paths,
            vec![
                PathBuf::from("platform-cache")
                    .join(APP_CACHE_DIR)
                    .join(PRICING_CACHE_FILE),
                PathBuf::from("home-dir")
                    .join(".cache")
                    .join(APP_CACHE_DIR)
                    .join(PRICING_CACHE_FILE),
            ]
        );
    }

    #[test]
    fn cache_paths_use_legacy_as_explicit_fallback() {
        let home = PathBuf::from("home-dir");
        let paths = select_cache_paths(None, Some(home.as_path()));
        let legacy_path = home
            .join(".cache")
            .join(APP_CACHE_DIR)
            .join(PRICING_CACHE_FILE);

        assert_eq!(paths.write_path, Some(legacy_path.clone()));
        assert_eq!(paths.read_paths, vec![legacy_path]);
    }

    #[test]
    fn load_raw_cache_reads_legacy_when_preferred_absent() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let legacy_path = legacy_cache_file(home_root.path());
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path);

        let data = load_raw_cache_from_paths(&paths.read_paths).unwrap();

        assert!(data.contains_key("legacy-model"));
    }

    #[test]
    fn load_raw_cache_prefers_current_when_both_exist() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let current_path = paths.write_path.as_ref().unwrap();
        let legacy_path = legacy_cache_file(home_root.path());
        save_raw_cache_to_path(&sample_raw_data("current-model"), current_path);
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path);

        let data = load_raw_cache_from_paths(&paths.read_paths).unwrap();

        assert!(data.contains_key("current-model"));
        assert!(!data.contains_key("legacy-model"));
    }

    #[test]
    fn load_raw_cache_if_fresh_reads_legacy_when_preferred_absent() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let legacy_path = legacy_cache_file(home_root.path());
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path);

        let (data, _age) =
            load_raw_cache_if_fresh_from_paths(&paths.read_paths, Duration::from_secs(60)).unwrap();

        assert!(data.contains_key("legacy-model"));
    }

    #[test]
    fn save_raw_cache_writes_current_path_not_legacy() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let current_path = paths.write_path.as_ref().unwrap();
        let legacy_path = legacy_cache_file(home_root.path());

        save_raw_cache_to_path(&sample_raw_data("current-model"), current_path);

        assert!(current_path.exists());
        assert!(!legacy_path.exists());
    }
}
