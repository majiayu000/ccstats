//! Session discovery for Pi-family sources.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PI_AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
const PI_SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";
const SENPI_AGENT_DIR_ENV: &str = "SENPI_CODING_AGENT_DIR";
const SENPI_SESSION_DIR_ENV: &str = "SENPI_CODING_AGENT_SESSION_DIR";

enum SessionDirSetting {
    Missing,
    Reset,
    Path(PathBuf),
    Invalid(PathBuf),
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(expand_home_path)
}

fn expand_home_path(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" {
        return dirs::home_dir().unwrap_or(path);
    }
    value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix(r"~\"))
        .and_then(|suffix| dirs::home_dir().map(|home| home.join(suffix)))
        .unwrap_or(path)
}

fn nearest_senpi_config_dir(require_agent: bool) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let mut current = env::current_dir().ok()?;
    for _ in 0..=100 {
        if current == home {
            return None;
        }
        let config = current.join(".senpi");
        let agent = config.join("agent");
        let is_real_directory = |path: &Path| {
            fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
        };
        if is_real_directory(&config) && (!require_agent || is_real_directory(&agent)) {
            return Some(config);
        }
        if !current.pop() {
            return None;
        }
    }
    None
}

fn settings_source(directory: &Path) -> Option<PathBuf> {
    let jsonc = directory.join("settings.jsonc");
    if jsonc.is_file() {
        return Some(jsonc);
    }
    let json = directory.join("settings.json");
    json.is_file().then_some(json)
}

fn read_session_dir_setting(directory: &Path) -> SessionDirSetting {
    let Some(path) = settings_source(directory) else {
        return SessionDirSetting::Missing;
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return SessionDirSetting::Invalid(path);
    };
    let content = content.trim_start_matches('\u{feff}');
    let Ok(value) = jsonc_parser::parse_to_serde_value::<serde_json::Value>(
        content,
        &jsonc_parser::ParseOptions::default(),
    ) else {
        return SessionDirSetting::Invalid(path);
    };
    match value.get("sessionDir") {
        None => SessionDirSetting::Missing,
        Some(serde_json::Value::String(value)) if !value.is_empty() => {
            let path = expand_home_path(PathBuf::from(value));
            if path.is_absolute() {
                SessionDirSetting::Path(path)
            } else if let Ok(cwd) = env::current_dir() {
                SessionDirSetting::Path(cwd.join(path))
            } else {
                SessionDirSetting::Invalid(directory.to_path_buf())
            }
        }
        Some(serde_json::Value::String(_) | serde_json::Value::Null) => SessionDirSetting::Reset,
        Some(_) => SessionDirSetting::Invalid(path),
    }
}

fn senpi_sessions_dir() -> Result<PathBuf, PathBuf> {
    if let Some(sessions) = non_empty_env(SENPI_SESSION_DIR_ENV) {
        return Ok(sessions);
    }
    let project_config = nearest_senpi_config_dir(false);
    let agent_dir = non_empty_env(SENPI_AGENT_DIR_ENV)
        .or_else(|| nearest_senpi_config_dir(true).map(|config| config.join("agent")))
        .or_else(|| dirs::home_dir().map(|home| home.join(".senpi/agent")))
        .ok_or_else(|| PathBuf::from("senpi-settings-error.jsonl"))?;
    let default = || agent_dir.join("sessions");
    let setting = project_config
        .as_deref()
        .map_or(SessionDirSetting::Missing, read_session_dir_setting);
    match setting {
        SessionDirSetting::Path(path) => Ok(path),
        SessionDirSetting::Reset => Ok(default()),
        SessionDirSetting::Invalid(path) => Err(path),
        SessionDirSetting::Missing => match read_session_dir_setting(&agent_dir) {
            SessionDirSetting::Path(path) => Ok(path),
            SessionDirSetting::Missing | SessionDirSetting::Reset => Ok(default()),
            SessionDirSetting::Invalid(path) => Err(path),
        },
    }
}

fn find_family_files(root: &Path) -> Vec<PathBuf> {
    let patterns = [root.join("*.jsonl"), root.join("**").join("*.jsonl")];
    let mut files = Vec::new();
    for pattern in patterns {
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            files.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }
    files.sort();
    files.dedup();
    files
}

pub(super) fn find_pi_files() -> Vec<PathBuf> {
    let root = non_empty_env(PI_SESSION_DIR_ENV)
        .or_else(|| non_empty_env(PI_AGENT_DIR_ENV).map(|root| root.join("sessions")))
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent/sessions")));
    root.as_deref().map(find_family_files).unwrap_or_default()
}

pub(super) fn find_senpi_files() -> Vec<PathBuf> {
    match senpi_sessions_dir() {
        Ok(root) => find_family_files(&root),
        Err(settings_path) => vec![settings_path],
    }
}

pub(super) fn find_kimchi_files() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| find_family_files(&home.join(".config/kimchi/harness/sessions")))
        .unwrap_or_default()
}
