use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ccstats-{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write test file");
}

fn write_pricing_cache(home: &Path, xdg_cache: &Path, contents: &str) {
    write_file(&xdg_cache.join("ccstats/pricing.json"), contents);
    write_file(&home.join("Library/Caches/ccstats/pricing.json"), contents);
    write_file(&home.join(".cache/ccstats/pricing.json"), contents);
}

fn resolve_ccstats_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("CARGO_BIN_EXE_ccstats") {
        return PathBuf::from(bin);
    }

    let bin_name = if cfg!(windows) {
        "ccstats.exe"
    } else {
        "ccstats"
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir
            .join("target")
            .join("llvm-cov-target")
            .join("debug")
            .join(bin_name),
        manifest_dir.join("target").join("debug").join(bin_name),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("unable to locate ccstats binary"))
}

fn run_ccstats(args: &[&str], envs: &[(&str, &Path)]) -> (bool, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(resolve_ccstats_binary());
    cmd.args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}

#[test]
fn statusline_offline_corrupt_pricing_cache_fails_closed() {
    let root = unique_temp_dir("statusline-corrupt-pricing");
    let xdg_cache = root.join("xdg-cache");
    write_pricing_cache(&root, &xdg_cache, "{not json");
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z").to_string();
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_1","model":"mystery-model","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}
"#
        ),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &["statusline", "-j", "-O", "--strict-pricing"],
        &[("HOME", &root), ("XDG_CACHE_HOME", &xdg_cache)],
    );

    assert!(!ok, "expected corrupt pricing cache failure");
    assert!(
        stdout.is_empty(),
        "stdout should not contain all-N/A statusline output: {}",
        String::from_utf8_lossy(&stdout)
    );
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("pricing cache"), "stderr: {stderr}");
    assert!(stderr.contains("malformed"), "stderr: {stderr}");

    let _ = fs::remove_dir_all(root);
}
