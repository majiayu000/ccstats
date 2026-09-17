use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use chrono::NaiveDate;
use serde_json::json;

use crate::app::CommandContext;
use crate::catalog::{
    AnalysisFilter, diagnose_usage_sources, list_usage_sources, usage_analysis_with_cli_config,
};
use crate::limits_cmd::limits_json;
use crate::sdk::{SummaryOptions, UsageRange, UsageSource};

pub(crate) fn handle(ctx: &CommandContext<'_>) {
    let bind = ctx.cli.serve_bind();
    assert_loopback(bind);
    let listener = TcpListener::bind(bind).unwrap_or_else(|error| {
        eprintln!("Error: failed to bind {bind}: {error}");
        std::process::exit(1);
    });
    let local = listener.local_addr().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    eprintln!("ccstats serve listening on http://{local}  (loopback only)");
    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                if let Err(error) = handle_connection(&mut stream, ctx) {
                    eprintln!("serve: {error}");
                }
            }
            Err(error) => eprintln!("serve: {error}"),
        }
    }
}

fn assert_loopback(bind: &str) {
    let addrs = bind.to_socket_addrs().unwrap_or_else(|error| {
        eprintln!("Error: invalid --bind {bind}: {error}");
        std::process::exit(1);
    });
    let mut any = false;
    for addr in addrs {
        any = true;
        if !addr.ip().is_loopback() {
            eprintln!("Error: ccstats serve only binds loopback (127.0.0.1 / ::1), not {addr}");
            std::process::exit(1);
        }
    }
    if !any {
        eprintln!("Error: --bind {bind} did not resolve");
        std::process::exit(1);
    }
}

fn handle_connection(stream: &mut TcpStream, ctx: &CommandContext<'_>) -> Result<(), String> {
    let (method, path, query) = read_request(stream)?;
    let (status, body) = dispatch(&method, &path, &query, ctx);
    write_response(stream, status, &body)
}

pub(crate) fn dispatch(
    method: &str,
    path: &str,
    query: &str,
    ctx: &CommandContext<'_>,
) -> (u16, String) {
    if method != "GET" {
        return json_error(405, "only GET is supported");
    }
    match path {
        "/v1/diagnostics" => match diagnose_usage_sources() {
            Ok(value) => encode_ok(value),
            Err(error) => json_error(400, &error.to_string()),
        },
        "/v1/sources" => match list_usage_sources() {
            Ok(value) => encode_ok(value),
            Err(error) => json_error(400, &error.to_string()),
        },
        "/v1/analysis" => analysis(query, ctx),
        "/v1/limits" => match limits_json(ctx, query_param(query, "source").as_deref()) {
            Ok(body) => (200, body),
            Err(error) => json_error(400, &error),
        },
        _ => json_error(404, "unknown path"),
    }
}

fn analysis(query: &str, ctx: &CommandContext<'_>) -> (u16, String) {
    let source_name = query_param(query, "source").unwrap_or_else(|| "claude".to_string());
    let Ok(source) = source_name.parse::<UsageSource>() else {
        return json_error(400, &format!("unknown source '{source_name}'"));
    };
    let since = parse_date_param(query, "since");
    let until = parse_date_param(query, "until");
    if let (Some(Err(error)), _) | (_, Some(Err(error))) = (&since, &until) {
        return json_error(400, error);
    }
    let range = match (since.and_then(Result::ok), until.and_then(Result::ok)) {
        (None, None) => UsageRange::Today,
        (since, until) => UsageRange::DateRange { since, until },
    };
    let options = SummaryOptions {
        source,
        range,
        timezone: ctx.cli.timezone.clone(),
        offline: ctx.cli.offline,
        strict_pricing: ctx.cli.strict_pricing,
        currency: ctx.cli.currency.clone(),
    };
    match usage_analysis_with_cli_config(options, &AnalysisFilter::default()) {
        Ok(analysis) => encode_ok(analysis),
        Err(error) => json_error(400, &error.to_string()),
    }
}

fn parse_date_param(query: &str, key: &str) -> Option<Result<NaiveDate, String>> {
    query_param(query, key).map(|raw| {
        NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(&raw, "%Y%m%d"))
            .map_err(|_| format!("invalid {key} date '{raw}'"))
    })
}

fn encode_ok<T: serde::Serialize>(value: T) -> (u16, String) {
    match serde_json::to_string(&value) {
        Ok(body) => (200, body),
        Err(error) => json_error(500, &error.to_string()),
    }
}

fn json_error(status: u16, message: &str) -> (u16, String) {
    (status, json!({ "error": message }).to_string())
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| urlencoding_decode(value))
    })
}

fn urlencoding_decode(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte as char);
                    index += 3;
                } else {
                    out.push('%');
                    index += 1;
                }
            }
            byte => {
                out.push(byte as char);
                index += 1;
            }
        }
    }
    out
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, String), String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < 8192 {
        let n = stream.read(&mut byte).map_err(|error| error.to_string())?;
        if n == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/");
    let (path, query) = target
        .split_once('?')
        .map_or((target.to_string(), String::new()), |(path, query)| {
            (path.to_string(), query.to_string())
        });
    Ok((method, path, query))
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[test]
    fn query_param_reads_source() {
        assert_eq!(
            query_param("source=cursor&since=2026-01-01", "source").as_deref(),
            Some("cursor")
        );
        assert_eq!(query_param("source=claude", "missing"), None);
    }

    #[test]
    fn loopback_addrs_are_accepted() {
        let addr: SocketAddr = "127.0.0.1:17890".parse().unwrap();
        assert!(addr.ip().is_loopback());
    }
}
