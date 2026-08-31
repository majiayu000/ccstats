use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::utils::Timezone;

const MAX_TOKEN_COUNT: i64 = 1_i64 << 40;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)] // names mirror the official TokenUsage schema
pub(super) struct Usage {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

fn checked_count(value: i64) -> Result<i64, &'static str> {
    (0..=MAX_TOKEN_COUNT)
        .contains(&value)
        .then_some(value)
        .ok_or("invalid token count")
}

pub(super) fn usage_entry(
    session_id: &str,
    cwd: Option<&str>,
    event_time: i64,
    model: String,
    usage: Usage,
    timezone: Timezone,
    identity: &str,
) -> Result<RawEntry, &'static str> {
    let input = checked_count(usage.input_tokens)?;
    let output = checked_count(usage.output_tokens)?;
    let cache_read = checked_count(usage.cache_read_tokens.unwrap_or(0))?;
    let cache_write = checked_count(usage.cache_write_tokens.unwrap_or(0))?;
    let reasoning = checked_count(usage.reasoning_tokens.unwrap_or(0))?;
    if reasoning > output {
        return Err("reasoning exceeds output");
    }
    let known_prompt = input
        .checked_add(cache_read)
        .and_then(|value| value.checked_add(cache_write))
        .ok_or("token total overflow")?;
    let component_total = known_prompt
        .checked_add(output)
        .ok_or("token total overflow")?;
    let reported_total = match usage.total_tokens {
        Some(total) => {
            let total = checked_count(total)?;
            let exact_prompt = total.checked_sub(output).ok_or("total below output")?;
            if exact_prompt < known_prompt
                || (usage.cache_read_tokens.is_some()
                    && usage.cache_write_tokens.is_some()
                    && total != component_total)
            {
                return Err("contradictory total token count");
            }
            Some(total)
        }
        None => None,
    };
    let timestamp =
        DateTime::<Utc>::from_timestamp_millis(event_time).ok_or("invalid timestamp")?;
    Ok(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: event_time,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id("dsh", identity)),
        session_key: format!("dsh::{session_id}"),
        session_id: session_id.to_string(),
        project_path: cwd.unwrap_or_default().to_string(),
        model,
        input_tokens: input,
        output_tokens: output - reasoning,
        cache_creation: cache_write,
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens: reasoning,
        stop_reason: Some("usage".to_string()),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: reported_total,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{Usage, usage_entry};
    use crate::utils::Timezone;

    #[test]
    fn dsh_entries_do_not_claim_grok_api_equivalent_coverage() {
        let entry = usage_entry(
            "session-a",
            Some("/workspace/project"),
            1_788_145_200_000,
            "deepseek-v4".to_string(),
            Usage {
                input_tokens: 3,
                output_tokens: 1,
                total_tokens: Some(4),
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
            },
            Timezone::Named(chrono_tz::UTC),
            "session-a:attempt:0",
        )
        .expect("valid DSH usage");

        assert_eq!(entry.api_equivalent_priced_tokens, 0);
        assert_eq!(entry.api_equivalent_coverage_tokens, 0);
    }
}
