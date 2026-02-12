use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::JqError;

/// Pipe JSON through jq with the given filter expression
pub(crate) fn filter_json(json: &str, filter: &str) -> Result<String, JqError> {
    let mut child = Command::new("jq")
        .arg(filter)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                JqError::NotFound
            } else {
                JqError::Spawn(e)
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json.as_bytes()).map_err(JqError::Stdin)?;
    }

    let output = child.wait_with_output().map_err(JqError::Wait)?;

    if output.status.success() {
        String::from_utf8(output.stdout).map_err(JqError::Utf8)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(JqError::Filter(stderr.trim().to_string()))
    }
}
