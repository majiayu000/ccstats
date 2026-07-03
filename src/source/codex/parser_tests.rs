use super::*;

#[test]
fn test_subtract_normal() {
    let total = TokenUsage {
        input_tokens: Some(1000),
        cached_input_tokens: Some(200),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(500),
        reasoning_output_tokens: Some(100),
        total_tokens: Some(1500),
    };
    let prev = TokenUsage {
        input_tokens: Some(400),
        cached_input_tokens: Some(100),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(200),
        reasoning_output_tokens: Some(50),
        total_tokens: Some(600),
    };
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(600));
    assert_eq!(delta.cached_input_tokens, Some(100));
    assert_eq!(delta.output_tokens, Some(300));
    assert_eq!(delta.reasoning_output_tokens, Some(50));
    assert_eq!(delta.total_tokens, Some(900));
}

#[test]
fn test_subtract_clamps_negative_to_zero() {
    let total = TokenUsage {
        input_tokens: Some(100),
        cached_input_tokens: Some(50),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(10),
        reasoning_output_tokens: Some(0),
        total_tokens: Some(110),
    };
    let prev = TokenUsage {
        input_tokens: Some(500),
        cached_input_tokens: Some(200),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(300),
        reasoning_output_tokens: Some(100),
        total_tokens: Some(800),
    };
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(0));
    assert_eq!(delta.cached_input_tokens, Some(0));
    assert_eq!(delta.output_tokens, Some(0));
    assert_eq!(delta.reasoning_output_tokens, Some(0));
    assert_eq!(delta.total_tokens, Some(0));
}

#[test]
fn test_subtract_none_fields_treated_as_zero() {
    let total = TokenUsage {
        input_tokens: Some(100),
        ..Default::default()
    };
    let prev = TokenUsage::default();
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(100));
    assert_eq!(delta.output_tokens, Some(0));
    assert_eq!(delta.reasoning_output_tokens, Some(0));
}

#[test]
fn test_usage_totals_duplicate_when_complete_vector_matches() {
    let prev = UsageTotals {
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };
    let total = UsageTotals {
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };

    assert!(total.is_duplicate_of(&prev));
}

#[test]
fn test_usage_totals_not_duplicate_when_component_grows_with_zero_total() {
    let prev = UsageTotals {
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };
    let total = UsageTotals {
        input_tokens: 150,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };

    assert!(!total.is_duplicate_of(&prev));
    assert_eq!(total.subtract(prev).input_tokens, 50);
}

#[test]
fn test_is_empty_default() {
    assert!(TokenUsage::default().is_empty());
}

#[test]
fn test_is_empty_with_input() {
    let usage = TokenUsage {
        input_tokens: Some(1),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_is_empty_with_cached_only() {
    let usage = TokenUsage {
        cached_input_tokens: Some(50),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_is_empty_with_reasoning_only() {
    let usage = TokenUsage {
        reasoning_output_tokens: Some(10),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_cached_input_prefers_cached_input_tokens() {
    let usage = TokenUsage {
        cached_input_tokens: Some(100),
        alt_cache_read_input_tokens: Some(50),
        ..Default::default()
    };
    assert_eq!(usage.cached_input(), 100);
}

#[test]
fn test_cached_input_falls_back_to_cache_read() {
    let usage = TokenUsage {
        cached_input_tokens: None,
        alt_cache_read_input_tokens: Some(75),
        ..Default::default()
    };
    assert_eq!(usage.cached_input(), 75);
}

#[test]
fn test_cached_input_both_none_returns_zero() {
    let usage = TokenUsage::default();
    assert_eq!(usage.cached_input(), 0);
}

#[test]
fn test_extract_model_from_info_model() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("fallback-model"),
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: Some("info-model"),
            model_name: Some("info-model-name"),
            metadata: Some(Metadata {
                model: Some("meta-model"),
            }),
        }),
    };
    assert_eq!(extract_model(&payload), Some("info-model".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_model_name() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("fallback"),
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: Some("model-name"),
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("model-name".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_metadata() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("fallback"),
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: None,
            metadata: Some(Metadata {
                model: Some("meta-model"),
            }),
        }),
    };
    assert_eq!(extract_model(&payload), Some("meta-model".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_payload_model() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("payload-model"),
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: None,
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("payload-model".to_string()));
}

#[test]
fn test_extract_model_no_info_uses_payload() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("payload-only"),
        info: None,
    };
    assert_eq!(extract_model(&payload), Some("payload-only".to_string()));
}

#[test]
fn test_extract_model_all_none_returns_none() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: None,
        info: None,
    };
    assert_eq!(extract_model(&payload), None);
}

#[test]
fn test_extract_model_empty_strings_skipped() {
    let payload = Payload {
        payload_type: None,
        id: None,
        model: Some("real-model"),
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: Some("  "),
            model_name: Some(""),
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("real-model".to_string()));
}
