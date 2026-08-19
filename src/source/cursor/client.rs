//! Cursor usage API client
//!
//! Enterprise teams authenticate with `CURSOR_API_KEY` against the official
//! Admin API. Individual and self-serve plans use `CURSOR_SESSION_TOKEN` against
//! the dashboard usage endpoints.

use std::env;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::{Value, json};

const ADMIN_EVENTS_URL: &str = "https://api.cursor.com/teams/filtered-usage-events";
const DASHBOARD_EVENTS_URL: &str = "https://cursor.com/api/dashboard/get-filtered-usage-events";
const DASHBOARD_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const DASHBOARD_ORIGIN: &str = "https://cursor.com";

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const PAGE_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_LOOKBACK_DAYS: i64 = 90;
const MAX_PAGES: usize = 100;
const ADMIN_PAGE_SIZE: u32 = 1000;
const DASHBOARD_PAGE_SIZE: u32 = 100;

pub(super) const API_KEY_ENV: &str = "CURSOR_API_KEY";
pub(super) const SESSION_TOKEN_ENV: &str = "CURSOR_SESSION_TOKEN";

#[derive(Debug, Clone, Copy)]
enum CursorApi {
    Admin,
    Dashboard,
}

pub(super) fn has_api_credentials() -> bool {
    env_nonempty(API_KEY_ENV) || env_nonempty(SESSION_TOKEN_ENV)
}

pub(super) fn fetch_usage_events(debug: bool) -> Result<Vec<Value>, String> {
    let api_key = env_nonempty(API_KEY_ENV).then(|| env::var(API_KEY_ENV).unwrap_or_default());
    let session_token =
        env_nonempty(SESSION_TOKEN_ENV).then(|| env::var(SESSION_TOKEN_ENV).unwrap_or_default());

    match (api_key, session_token) {
        (Some(api_key), _) => fetch_paginated(CursorApi::Admin, Some(&api_key), None, debug),
        (None, Some(token)) => fetch_paginated(CursorApi::Dashboard, None, Some(&token), debug),
        (None, None) => Err(
            "Cursor usage API credentials were not found. Set CURSOR_API_KEY or CURSOR_SESSION_TOKEN."
                .to_string(),
        ),
    }
}

fn fetch_paginated(
    api: CursorApi,
    api_key: Option<&str>,
    session_token: Option<&str>,
    debug: bool,
) -> Result<Vec<Value>, String> {
    let (start_ms, end_ms) = date_window(api, api_key, session_token, debug);
    let mut events = Vec::new();
    let mut page = 1u32;

    loop {
        if page as usize > MAX_PAGES {
            if debug {
                eprintln!("Cursor usage API stopped after {MAX_PAGES} pages");
            }
            break;
        }

        let body = page_body(api, start_ms, end_ms, page);
        let payload = post_json(api, api_key, session_token, &body)?;
        let page_events = super::parser::events_from_payload(&payload);
        let page_len = page_events.len();
        events.extend(page_events.into_iter().cloned());

        if page_len == 0 || !has_next_page(api, &payload, page, events.len()) {
            break;
        }

        page += 1;
        thread::sleep(PAGE_DELAY);
    }

    Ok(events)
}

fn date_window(
    api: CursorApi,
    api_key: Option<&str>,
    session_token: Option<&str>,
    debug: bool,
) -> (i64, i64) {
    let end = Utc::now();
    let default_start = end - chrono::Duration::days(DEFAULT_LOOKBACK_DAYS);
    if matches!(api, CursorApi::Dashboard)
        && let Some(start) = dashboard_billing_start(api_key, session_token, debug)
    {
        return (start, end.timestamp_millis());
    }
    (default_start.timestamp_millis(), end.timestamp_millis())
}

fn dashboard_billing_start(
    api_key: Option<&str>,
    session_token: Option<&str>,
    debug: bool,
) -> Option<i64> {
    let payload = get_json(DASHBOARD_SUMMARY_URL, api_key, session_token).ok()?;
    let start = payload.get("billingCycleStart")?.as_str()?;
    let parsed = start.parse::<chrono::DateTime<Utc>>().ok()?;
    if debug {
        eprintln!("Cursor billing cycle starts at {start}");
    }
    Some(parsed.timestamp_millis())
}

fn page_body(api: CursorApi, start_ms: i64, end_ms: i64, page: u32) -> Value {
    match api {
        CursorApi::Admin => json!({
            "startDate": start_ms,
            "endDate": end_ms,
            "page": page,
            "pageSize": ADMIN_PAGE_SIZE,
        }),
        CursorApi::Dashboard => json!({
            "startDate": start_ms.to_string(),
            "endDate": end_ms.to_string(),
            "page": page,
            "pageSize": DASHBOARD_PAGE_SIZE,
        }),
    }
}

fn has_next_page(api: CursorApi, payload: &Value, page: u32, fetched: usize) -> bool {
    if let Some(pagination) = payload.get("pagination") {
        if let Some(has_next) = pagination.get("hasNextPage").and_then(Value::as_bool) {
            return has_next;
        }
        if let Some(num_pages) = pagination.get("numPages").and_then(Value::as_u64) {
            return u64::from(page) < num_pages;
        }
    }

    let total = payload
        .get("totalUsageEventsCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let page_size = match api {
        CursorApi::Admin => ADMIN_PAGE_SIZE,
        CursorApi::Dashboard => DASHBOARD_PAGE_SIZE,
    };
    fetched < total as usize && (fetched as u32) >= page.saturating_sub(1) * page_size
}

fn post_json(
    api: CursorApi,
    api_key: Option<&str>,
    session_token: Option<&str>,
    body: &Value,
) -> Result<Value, String> {
    let url = match api {
        CursorApi::Admin => ADMIN_EVENTS_URL,
        CursorApi::Dashboard => DASHBOARD_EVENTS_URL,
    };
    let encoded = serde_json::to_vec(body).map_err(|err| err.to_string())?;
    let mut request = http_agent()
        .post(url)
        .header("Content-Type", "application/json");
    request = apply_auth(request, api, api_key, session_token);
    let response = request
        .send(encoded)
        .map_err(|err| format!("Cursor usage API request failed: {err}"))?;
    read_json_body(response)
}

fn get_json(
    url: &str,
    api_key: Option<&str>,
    session_token: Option<&str>,
) -> Result<Value, String> {
    let mut request = http_agent().get(url);
    request = apply_auth(request, CursorApi::Dashboard, api_key, session_token);
    let response = request
        .call()
        .map_err(|err| format!("Cursor usage API request failed: {err}"))?;
    read_json_body(response)
}

fn apply_auth<S>(
    mut request: ureq::RequestBuilder<S>,
    api: CursorApi,
    api_key: Option<&str>,
    session_token: Option<&str>,
) -> ureq::RequestBuilder<S> {
    if let Some(api_key) = api_key {
        request = request.header(
            "Authorization",
            format!("Basic {}", base64_encode(format!("{api_key}:").as_bytes())),
        );
    }
    if let Some(token) = session_token {
        request = request.header("Cookie", format!("WorkosCursorSessionToken={token}"));
    }
    if matches!(api, CursorApi::Dashboard) {
        request = request.header("Origin", DASHBOARD_ORIGIN);
    }
    request
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into()
}

fn read_json_body(response: ureq::http::Response<ureq::Body>) -> Result<Value, String> {
    let status = response.status();
    let mut body = response.into_body();
    let parsed: Value = serde_json::from_reader(body.as_reader())
        .map_err(|err| format!("Cursor usage API returned invalid JSON: {err}"))?;
    if !status.is_success() {
        let message = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        return Err(format!(
            "Cursor usage API returned HTTP {}: {message}",
            status.as_u16()
        ));
    }
    Ok(parsed)
}

fn env_nonempty(key: &str) -> bool {
    env::var(key).is_ok_and(|value| !value.trim().is_empty())
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index < input.len() {
        let b0 = input[index];
        let b1 = input.get(index + 1).copied().unwrap_or(0);
        let b2 = input.get(index + 2).copied().unwrap_or(0);
        let triple = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if index + 1 < input.len() {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if index + 2 < input.len() {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        index += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn base64_encode_basic_auth_credential() {
        assert_eq!(base64_encode(b"key:"), "a2V5Og==");
    }

    #[test]
    fn page_body_uses_numeric_dates_for_admin_api() {
        let body = page_body(CursorApi::Admin, 100, 200, 2);
        assert_eq!(body["startDate"], 100);
        assert_eq!(body["pageSize"], ADMIN_PAGE_SIZE);
        assert_eq!(body["page"], 2);
    }

    #[test]
    fn page_body_uses_string_dates_for_dashboard_api() {
        let body = page_body(CursorApi::Dashboard, 100, 200, 1);
        assert_eq!(body["startDate"], "100");
        assert_eq!(body["pageSize"], DASHBOARD_PAGE_SIZE);
    }

    #[test]
    fn has_next_page_reads_admin_pagination_flag() {
        let payload = json!({
            "pagination": {"hasNextPage": true, "numPages": 4},
            "totalUsageEventsCount": 4000
        });
        assert!(has_next_page(CursorApi::Admin, &payload, 1, 1000));
    }

    #[test]
    fn has_next_page_uses_total_count_for_dashboard() {
        let payload = json!({ "totalUsageEventsCount": 250 });
        assert!(has_next_page(CursorApi::Dashboard, &payload, 1, 100));
        assert!(!has_next_page(CursorApi::Dashboard, &payload, 3, 250));
    }

    #[test]
    fn default_date_window_is_bounded() {
        let (start, end) = date_window(CursorApi::Admin, None, None, false);
        assert!(end >= start);
        let start_dt = Utc.timestamp_millis_opt(start).single().unwrap();
        let end_dt = Utc.timestamp_millis_opt(end).single().unwrap();
        assert!(end_dt - start_dt <= chrono::Duration::days(DEFAULT_LOOKBACK_DAYS + 1));
    }
}
