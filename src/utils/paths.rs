//! Literal filesystem roots and portable directory overrides.
use std::path::{Path, PathBuf};

pub(crate) fn glob_pattern(root: &Path, suffix: &str) -> String {
    format!(
        "{}/{suffix}",
        glob::Pattern::escape(&root.to_string_lossy())
    )
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

#[cfg(windows)]
fn overridden_dir(variable: &str, suffix: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(suffix))
        })
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(path) = overridden_dir("XDG_CONFIG_HOME", ".config") {
        return Some(path);
    }
    dirs::config_dir()
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(path) = overridden_dir("XDG_DATA_HOME", ".local/share") {
        return Some(path);
    }
    dirs::data_dir()
}

pub(crate) fn data_local_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(path) = overridden_dir("XDG_DATA_HOME", ".local/share") {
        return Some(path);
    }
    dirs::data_local_dir()
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(path) = overridden_dir("XDG_CACHE_HOME", ".cache") {
        return Some(path);
    }
    dirs::cache_dir()
}
