use std::io::Write;
use std::process::{Command, Stdio};

/// Pipe JSON through jq with the given filter expression
pub(crate) fn filter_json(json: &str, filter: &str) -> Result<String, String> {
    let mut child = Command::new("jq")
        .arg(filter)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "jq not found. Please install jq to use --jq option.".to_string()
            } else {
                format!("Failed to run jq: {}", e)
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(json.as_bytes())
            .map_err(|e| format!("Failed to write to jq stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for jq: {}", e))?;

    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 from jq: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("jq error: {}", stderr.trim()))
    }
}
