//! File-level caching for parsed entries
//!
//! Caches parsed entries keyed by file path with mtime/size validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::core::types::RawEntry;

const CACHE_VERSION: u32 = 3;

/// Cached file data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    pub mtime: i64,
    pub size: u64,
    pub entries: Vec<RawEntry>,
}

/// Full cache structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntriesCache {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub files: HashMap<String, CachedFile>,
}

/// Result of loading a file (from cache or freshly parsed)
pub struct FileLoadResult {
    pub key: String,
    pub entries: Vec<RawEntry>,
    pub mtime: Option<i64>,
    pub size: Option<u64>,
    pub from_cache: bool,
}

/// Get file metadata (mtime, size)
pub fn file_meta(path: &Path) -> Option<(i64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((mtime, meta.len()))
}

/// Cache manager for a specific source
pub struct CacheManager {
    cache_path: PathBuf,
    cache: EntriesCache,
}

impl CacheManager {
    /// Create a new cache manager for the given cache file path
    pub fn new(cache_path: PathBuf) -> Self {
        let cache = Self::load_cache(&cache_path);
        Self { cache_path, cache }
    }

    /// Create a cache manager that doesn't persist
    pub fn ephemeral() -> Self {
        Self {
            cache_path: PathBuf::new(),
            cache: EntriesCache::default(),
        }
    }

    fn load_cache(path: &Path) -> EntriesCache {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return EntriesCache::default(),
        };
        match serde_json::from_reader(file) {
            Ok(cache) => {
                let cache: EntriesCache = cache;
                if cache.version == CACHE_VERSION {
                    cache
                } else {
                    EntriesCache::default()
                }
            }
            Err(_) => EntriesCache::default(),
        }
    }

    /// Check if file is cached and still valid
    pub fn get_cached(&self, path: &Path) -> Option<&CachedFile> {
        let key = path.to_string_lossy();
        let meta = file_meta(path)?;
        let cached = self.cache.files.get(key.as_ref())?;
        if cached.mtime == meta.0 && cached.size == meta.1 {
            Some(cached)
        } else {
            None
        }
    }

    /// Save entries to cache
    pub fn save(&mut self, results: Vec<FileLoadResult>) {
        if self.cache_path.as_os_str().is_empty() {
            return;
        }

        let mut files = HashMap::new();
        for result in results {
            if let (Some(mtime), Some(size)) = (result.mtime, result.size) {
                files.insert(
                    result.key,
                    CachedFile {
                        mtime,
                        size,
                        entries: result.entries,
                    },
                );
            }
        }

        let cache = EntriesCache {
            version: CACHE_VERSION,
            files,
        };

        if let Some(parent) = self.cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = File::create(&self.cache_path) {
            let _ = serde_json::to_writer(&mut file, &cache);
        }
    }
}

/// Get the default cache directory
pub fn get_cache_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".cache").join("ccstats"))
}

/// Get cache path for a specific source
pub fn get_source_cache_path(source_name: &str) -> Option<PathBuf> {
    let cache_dir = get_cache_dir()?;
    Some(cache_dir.join(format!("{}.json", source_name)))
}
