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

/// Missing optional data is empty; other filesystem failures make discovery incomplete.
pub(crate) fn existing_file(path: PathBuf) -> (Vec<PathBuf>, usize) {
    match std::fs::metadata(&path) {
        Ok(meta) => (meta.is_file().then_some(path).into_iter().collect(), 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), 0),
        Err(_) => (Vec::new(), 1),
    }
}

pub(crate) fn glob_files(root: &Path, pattern: &str) -> (Vec<PathBuf>, usize) {
    // glob may silently yield nothing when a literal prefix cannot be inspected.
    match std::fs::metadata(root) {
        Ok(meta) if meta.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), 0),
        Ok(_) | Err(_) => return (Vec::new(), 1),
    }
    let Ok(matches) = glob::glob(pattern) else {
        return (Vec::new(), 1);
    };
    let mut files = Vec::new();
    let mut errors = 0;
    for item in matches {
        match item {
            Ok(path) => {
                // A yielded match is no longer an optional missing root.
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.is_file() => files.push(path),
                    Ok(_) => {}
                    Err(_) => errors += 1,
                }
            }
            Err(_) => errors += 1,
        }
    }
    (files, errors)
}

fn nonempty_env_path(variable: &str) -> Option<PathBuf> {
    Some(PathBuf::from(
        std::env::var_os(variable).filter(|value| !value.is_empty())?,
    ))
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    let home = nonempty_env_path("HOME");
    #[cfg(windows)]
    let home = home.filter(|path| path.is_absolute());
    home.or_else(dirs::home_dir)
}

fn explicit_xdg(variable: &str) -> Option<PathBuf> {
    nonempty_env_path(variable).filter(|path| path.is_absolute())
}

/// True when `XDG_CONFIG_HOME` is a nonempty absolute override.
pub(crate) fn has_explicit_xdg_config() -> bool {
    explicit_xdg("XDG_CONFIG_HOME").is_some()
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Some(path) = explicit_xdg("XDG_CONFIG_HOME") {
        return Some(path);
    }
    dirs::config_dir()
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    if let Some(path) = explicit_xdg("XDG_DATA_HOME") {
        return Some(path);
    }
    dirs::data_dir()
}

pub(crate) fn data_local_dir() -> Option<PathBuf> {
    if let Some(path) = explicit_xdg("XDG_DATA_HOME") {
        return Some(path);
    }
    dirs::data_local_dir()
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    if let Some(path) = explicit_xdg("XDG_CACHE_HOME") {
        return Some(path);
    }
    dirs::cache_dir()
}

#[cfg(all(test, unix))]
mod discovery_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn matched_dangling_symlink_is_a_discovery_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.jsonl");
        std::os::unix::fs::symlink(temp.path().join("missing"), &path).unwrap();
        let pattern = glob_pattern(temp.path(), "*.jsonl");
        assert_eq!(glob_files(temp.path(), &pattern), (Vec::new(), 1));
        assert_eq!(existing_file(temp.path().join("optional")), (Vec::new(), 0));
    }

    #[test]
    fn literal_prefix_failure_is_not_a_missing_optional_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source/logs");
        let pattern = glob_pattern(&root, "*.jsonl");
        assert_eq!(glob_files(&root, &pattern), (Vec::new(), 0));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("usage.jsonl");
        std::fs::write(&file, "").unwrap();
        assert_eq!(glob_files(&root, &pattern), (vec![file.clone()], 0));
        let parent = root.parent().unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o000)).unwrap();
        let failed = glob_files(&root, &pattern);
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(failed, (Vec::new(), 1));
        assert_eq!(glob_files(&root, &pattern), (vec![file], 0));
    }
}
