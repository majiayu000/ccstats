//! `ccstats mcp`: a read-only Model Context Protocol server over stdio.
//!
//! Newline-delimited JSON-RPC 2.0 on stdin/stdout. Stdout carries protocol
//! messages only; diagnostics go to stderr. Tool results reuse the same JSON
//! as `ccstats limits --json`, the SDK [`CostSummary`](crate::CostSummary),
//! and the SDK source diagnostics.

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

use crate::app::CommandContext;
use crate::catalog::diagnose_usage_sources;
use crate::limits_cmd::limits_json;
use crate::sdk::{SummaryOptions, UsageRange, UsageSource, summarize_cost};
use crate::source::all_sources;

/// Newest first. Clients requesting another version get the newest one.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Read-only local usage ledger. Call get_limits before starting long or \
expensive work to see how much of each provider quota window is used. Call get_usage_summary for \
token and cost totals. Call doctor when a source returns no data. Unknown cost is null, never 0; \
rows with source \"estimated\" are inferred from local logs, not provider-reported.";

pub(crate) fn handle(ctx: &CommandContext<'_>) {
    eprintln!("ccstats mcp: serving Model Context Protocol on stdio");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("ccstats mcp: stdin read failed: {error}");
                return;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle_message(&line, ctx) else {
            continue;
        };
        if let Err(error) = writeln!(stdout, "{response}").and_then(|()| stdout.flush()) {
            eprintln!("ccstats mcp: stdout write failed: {error}");
            return;
        }
    }
}

/// Handle one JSON-RPC message. Returns `None` for notifications and
/// client responses, which never get a reply.
fn handle_message(line: &str, ctx: &CommandContext<'_>) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            return Some(error_response(
                &Value::Null,
                PARSE_ERROR,
                &format!("parse error: {error}"),
            ));
        }
    };
    let Some(object) = message.as_object() else {
        return Some(error_response(
            &Value::Null,
            INVALID_REQUEST,
            "expected a single JSON-RPC object",
        ));
    };
    let id = object.get("id")?.clone();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // A response to a server request; this server never sends requests.
        return None;
    };
    let params = object.get("params").unwrap_or(&Value::Null);
    Some(match dispatch(method, params, ctx) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(&id, code, &message),
    })
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn dispatch(
    method: &str,
    params: &Value,
    ctx: &CommandContext<'_>,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(params, ctx),
        other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "ccstats", "version": crate::VERSION },
        "instructions": INSTRUCTIONS,
    })
}

fn tool_definitions() -> Value {
    let source_names: Vec<&str> = all_sources().map(crate::source::Source::name).collect();
    json!([
        {
            "name": "get_limits",
            "title": "Quota windows",
            "description": "Current quota / rate-limit windows for Claude Code, Codex, and Cursor. \
    Same JSON as `ccstats limits --json`. Read `windows[]`: each row has `provider`, `window` \
    (`five_hour`, `seven_day`, `weekly`, `billing_cycle`, or `estimated_5h`), `used_pct` (0-100; null \
    when unknown), `resets_at`, `source` (`official` = provider-reported, `estimated` = inferred from \
    local logs), and `stale`. Claude official windows appear only after `ccstats statusline` has \
    received Claude Code `rate_limits`. `notes[]` explains anything missing. Call this before \
    starting long or expensive work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "enum": ["all", "claude", "codex", "cursor"],
                        "default": "all",
                        "description": "Provider to report; `all` returns every provider."
                    }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": true }
        },
        {
            "name": "get_usage_summary",
            "title": "Usage summary",
            "description": "Token and cost totals for one source over the current period in the \
    configured timezone: `today`, `week` (Monday through today), or `month` (the 1st through today). \
    Returns the ccstats SDK CostSummary JSON: `tokens` (`input_tokens`, `output_tokens`, \
    `reasoning_tokens`, `cache_creation_tokens`, `cache_read_tokens`, `total_tokens`), `cost` / `cost_usd` (null when unknown, never a silent 0), `cost_kind`, \
    `pricing_source`, and per-model rows in `models`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "enum": source_names,
                        "description": "Usage source, e.g. `claude` or `codex`."
                    },
                    "period": {
                        "type": "string",
                        "enum": ["today", "week", "month"],
                        "default": "today",
                        "description": "Current period to summarize."
                    }
                },
                "required": ["source"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": true }
        },
        {
            "name": "diagnose",
            "title": "Claude quota diagnosis",
            "description": "Explain local Claude token composition, subagents, compaction boundaries and same-model/endpoint completed-turn version comparisons. Same JSON as ccstats diagnose --json. This is observed token volume, not subscription billing; missing cache fields and insufficient samples remain explicit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": { "type": "string", "enum": ["5h", "today", "7d"], "default": "5h" },
                    "session": { "type": "string", "description": "Local session ID; includes its recorded subagents." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "doctor",
            "title": "Source readiness",
            "description": "Readiness of every registered usage source, same data as `ccstats \
    doctor --json`: `name`, `status` (`detected`, `configured`, `missing`, or `error`), `files`, `detail`, and a \
    `setup` hint. Checks local paths and credential presence only; never contacts remote services. \
    Use when another tool reports no data.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}

fn call_tool(params: &Value, ctx: &CommandContext<'_>) -> Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((
            INVALID_PARAMS,
            "tools/call requires a tool name".to_string(),
        ));
    };
    let empty = Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Ok(tool_error("arguments must be a JSON object")),
    };
    let outcome = match name {
        "get_limits" => get_limits(arguments, ctx),
        "get_usage_summary" => get_usage_summary(arguments, ctx),
        "diagnose" => diagnose(arguments, ctx),
        "doctor" => doctor(arguments),
        other => return Err((INVALID_PARAMS, format!("unknown tool: {other}"))),
    };
    Ok(outcome.map_or_else(|message| tool_error(&message), tool_success))
}

fn tool_success(value: Value) -> Value {
    let mut result = json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "isError": false,
    });
    // structuredContent must be an object; array payloads stay text-only.
    if value.is_object() {
        result["structuredContent"] = value;
    }
    result
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

fn reject_unknown_arguments(
    arguments: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    match arguments
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        Some(key) => Err(format!("unknown argument '{key}'")),
        None => Ok(()),
    }
}

fn string_argument<'a>(
    arguments: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err(format!("argument '{key}' must be a string")),
    }
}

fn get_limits(arguments: &Map<String, Value>, ctx: &CommandContext<'_>) -> Result<Value, String> {
    reject_unknown_arguments(arguments, &["source"])?;
    let source = string_argument(arguments, "source")?;
    let body = limits_json(ctx, source)?;
    serde_json::from_str(&body).map_err(|error| format!("invalid limits JSON: {error}"))
}

fn get_usage_summary(
    arguments: &Map<String, Value>,
    ctx: &CommandContext<'_>,
) -> Result<Value, String> {
    reject_unknown_arguments(arguments, &["source", "period"])?;
    let Some(source_name) = string_argument(arguments, "source")? else {
        return Err("argument 'source' is required".to_string());
    };
    let source = source_name
        .parse::<UsageSource>()
        .map_err(|error| error.to_string())?;
    let range = match string_argument(arguments, "period")?.unwrap_or("today") {
        "today" => UsageRange::Today,
        "week" => UsageRange::ThisWeek,
        "month" => UsageRange::ThisMonth,
        other => {
            return Err(format!(
                "unknown period '{other}'; expected today, week, or month"
            ));
        }
    };
    let summary = summarize_cost(SummaryOptions {
        source,
        range,
        timezone: ctx.cli.timezone.clone(),
        offline: ctx.cli.offline,
        strict_pricing: ctx.cli.strict_pricing,
        currency: ctx.cli.currency.clone(),
    })
    .map_err(|error| error.to_string())?;
    serde_json::to_value(summary).map_err(|error| error.to_string())
}

fn diagnose(arguments: &Map<String, Value>, ctx: &CommandContext<'_>) -> Result<Value, String> {
    reject_unknown_arguments(arguments, &["window", "session"])?;
    let window = match string_argument(arguments, "window")?.unwrap_or("5h") {
        "5h" => crate::diagnose_cmd::DiagnoseWindow::FiveHours,
        "today" => crate::diagnose_cmd::DiagnoseWindow::Today,
        "7d" => crate::diagnose_cmd::DiagnoseWindow::SevenDays,
        other => {
            return Err(format!(
                "unknown window '{other}'; expected 5h, today, or 7d"
            ));
        }
    };
    let report = crate::diagnose_cmd::report(ctx, window, string_argument(arguments, "session")?)?;
    serde_json::to_value(report).map_err(|error| error.to_string())
}

fn doctor(arguments: &Map<String, Value>) -> Result<Value, String> {
    reject_unknown_arguments(arguments, &[])?;
    let diagnostics = diagnose_usage_sources().map_err(|error| error.to_string())?;
    serde_json::to_value(diagnostics).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_supported_version_and_falls_back_to_latest() {
        let known = initialize(&json!({ "protocolVersion": "2025-06-18" }));
        assert_eq!(known["protocolVersion"], "2025-06-18");
        let unknown = initialize(&json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(unknown["protocolVersion"], SUPPORTED_PROTOCOL_VERSIONS[0]);
        assert_eq!(unknown["serverInfo"]["name"], "ccstats");
    }

    #[test]
    fn tool_success_only_sets_structured_content_for_objects() {
        let object = tool_success(json!({ "a": 1 }));
        assert_eq!(object["structuredContent"]["a"], 1);
        let array = tool_success(json!([1, 2]));
        assert!(array.get("structuredContent").is_none());
        assert_eq!(array["content"][0]["text"], "[1,2]");
    }

    #[test]
    fn tool_definitions_list_every_source_for_usage_summary() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["get_limits", "get_usage_summary", "diagnose", "doctor"]
        );
        let sources = tools[1]["inputSchema"]["properties"]["source"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(sources.len(), all_sources().count());
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let mut arguments = Map::new();
        arguments.insert("bogus".to_string(), json!(1));
        assert!(reject_unknown_arguments(&arguments, &["source"]).is_err());
    }
}
