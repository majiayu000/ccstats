use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ccstats-{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

pub(crate) fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write test file");
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
    let mut candidates = Vec::new();

    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(target_dir).join("debug").join(bin_name));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(
        manifest_dir
            .join("target")
            .join("llvm-cov-target")
            .join("debug")
            .join(bin_name),
    );
    candidates.push(manifest_dir.join("target").join("debug").join(bin_name));

    if let Some(bin) = candidates.iter().find(|path| path.is_file()) {
        return bin.clone();
    }

    panic!(
        "unable to locate ccstats binary; checked: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

pub(crate) fn run_ccstats(args: &[&str], envs: &[(&str, &Path)]) -> (bool, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(resolve_ccstats_binary());
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}
