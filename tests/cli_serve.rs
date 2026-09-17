mod common;

use common::{run_ccstats, unique_temp_dir};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn serve_rejects_non_loopback_bind() {
    let home = unique_temp_dir("serve-bind");
    let (ok, _stdout, stderr) = run_ccstats(
        &["serve", "--bind", "0.0.0.0:17991", "--offline"],
        &[("HOME", &home)],
    );
    assert!(!ok);
    let err = String::from_utf8_lossy(&stderr);
    assert!(err.contains("loopback"), "{err}");
    let _ = fs::remove_dir_all(home);
}

#[test]
fn serve_diagnostics_matches_doctor_json_shape() {
    let home = unique_temp_dir("serve-diag");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bin = env!("CARGO_BIN_EXE_ccstats");
    let mut child = Command::new(bin)
        .args(["serve", "--bind", &format!("127.0.0.1:{port}"), "--offline"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env_remove("CURSOR_API_KEY")
        .env_remove("CURSOR_SESSION_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    let mut connected = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            connected = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(connected, "server did not bind");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /v1/diagnostics HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("");
    let value: Value = serde_json::from_str(body).unwrap();
    assert!(
        value
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "claude")
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(home);
}
