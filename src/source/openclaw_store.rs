//! `OpenClaw` current transcript discovery and storage readers.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

const OPENCLAW_CONFIG_PATH_ENV: &str = "OPENCLAW_CONFIG_PATH";
const OPENCLAW_HOME_ENV: &str = "OPENCLAW_HOME";
const OPENCLAW_STATE_DIR_ENV: &str = "OPENCLAW_STATE_DIR";

pub(super) fn find_transcript_stores() -> Vec<PathBuf> {
    let Some(root) = state_dir() else {
        return Vec::new();
    };
    let Some(home) = effective_home() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for pattern in [
        root.join("agents/*/sessions/*"),
        root.join("agents/*/agent/openclaw-agent.sqlite"),
    ] {
        paths.extend(
            glob::glob(&pattern.to_string_lossy())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|path| path.is_file())
                .filter(|path| is_counted_transcript(path) || is_sqlite_store(path)),
        );
    }
    match configured_stores(&root, &home) {
        Ok(configured) => paths.extend(configured),
        Err(config_path) => paths.push(config_path),
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(super) struct TranscriptLoad {
    pub(super) transcripts: Vec<Vec<String>>,
    pub(super) errors: usize,
}

pub(super) fn load_transcripts(path: &Path) -> Result<TranscriptLoad, String> {
    if is_sqlite_store(path) {
        load_sqlite(path)
    } else {
        read_jsonl_blob(
            &fs::read(path).map_err(|error| error.to_string())?,
            is_zstd(path),
        )
        .map(|lines| TranscriptLoad {
            transcripts: vec![lines],
            errors: 0,
        })
    }
}

fn effective_home() -> Option<PathBuf> {
    env::var_os(OPENCLAW_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

fn expand_openclaw_home(path: &Path, home: &Path) -> PathBuf {
    if path == Path::new("~") {
        return home.to_path_buf();
    }
    path.strip_prefix("~")
        .map_or_else(|_| path.to_path_buf(), |suffix| home.join(suffix))
}

fn state_dir() -> Option<PathBuf> {
    let home = effective_home()?;
    Some(
        env::var_os(OPENCLAW_STATE_DIR_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map_or_else(
                || home.join(".openclaw"),
                |path| expand_openclaw_home(&path, &home),
            ),
    )
}

#[derive(Deserialize, Default)]
struct OpenClawConfig {
    #[serde(default)]
    session: SessionConfig,
    #[serde(default)]
    agents: AgentsConfig,
}

#[derive(Deserialize, Default)]
struct SessionConfig {
    store: Option<String>,
}

#[derive(Deserialize, Default)]
struct AgentsConfig {
    #[serde(default)]
    list: Vec<AgentConfig>,
}

#[derive(Deserialize)]
struct AgentConfig {
    id: Option<String>,
    #[serde(rename = "agentDir")]
    agent_dir: Option<String>,
}

fn configured_stores(root: &Path, home: &Path) -> Result<Vec<PathBuf>, PathBuf> {
    let explicit_config = env::var_os(OPENCLAW_CONFIG_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let config_path = explicit_config.clone().map_or_else(
        || root.join("openclaw.json"),
        |path| resolve_path(&path, home),
    );
    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(_) if explicit_config.is_none() && !config_path.exists() => return Ok(Vec::new()),
        Err(_) => return Err(config_path),
    };
    let config = json5::from_str::<OpenClawConfig>(&content).map_err(|_| config_path.clone())?;
    let mut agent_ids = vec!["main"];
    agent_ids.extend(
        config
            .agents
            .list
            .iter()
            .filter_map(|agent| agent.id.as_deref())
            .filter(|id| !id.trim().is_empty()),
    );
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let mut paths = Vec::new();
    if let Some(store) = config.session.store {
        let expanded = if store.contains("{agentId}") {
            agent_ids
                .iter()
                .map(|id| store.replace("{agentId}", id))
                .collect::<Vec<_>>()
        } else {
            vec![store]
        };
        for store in expanded {
            paths.extend(configured_session_store_paths(&resolve_path(
                Path::new(&store),
                home,
            )));
        }
    }
    paths.extend(config.agents.list.into_iter().filter_map(|agent| {
        let agent_dir = agent.agent_dir?;
        let path = resolve_path(Path::new(&agent_dir), home).join("openclaw-agent.sqlite");
        path.is_file().then_some(path)
    }));
    Ok(paths)
}

fn configured_session_store_paths(store: &Path) -> Vec<PathBuf> {
    let exact_sqlite = is_sqlite_store(store);
    let target = if exact_sqlite {
        store.to_path_buf()
    } else {
        let parent = store.parent().unwrap_or_else(|| Path::new("."));
        if store.file_name().and_then(|name| name.to_str()) == Some("sessions.json") {
            if parent.file_name().and_then(|name| name.to_str()) == Some("sessions")
                && parent
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    == Some("agents")
            {
                parent
                    .parent()
                    .unwrap_or(parent)
                    .join("agent/openclaw-agent.sqlite")
            } else {
                parent.join("openclaw-agent.sqlite")
            }
        } else {
            let stem = store
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("openclaw-agent");
            parent.join(format!("{stem}.sqlite"))
        }
    };
    let mut paths = target
        .is_file()
        .then_some(target.clone())
        .into_iter()
        .collect::<Vec<_>>();
    if exact_sqlite {
        return paths;
    }
    if let (Some(parent), Some(stem)) = (
        target.parent(),
        target.file_stem().and_then(|stem| stem.to_str()),
    ) {
        let pattern = parent.join(format!("{stem}.*.sqlite"));
        paths.extend(
            glob::glob(&pattern.to_string_lossy())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|path| path.is_file()),
        );
    }
    paths
}

fn resolve_path(path: &Path, home: &Path) -> PathBuf {
    let expanded = expand_openclaw_home(path, home);
    if expanded.is_absolute() {
        expanded
    } else {
        env::current_dir().map_or(expanded.clone(), |cwd| cwd.join(expanded))
    }
}

fn is_sqlite_store(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sqlite"))
}

fn is_zstd(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("zst")
}

fn is_counted_transcript(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    if is_primary_transcript(name) {
        return true;
    }
    let uncompressed = name.strip_suffix(".zst").unwrap_or(name);
    ["reset", "deleted"].into_iter().any(|reason| {
        let marker = format!(".jsonl.{reason}.");
        uncompressed.rfind(&marker).is_some_and(|index| {
            index > 0
                && valid_archive_suffix(&uncompressed[index + marker.len()..])
                && is_primary_transcript(&format!("{}.jsonl", &uncompressed[..index]))
        })
    })
}

fn is_primary_transcript(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return false;
    };
    name != "sessions.json" && !name.ends_with(".trajectory.jsonl") && !is_checkpoint_stem(stem)
}

fn is_checkpoint_stem(stem: &str) -> bool {
    stem.rsplit_once(".checkpoint.")
        .is_some_and(|(session, uuid)| !session.is_empty() && valid_uuid(uuid))
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
        && matches!(value.as_bytes()[14], b'1'..=b'5')
        && matches!(
            value.as_bytes()[19].to_ascii_lowercase(),
            b'8' | b'9' | b'a' | b'b'
        )
}

fn valid_archive_suffix(value: &str) -> bool {
    let Some(z_index) = value.find('Z') else {
        return false;
    };
    let (stamp, generation) = value.split_at(z_index + 1);
    let stamp_shape = (stamp.len() == 20
        && matches!(
            stamp.as_bytes(),
            [
                _,
                _,
                _,
                _,
                b'-',
                _,
                _,
                b'-',
                _,
                _,
                b'T',
                _,
                _,
                b'-',
                _,
                _,
                b'-',
                _,
                _,
                b'Z'
            ]
        ))
        || (stamp.len() == 24
            && matches!(
                stamp.as_bytes(),
                [
                    _,
                    _,
                    _,
                    _,
                    b'-',
                    _,
                    _,
                    b'-',
                    _,
                    _,
                    b'T',
                    _,
                    _,
                    b'-',
                    _,
                    _,
                    b'-',
                    _,
                    _,
                    b'.',
                    _,
                    _,
                    _,
                    b'Z'
                ]
            ));
    stamp_shape
        && stamp.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        })
        && (generation.is_empty()
            || generation.strip_prefix('.').is_some_and(|hex| {
                hex.len() == 32 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            }))
}

fn read_jsonl_blob(bytes: &[u8], compressed: bool) -> Result<Vec<String>, String> {
    let decoded = if compressed {
        zstd::stream::decode_all(Cursor::new(bytes)).map_err(|error| error.to_string())?
    } else {
        bytes.to_vec()
    };
    let text = String::from_utf8(decoded).map_err(|error| error.to_string())?;
    Ok(text.lines().map(str::to_string).collect())
}

fn load_sqlite(path: &Path) -> Result<TranscriptLoad, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags).map_err(|error| error.to_string())?;
    let mut transcripts = BTreeMap::<String, Vec<String>>::new();
    let mut statement = connection
        .prepare("SELECT session_id, event_json FROM transcript_events ORDER BY session_id, seq")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut errors = 0;
    for row in rows {
        match row {
            Ok((session_id, event_json)) => {
                transcripts.entry(session_id).or_default().push(event_json);
            }
            Err(_) => errors += 1,
        }
    }
    drop(statement);

    let mut result = transcripts.into_values().collect::<Vec<_>>();
    let Ok(mut archives) = connection.prepare(
        "SELECT encoding, archive_blob FROM session_transcript_archives \
             ORDER BY session_id, generation",
    ) else {
        return Ok(TranscriptLoad {
            transcripts: result,
            errors: errors + 1,
        });
    };
    let Ok(rows) = archives.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    }) else {
        return Ok(TranscriptLoad {
            transcripts: result,
            errors: errors + 1,
        });
    };
    for row in rows {
        let Ok((encoding, blob)) = row else {
            errors += 1;
            continue;
        };
        let compressed = match encoding.as_str() {
            "identity" => false,
            "zstd" => true,
            _ => {
                errors += 1;
                continue;
            }
        };
        match read_jsonl_blob(&blob, compressed) {
            Ok(lines) => result.push(lines),
            Err(_) => errors += 1,
        }
    }
    Ok(TranscriptLoad {
        transcripts: result,
        errors,
    })
}
