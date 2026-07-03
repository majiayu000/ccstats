use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::source::CacheMetadata;

const APP_CACHE_DIR: &str = "ccstats";
const PRICING_CACHE_FILE: &str = "pricing.json";

pub(super) type RawPricingCache = HashMap<String, serde_json::Value>;
#[derive(Debug)]
pub(super) struct RawPricingCacheSnapshot {
    pub(super) data: RawPricingCache,
    pub(super) metadata: CacheMetadata,
}

type FreshRawPricingCache = RawPricingCacheSnapshot;
type CacheReadResult<T> = Result<Option<T>, CacheReadError>;

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

#[derive(Debug, thiserror::Error)]
pub(crate) enum CacheReadError {
    #[error("failed to inspect pricing cache at {path:?}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read pricing cache timestamp at {path:?}: {source}")]
    Modified {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open pricing cache at {path:?}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("pricing cache at {path:?} is malformed: {source}")]
    Malformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CacheWriteError {
    #[error("failed to create pricing cache directory {path:?}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create temporary pricing cache {path:?}: {source}")]
    CreateTemp {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize pricing cache to {path:?}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to flush temporary pricing cache {path:?}: {source}")]
    Flush {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to sync temporary pricing cache {path:?}: {source}")]
    Sync {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to replace pricing cache {target:?} with {temp:?}: {source}")]
    Rename {
        temp: PathBuf,
        target: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub(super) fn get_cache_path() -> Option<PathBuf> {
    cache_paths().write_path
}

#[cfg(test)]
pub(super) fn load_raw_cache_from_paths(paths: &[PathBuf]) -> CacheReadResult<RawPricingCache> {
    Ok(load_raw_cache_snapshot_from_paths(paths)?.map(|snapshot| snapshot.data))
}

pub(super) fn load_raw_cache_snapshot_from_paths(
    paths: &[PathBuf],
) -> CacheReadResult<RawPricingCacheSnapshot> {
    for path in paths {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(CacheReadError::Metadata {
                    path: path.clone(),
                    source,
                });
            }
        };
        let modified = meta.modified().map_err(|source| CacheReadError::Modified {
            path: path.clone(),
            source,
        })?;
        let file = File::open(path).map_err(|source| CacheReadError::Open {
            path: path.clone(),
            source,
        })?;
        let data = serde_json::from_reader(file).map_err(|source| CacheReadError::Malformed {
            path: path.clone(),
            source,
        })?;
        return Ok(Some(RawPricingCacheSnapshot {
            data,
            metadata: cache_metadata(modified),
        }));
    }
    Ok(None)
}

fn cache_metadata(modified: SystemTime) -> CacheMetadata {
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::MAX);
    CacheMetadata { age, modified }
}

pub(super) fn load_raw_cache_if_fresh_from_paths(
    paths: &[PathBuf],
    ttl: Duration,
) -> CacheReadResult<FreshRawPricingCache> {
    for path in paths {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(CacheReadError::Metadata {
                    path: path.clone(),
                    source,
                });
            }
        };
        let modified = meta.modified().map_err(|source| CacheReadError::Modified {
            path: path.clone(),
            source,
        })?;
        let metadata = cache_metadata(modified);
        if metadata.age > ttl {
            return Ok(None);
        }
        let file = File::open(path).map_err(|source| CacheReadError::Open {
            path: path.clone(),
            source,
        })?;
        let data = serde_json::from_reader(file).map_err(|source| CacheReadError::Malformed {
            path: path.clone(),
            source,
        })?;
        return Ok(Some(RawPricingCacheSnapshot { data, metadata }));
    }
    Ok(None)
}

pub(super) fn load_raw_cache_snapshot() -> CacheReadResult<RawPricingCacheSnapshot> {
    let paths = cache_paths();
    load_raw_cache_snapshot_from_paths(&paths.read_paths)
}

pub(super) fn load_raw_cache_if_fresh(ttl: Duration) -> CacheReadResult<FreshRawPricingCache> {
    let paths = cache_paths();
    load_raw_cache_if_fresh_from_paths(&paths.read_paths, ttl)
}

pub(super) fn save_raw_cache(
    raw_data: &HashMap<String, serde_json::Value>,
) -> Result<(), CacheWriteError> {
    let Some(path) = get_cache_path() else {
        return Ok(());
    };
    save_raw_cache_to_path(raw_data, &path)
}

pub(super) fn save_raw_cache_to_path(
    raw_data: &HashMap<String, serde_json::Value>,
    path: &Path,
) -> Result<(), CacheWriteError> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CacheWriteError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;

    let (temp_path, temp_file) = create_temp_file(parent)?;
    let write_result = write_cache_file(raw_data, &temp_path, temp_file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    fs::rename(&temp_path, path).map_err(|source| {
        let error = CacheWriteError::Rename {
            temp: temp_path.clone(),
            target: path.to_path_buf(),
            source,
        };
        let _ = fs::remove_file(&temp_path);
        error
    })
}

fn create_temp_file(parent: &Path) -> Result<(PathBuf, File), CacheWriteError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    let process_id = std::process::id();

    for attempt in 0..32 {
        let temp_path = parent.join(format!(
            ".{PRICING_CACHE_FILE}.{process_id}.{nanos}.{attempt}.tmp"
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(CacheWriteError::CreateTemp {
                    path: temp_path,
                    source,
                });
            }
        }
    }

    let temp_path = parent.join(format!(".{PRICING_CACHE_FILE}.{process_id}.{nanos}.tmp"));
    Err(CacheWriteError::CreateTemp {
        path: temp_path,
        source: std::io::Error::new(
            ErrorKind::AlreadyExists,
            "unable to allocate unique pricing cache temporary file",
        ),
    })
}

fn write_cache_file(
    raw_data: &HashMap<String, serde_json::Value>,
    temp_path: &Path,
    temp_file: File,
) -> Result<(), CacheWriteError> {
    let mut writer = BufWriter::new(temp_file);
    serde_json::to_writer(&mut writer, raw_data).map_err(|source| CacheWriteError::Serialize {
        path: temp_path.to_path_buf(),
        source,
    })?;
    writer.flush().map_err(|source| CacheWriteError::Flush {
        path: temp_path.to_path_buf(),
        source,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| CacheWriteError::Sync {
            path: temp_path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
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
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path).unwrap();

        let data = load_raw_cache_from_paths(&paths.read_paths)
            .unwrap()
            .unwrap();

        assert!(data.contains_key("legacy-model"));
    }

    #[test]
    fn load_raw_cache_prefers_current_when_both_exist() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let current_path = paths.write_path.as_ref().unwrap();
        let legacy_path = legacy_cache_file(home_root.path());
        save_raw_cache_to_path(&sample_raw_data("current-model"), current_path).unwrap();
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path).unwrap();

        let data = load_raw_cache_from_paths(&paths.read_paths)
            .unwrap()
            .unwrap();

        assert!(data.contains_key("current-model"));
        assert!(!data.contains_key("legacy-model"));
    }

    #[test]
    fn load_raw_cache_if_fresh_reads_legacy_when_preferred_absent() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let legacy_path = legacy_cache_file(home_root.path());
        save_raw_cache_to_path(&sample_raw_data("legacy-model"), &legacy_path).unwrap();

        let snapshot =
            load_raw_cache_if_fresh_from_paths(&paths.read_paths, Duration::from_secs(60))
                .unwrap()
                .unwrap();

        assert!(snapshot.data.contains_key("legacy-model"));
    }

    #[test]
    fn save_raw_cache_writes_current_path_not_legacy() {
        let platform_root = TempDir::new().unwrap();
        let home_root = TempDir::new().unwrap();
        let paths = select_cache_paths(Some(platform_root.path()), Some(home_root.path()));
        let current_path = paths.write_path.as_ref().unwrap();
        let legacy_path = legacy_cache_file(home_root.path());

        save_raw_cache_to_path(&sample_raw_data("current-model"), current_path).unwrap();

        assert!(current_path.exists());
        assert!(!legacy_path.exists());
    }

    #[test]
    fn load_raw_cache_returns_missing_for_absent_cache() {
        let root = TempDir::new().unwrap();
        let missing = root.path().join("pricing.json");

        let result = load_raw_cache_from_paths(&[missing]).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn load_raw_cache_reports_malformed_cache() {
        let root = TempDir::new().unwrap();
        let cache_path = root.path().join("pricing.json");
        fs::write(&cache_path, "{not json").unwrap();

        let error = load_raw_cache_from_paths(&[cache_path]).unwrap_err();

        assert!(matches!(error, CacheReadError::Malformed { .. }));
    }

    #[test]
    fn load_raw_cache_if_fresh_reports_malformed_cache() {
        let root = TempDir::new().unwrap();
        let cache_path = root.path().join("pricing.json");
        fs::write(&cache_path, "{not json").unwrap();

        let error =
            load_raw_cache_if_fresh_from_paths(&[cache_path], Duration::from_secs(60)).unwrap_err();

        assert!(matches!(error, CacheReadError::Malformed { .. }));
    }

    #[test]
    fn save_raw_cache_atomically_replaces_existing_cache() {
        let root = TempDir::new().unwrap();
        let cache_path = root.path().join("pricing.json");
        save_raw_cache_to_path(&sample_raw_data("old-model"), &cache_path).unwrap();

        save_raw_cache_to_path(&sample_raw_data("new-model"), &cache_path).unwrap();
        let data = load_raw_cache_from_paths(&[cache_path]).unwrap().unwrap();

        assert!(data.contains_key("new-model"));
        assert!(!data.contains_key("old-model"));
    }

    #[cfg(unix)]
    #[test]
    fn save_raw_cache_failure_keeps_existing_cache() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let cache_path = root.path().join("pricing.json");
        save_raw_cache_to_path(&sample_raw_data("old-model"), &cache_path).unwrap();
        let old_contents = fs::read_to_string(&cache_path).unwrap();

        let mut permissions = fs::metadata(root.path()).unwrap().permissions();
        permissions.set_mode(0o555);
        fs::set_permissions(root.path(), permissions).unwrap();

        let save_result = save_raw_cache_to_path(&sample_raw_data("new-model"), &cache_path);

        let mut permissions = fs::metadata(root.path()).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(root.path(), permissions).unwrap();

        assert!(save_result.is_err());
        assert_eq!(fs::read_to_string(&cache_path).unwrap(), old_contents);
    }
}
