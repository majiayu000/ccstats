//! Literal filesystem roots and portable directory overrides.
use std::path::{Path, PathBuf};

pub(crate) fn glob_pattern(root: &Path, suffix: &str) -> String {
    let text = root.to_string_lossy();
    #[cfg(windows)]
    if let Some(std::path::Component::Prefix(prefix)) = root.components().next() {
        // The prefix is a literal filesystem scope in glob, not a pattern.
        // In particular, escaping the ? in \\?\ would corrupt canonical paths.
        let prefix_text = prefix.as_os_str().to_string_lossy();
        let scope = match prefix.kind() {
            std::path::Prefix::VerbatimUNC(server, share) => {
                // glob supports ordinary UNC scopes, but rejects verbatim UNC.
                format!(
                    r"\\{}\{}",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                )
            }
            _ => prefix_text.to_string(),
        };
        return format!(
            "{}{}\\{}",
            scope,
            glob::Pattern::escape(&text[prefix_text.len()..]),
            suffix.replace('/', r"\"),
        );
    }
    format!("{}/{suffix}", glob::Pattern::escape(&text))
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
