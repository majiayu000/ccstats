//! Local ccstats credentials (`credentials.toml`), separate from `config.toml`.
//!
//! Search order matches the config directory family but uses a different file:
//! 1. `~/.config/ccstats/credentials.toml`
//! 2. `dirs::config_dir()/ccstats/credentials.toml` when that path is distinct
//!
//! Cursor HTTP clients read `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` first,
//! then this file. Never log secret values.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) const API_KEY_ENV: &str = "CURSOR_API_KEY";
pub(crate) const SESSION_TOKEN_ENV: &str = "CURSOR_SESSION_TOKEN";

const CREDENTIALS_FILE: &str = "credentials.toml";

#[derive(Debug, Error)]
pub(crate) enum CredentialsError {
    #[error("no credentials path is available")]
    NoPath,
    #[error("failed to create {}: {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read credentials {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse credentials file")]
    Parse,
    #[error("failed to serialize credentials")]
    Serialize,
    #[error("failed to write credentials {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialOrigin {
    Env,
    File,
}

impl CredentialOrigin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorAuth {
    ApiKey(String),
    SessionToken(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedCursorCredentials {
    pub(crate) auth: CursorAuth,
    pub(crate) origin: CredentialOrigin,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CredentialsFile {
    #[serde(default)]
    cursor: CursorCredentials,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CursorCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_token: Option<String>,
}

impl CursorCredentials {
    fn from_auth(auth: CursorAuth) -> Self {
        match auth {
            CursorAuth::ApiKey(api_key) => Self {
                api_key: Some(api_key),
                session_token: None,
            },
            CursorAuth::SessionToken(session_token) => Self {
                api_key: None,
                session_token: Some(session_token),
            },
        }
    }

    fn into_auth(self) -> Option<CursorAuth> {
        if let Some(api_key) = nonempty_opt(self.api_key.as_deref()) {
            return Some(CursorAuth::ApiKey(api_key));
        }
        nonempty_opt(self.session_token.as_deref()).map(CursorAuth::SessionToken)
    }

    fn is_empty(&self) -> bool {
        nonempty_opt(self.api_key.as_deref()).is_none()
            && nonempty_opt(self.session_token.as_deref()).is_none()
    }
}

/// Resolve Cursor API credentials: environment variables, then `credentials.toml`.
pub(crate) fn resolve_cursor_credentials() -> Option<ResolvedCursorCredentials> {
    if let Some(auth) = env_cursor_auth() {
        return Some(ResolvedCursorCredentials {
            auth,
            origin: CredentialOrigin::Env,
        });
    }
    match load_cursor_auth_from_file() {
        Ok(Some(auth)) => Some(ResolvedCursorCredentials {
            auth,
            origin: CredentialOrigin::File,
        }),
        Ok(None) | Err(_) => None,
    }
}

pub(crate) fn save_cursor_auth(auth: CursorAuth) -> Result<(), CredentialsError> {
    write_credentials_file(
        &write_path()?,
        &CredentialsFile {
            cursor: CursorCredentials::from_auth(auth),
        },
    )
}

pub(crate) fn clear_cursor_credentials() -> Result<(), CredentialsError> {
    let Some(path) = existing_credentials_path() else {
        return Ok(());
    };
    fs::remove_file(&path).map_err(|source| CredentialsError::Write { path, source })
}

fn env_cursor_auth() -> Option<CursorAuth> {
    if let Some(api_key) = env_nonempty(API_KEY_ENV) {
        return Some(CursorAuth::ApiKey(api_key));
    }
    env_nonempty(SESSION_TOKEN_ENV).map(CursorAuth::SessionToken)
}

fn load_cursor_auth_from_file() -> Result<Option<CursorAuth>, CredentialsError> {
    Ok(load_credentials_file()?.and_then(|file| file.cursor.into_auth()))
}

fn load_credentials_file() -> Result<Option<CredentialsFile>, CredentialsError> {
    for path in credentials_paths() {
        match read_credentials_file(&path) {
            Ok(Some(file)) => return Ok(Some(file)),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn read_credentials_file(path: &Path) -> Result<Option<CredentialsFile>, CredentialsError> {
    match fs::read_to_string(path) {
        Ok(content) => toml::from_str(&content)
            .map(Some)
            .map_err(|_| CredentialsError::Parse),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CredentialsError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_path() -> Result<PathBuf, CredentialsError> {
    existing_credentials_path()
        .or_else(|| credentials_paths().into_iter().next())
        .ok_or(CredentialsError::NoPath)
}

fn existing_credentials_path() -> Option<PathBuf> {
    credentials_paths().into_iter().find(|path| path.is_file())
}

/// Config-directory family used by `config.rs`, with `credentials.toml` as the filename.
pub(crate) fn credentials_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".config").join("ccstats").join(CREDENTIALS_FILE));
    }

    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("ccstats").join(CREDENTIALS_FILE);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    paths
}

fn write_credentials_file(path: &Path, file: &CredentialsFile) -> Result<(), CredentialsError> {
    if file.cursor.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(|source| CredentialsError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
        return Ok(());
    }

    let encoded = toml::to_string_pretty(file).map_err(|_| CredentialsError::Serialize)?;
    atomic_write(path, encoded.as_bytes())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), CredentialsError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CredentialsError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;

    let (temp_path, mut temp_file) = create_temp_file(parent)?;
    let write_result = temp_file
        .write_all(contents)
        .and_then(|()| temp_file.sync_all())
        .map_err(|source| CredentialsError::Write {
            path: temp_path.clone(),
            source,
        });
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        CredentialsError::Write {
            path: path.to_path_buf(),
            source,
        }
    })?;
    restrict_final_permissions(path);
    Ok(())
}

fn create_temp_file(parent: &Path) -> Result<(PathBuf, File), CredentialsError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    let process_id = std::process::id();

    for attempt in 0..32 {
        let temp_path = parent.join(format!(
            ".{CREDENTIALS_FILE}.{process_id}.{nanos}.{attempt}.tmp"
        ));
        match open_private_temp(&temp_path) {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(CredentialsError::Write {
                    path: temp_path,
                    source,
                });
            }
        }
    }

    Err(CredentialsError::Write {
        path: parent.join(format!(".{CREDENTIALS_FILE}.{process_id}.{nanos}.tmp")),
        source: io::Error::new(
            ErrorKind::AlreadyExists,
            "unable to allocate unique credentials temporary file",
        ),
    })
}

fn open_private_temp(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn restrict_final_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(source) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "Warning: could not set 0600 on {}: {source}",
                path.display()
            );
        }
    }
    #[cfg(windows)]
    {
        if let Err(source) = restrict_windows_acl(path) {
            eprintln!(
                "Warning: could not restrict ACLs on {}; the credentials file was still written ({source}).",
                path.display()
            );
        }
    }
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> io::Result<()> {
    let user = env::var("USERNAME").map_err(io::Error::other)?;
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{user}:(R,W)"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("icacls failed"))
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    env::var(key).ok().as_deref().and_then(nonempty_str)
}

fn nonempty_opt(value: Option<&str>) -> Option<String> {
    value.and_then(nonempty_str)
}

fn nonempty_str(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_paths_use_credentials_filename() {
        let paths = credentials_paths();
        assert!(!paths.is_empty());
        assert!(
            paths
                .iter()
                .all(|path| path.file_name().and_then(|name| name.to_str())
                    == Some(CREDENTIALS_FILE))
        );
        assert!(paths.iter().any(|path| {
            path.to_string_lossy()
                .contains(".config/ccstats/credentials.toml")
        }));
    }

    #[test]
    fn cursor_section_prefers_api_key_and_rejects_whitespace() {
        let both: CredentialsFile =
            toml::from_str("[cursor]\napi_key = \"key\"\nsession_token = \"cookie\"\n").unwrap();
        assert!(matches!(
            both.cursor.clone().into_auth(),
            Some(CursorAuth::ApiKey(key)) if key == "key"
        ));

        let blank: CredentialsFile = toml::from_str("[cursor]\nsession_token = \"   \"\n").unwrap();
        assert!(blank.cursor.is_empty());
        assert!(blank.cursor.into_auth().is_none());
    }

    #[test]
    fn serialize_cursor_keeps_only_one_secret() {
        let encoded = toml::to_string_pretty(&CredentialsFile {
            cursor: CursorCredentials::from_auth(CursorAuth::SessionToken("cookie".into())),
        })
        .unwrap();
        assert!(encoded.contains("session_token"));
        assert!(!encoded.contains("api_key"));
        let parsed: CredentialsFile = toml::from_str(&encoded).unwrap();
        assert!(matches!(
            parsed.cursor.into_auth(),
            Some(CursorAuth::SessionToken(token)) if token == "cookie"
        ));
    }
}
