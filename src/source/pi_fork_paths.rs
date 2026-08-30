//! Official session-root selection for Gajae Code, Prime Agent, and Oh My Pi.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

fn expand_home(path: PathBuf) -> PathBuf {
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

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(expand_home)
}

fn literal_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(part.as_os_str());
            }
            Component::ParentDir if !normalized.pop() && !path.is_absolute() => {
                normalized.push("..");
            }
            Component::ParentDir | Component::CurDir => {}
        }
    }
    normalized
}

fn resolved_path(path: PathBuf) -> PathBuf {
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir().map_or(path.clone(), |cwd| cwd.join(path))
    };
    let normalized = lexical_normalize(&absolute);
    fs::canonicalize(&normalized).unwrap_or(normalized)
}

fn find_jsonl(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for pattern in [root.join("*.jsonl"), root.join("**").join("*.jsonl")] {
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            files.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }
    files.sort();
    files.dedup();
    files
}

fn session_header(path: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let mut header: serde_json::Value = serde_json::from_str(lines.next()?).ok()?;
    if header["type"] != "session" {
        return None;
    }
    for line in lines {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record["type"] == "header_patch"
            && let Some(cwd) = record["patch"]["cwd"]
                .as_str()
                .filter(|cwd| !cwd.trim().is_empty())
        {
            header["cwd"] = serde_json::Value::String(cwd.to_string());
        }
    }
    Some(header)
}

fn artifact_owner(path: &Path) -> PathBuf {
    let mut owner = path.to_path_buf();
    for _ in 0..32 {
        let Some(parent) = owner.parent() else {
            break;
        };
        let candidate = parent.with_extension("jsonl");
        if !candidate.is_file() {
            break;
        }
        owner = candidate;
    }
    owner
}

fn parent_session_file(owner: &Path, parent_session: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(parent_session);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        owner.parent()?.join(candidate)
    };
    if candidate.is_file() {
        return Some(candidate);
    }

    let sessions_root = owner.parent()?.parent()?;
    find_jsonl(sessions_root).into_iter().find(|path| {
        session_header(path)
            .and_then(|header| header["id"].as_str().map(str::to_owned))
            .is_some_and(|id| id == parent_session)
    })
}

pub(super) fn inherited_project_path(path: &Path, parent_session: Option<&str>) -> Option<String> {
    let mut owner = artifact_owner(path);
    if owner == path {
        return None;
    }
    let mut next_parent = parent_session.map(str::to_owned);
    for _ in 0..32 {
        let header = session_header(&owner)?;
        let cwd = header["cwd"].as_str()?.to_string();
        let parent = next_parent
            .take()
            .or_else(|| header["parentSession"].as_str().map(str::to_owned));
        let Some(parent) = parent else {
            return Some(cwd);
        };
        let Some(parent_file) = parent_session_file(&owner, &parent) else {
            return Some(cwd);
        };
        owner = artifact_owner(&parent_file);
    }
    None
}

pub(super) fn parent_transcript(path: &Path, parent_session: &str) -> Option<PathBuf> {
    parent_session_file(&artifact_owner(path), parent_session)
}

pub(super) fn linked_child_transcript(
    path: &Path,
    parent_session: Option<&str>,
    child_id: &str,
) -> Option<PathBuf> {
    let direct = path.with_extension("").join(format!("{child_id}.jsonl"));
    if direct.is_file() {
        return Some(direct);
    }
    let mut owner = artifact_owner(path);
    let mut next_parent = parent_session.map(str::to_owned);
    for _ in 0..32 {
        let child = owner.with_extension("").join(format!("{child_id}.jsonl"));
        if child.is_file() {
            return Some(child);
        }
        let header = session_header(&owner)?;
        let parent = next_parent
            .take()
            .or_else(|| header["parentSession"].as_str().map(str::to_owned))?;
        owner = artifact_owner(&parent_session_file(&owner, &parent)?);
    }
    None
}

fn safe_config_name(name: &str, default: &str) -> PathBuf {
    let trimmed = name.trim();
    let path = Path::new(trimmed);
    if trimmed.is_empty() || path.components().any(|part| part == Component::ParentDir) {
        PathBuf::from(default)
    } else {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(value) => Some(value),
                Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
                Component::ParentDir => unreachable!("rejected above"),
            })
            .collect()
    }
}

fn home_config_root(home: &Path, config: &Path) -> PathBuf {
    let mut relative = PathBuf::new();
    for part in config.components() {
        match part {
            Component::Normal(value) => relative.push(value),
            Component::ParentDir if !relative.pop() => relative.push(".."),
            Component::ParentDir
            | Component::CurDir
            | Component::RootDir
            | Component::Prefix(_) => {}
        }
    }
    home.join(relative)
}

fn xdg_app_root(app: &str, suffix: &[&str]) -> Option<PathBuf> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return None;
    }
    let mut root = env_path("XDG_DATA_HOME")?.join(app);
    for part in suffix {
        root.push(part);
    }
    root.is_dir().then_some(root)
}

enum SessionDirSetting {
    Missing,
    Reset,
    Path(PathBuf),
    Invalid(PathBuf),
}

fn prime_session_setting(path: &Path) -> SessionDirSetting {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) if !path.exists() => return SessionDirSetting::Missing,
        Err(_) => return SessionDirSetting::Invalid(path.to_path_buf()),
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return SessionDirSetting::Invalid(path.to_path_buf());
    };
    let Some(setting) = value.get("sessionDir") else {
        return SessionDirSetting::Missing;
    };
    match setting {
        serde_json::Value::String(value) if !value.is_empty() => {
            SessionDirSetting::Path(expand_home(PathBuf::from(value)))
        }
        serde_json::Value::Null | serde_json::Value::String(_) => SessionDirSetting::Reset,
        _ => SessionDirSetting::Invalid(path.to_path_buf()),
    }
}

pub(super) fn find_gjc_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let config = env::var("GJC_CONFIG_DIR").ok().map_or_else(
        || PathBuf::from(".gjc"),
        |value| safe_config_name(&value, ".gjc"),
    );
    let default_agent = resolved_path(home.join(config).join("agent"));
    let agent =
        env_path("GJC_CODING_AGENT_DIR").map_or_else(|| default_agent.clone(), resolved_path);
    let root = if agent == default_agent {
        xdg_app_root("gjc", &[]).map_or_else(|| agent.join("sessions"), |xdg| xdg.join("sessions"))
    } else {
        agent.join("sessions")
    };
    find_jsonl(&root)
}

pub(super) fn find_prime_files() -> Vec<PathBuf> {
    let mut invalid_settings = Vec::new();
    let agent = env_path("PRIME_AGENT_CODING_AGENT_DIR")
        .or_else(|| dirs::home_dir().map(|home| home.join(".prime/agent")));
    let root = env_path("PRIME_AGENT_SESSION_DIR").or_else(|| {
        let agent = agent?;
        let default = agent.join("sessions");
        let project = env::current_dir()
            .ok()
            .map(|cwd| cwd.join(".prime/agent/settings.json"))
            .map_or(SessionDirSetting::Missing, |path| {
                prime_session_setting(&path)
            });
        match project {
            SessionDirSetting::Path(path) => Some(path),
            SessionDirSetting::Reset => Some(default),
            SessionDirSetting::Invalid(path) => {
                invalid_settings.push(path);
                match prime_session_setting(&agent.join("settings.json")) {
                    SessionDirSetting::Path(path) => Some(path),
                    SessionDirSetting::Invalid(path) => {
                        invalid_settings.push(path);
                        Some(default)
                    }
                    SessionDirSetting::Missing | SessionDirSetting::Reset => Some(default),
                }
            }
            SessionDirSetting::Missing => match prime_session_setting(&agent.join("settings.json"))
            {
                SessionDirSetting::Path(path) => Some(path),
                SessionDirSetting::Invalid(path) => {
                    invalid_settings.push(path);
                    Some(default)
                }
                SessionDirSetting::Missing | SessionDirSetting::Reset => Some(default),
            },
        }
    });
    let Some(root) = root else {
        return Vec::new();
    };
    let mut files = find_jsonl(&root);
    if let Some(parent) = root.parent() {
        files.extend(find_jsonl(&parent.join("session-artifacts")));
    }
    files.extend(invalid_settings);
    files.sort();
    files.dedup();
    files
}

fn valid_omp_profile(profile: &str) -> bool {
    let bytes = profile.as_bytes();
    let valid_chars = !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte)
        });
    let basename = profile.split('.').next().unwrap_or_default();
    let reserved = matches!(
        basename.to_ascii_lowercase().as_str(),
        "con" | "prn" | "aux" | "nul"
    ) || (basename.len() == 4
        && matches!(&basename[..3].to_ascii_lowercase(), value if value == "com" || value == "lpt")
        && basename.as_bytes()[3].is_ascii_digit());
    valid_chars && profile != "." && profile != ".." && !profile.ends_with('.') && !reserved
}

fn normalize_profile(value: Option<String>) -> Result<Option<String>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value == "default" {
        Ok(None)
    } else if valid_omp_profile(value) {
        Ok(Some(value.to_string()))
    } else {
        Err(())
    }
}

fn omp_profile() -> Result<Option<String>, ()> {
    let value = match env::var("OMP_PROFILE") {
        Ok(value) => Some(value),
        Err(_) => env::var("PI_PROFILE").ok(),
    };
    normalize_profile(value)
}

fn profile_derived_agent(config: &Path, agent: &Path) -> bool {
    let Ok(Some(profile)) = normalize_profile(env::var("PI_PROFILE").ok()) else {
        return false;
    };
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let derived = home_config_root(&home, config)
        .join("profiles")
        .join(profile)
        .join("agent");
    resolved_path(agent.to_path_buf()) == resolved_path(derived)
}

pub(super) fn find_omp_files() -> Vec<PathBuf> {
    let Ok(profile) = omp_profile() else {
        return vec![PathBuf::from("invalid-omp-profile.jsonl")];
    };
    let config = env::var("PI_CONFIG_DIR")
        .ok()
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(".omp"), PathBuf::from);
    let root = if let Some(sessions) = literal_env_path("PI_CODING_AGENT_SESSION_DIR") {
        Some(sessions)
    } else if let Some(profile) = profile {
        xdg_app_root("omp", &["profiles", &profile])
            .map(|root| root.join("sessions"))
            .or_else(|| {
                dirs::home_dir().map(|home| {
                    home_config_root(&home, &config)
                        .join("profiles")
                        .join(profile)
                        .join("agent/sessions")
                })
            })
    } else {
        let home = dirs::home_dir();
        let default_agent = home
            .as_deref()
            .map(|home| resolved_path(home_config_root(home, &config).join("agent")));
        let agent = literal_env_path("PI_CODING_AGENT_DIR")
            .filter(|agent| !profile_derived_agent(&config, agent))
            .map(resolved_path);
        if agent
            .as_ref()
            .is_none_or(|agent| Some(agent) == default_agent.as_ref())
        {
            xdg_app_root("omp", &[])
                .map(|xdg| xdg.join("sessions"))
                .or_else(|| default_agent.map(|agent| agent.join("sessions")))
        } else {
            agent.map(|agent| agent.join("sessions"))
        }
    };
    root.as_deref().map(find_jsonl).unwrap_or_default()
}
