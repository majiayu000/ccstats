use std::collections::HashMap;

use super::types::ModelPricing;

pub(super) fn parse_litellm_data(
    data: HashMap<String, serde_json::Value>,
) -> HashMap<String, ModelPricing> {
    let mut models = HashMap::new();

    for (name, value) in data {
        // Load Claude models and OpenAI GPT models
        let is_claude = name.contains("claude");
        let is_openai = name.starts_with("openai/") || name.starts_with("gpt-");

        if !is_claude && !is_openai {
            continue;
        }

        let input = value
            .get("input_cost_per_token")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let output = value
            .get("output_cost_per_token")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let reasoning_output = value
            .get("reasoning_output_cost_per_token")
            .or_else(|| value.get("reasoning_cost_per_token"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(output);

        let pricing = ModelPricing {
            input,
            output,
            reasoning_output,
            cache_read: value
                .get("cache_read_input_token_cost")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0),
            cache_create: value
                .get("cache_creation_input_token_cost")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0),
        };

        // Store with multiple key variations for matching
        models.insert(name.clone(), pricing.clone());

        // Also store normalized versions
        if is_claude {
            let normalized = name.replace("claude-", "").replace("anthropic.", "");
            models.insert(normalized, pricing);
        } else if is_openai {
            // Store without openai/ prefix
            if let Some(stripped) = name.strip_prefix("openai/") {
                models.insert(stripped.to_string(), pricing);
            }
        }
    }

    models
}

pub(super) fn resolve_pricing_known(
    model: &str,
    models: &HashMap<String, ModelPricing>,
) -> Option<ModelPricing> {
    // Try exact match first
    if let Some(pricing) = models.get(model) {
        return Some(pricing.clone());
    }

    // Try with claude- prefix
    let with_prefix = format!("claude-{model}");
    if let Some(pricing) = models.get(&with_prefix) {
        return Some(pricing.clone());
    }

    // Try partial matching
    let model_lower = model.to_lowercase();
    let mut candidates: Vec<(&String, &ModelPricing)> = models
        .iter()
        .filter(|(name, _)| {
            let name_lower = name.to_lowercase();
            name_lower.contains(&model_lower) || model_lower.contains(&name_lower)
        })
        .collect();
    candidates.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    if let Some((_, pricing)) = candidates.first() {
        return Some((*pricing).clone());
    }

    None
}

pub(super) fn fallback_pricing(model: &str) -> ModelPricing {
    let model_lower = model.to_lowercase();
    if model_lower.contains("opus-4-5") || model_lower.contains("opus-4.5") {
        ModelPricing {
            input: 5e-6,   // $5/M
            output: 25e-6, // $25/M
            reasoning_output: 25e-6,
            cache_create: 6.25e-6, // $6.25/M
            cache_read: 0.5e-6,    // $0.5/M
        }
    } else if model_lower.contains("opus") {
        ModelPricing {
            input: 15e-6,
            output: 75e-6,
            reasoning_output: 75e-6,
            cache_create: 18.75e-6,
            cache_read: 1.5e-6,
        }
    } else if model_lower.contains("sonnet") {
        ModelPricing {
            input: 3e-6,
            output: 15e-6,
            reasoning_output: 15e-6,
            cache_create: 3.75e-6,
            cache_read: 0.3e-6,
        }
    } else if model_lower.contains("haiku") {
        ModelPricing {
            input: 0.8e-6,
            output: 4e-6,
            reasoning_output: 4e-6,
            cache_create: 1e-6,
            cache_read: 0.08e-6,
        }
    } else if model_lower.contains("gpt-5") || model_lower.contains("codex") {
        // GPT-5 / Codex pricing (approximate)
        ModelPricing {
            input: 1.25e-6, // $1.25/M
            output: 10e-6,  // $10/M
            reasoning_output: 10e-6,
            cache_create: 0.0,
            cache_read: 0.125e-6, // $0.125/M
        }
    } else if model_lower.contains("gpt-4") {
        ModelPricing {
            input: 2.5e-6,
            output: 10e-6,
            reasoning_output: 10e-6,
            cache_create: 0.0,
            cache_read: 0.0,
        }
    } else {
        // Default to sonnet pricing for unknown models
        ModelPricing {
            input: 3e-6,
            output: 15e-6,
            reasoning_output: 15e-6,
            cache_create: 3.75e-6,
            cache_read: 0.3e-6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_litellm_entry(input: f64, output: f64) -> serde_json::Value {
        json!({
            "input_cost_per_token": input,
            "output_cost_per_token": output,
        })
    }

    // --- parse_litellm_data tests ---

    #[test]
    fn test_parse_filters_non_claude_non_openai() {
        let mut data = HashMap::new();
        data.insert("mistral/large".to_string(), make_litellm_entry(1e-6, 2e-6));
        data.insert("google/gemini".to_string(), make_litellm_entry(1e-6, 2e-6));

        let result = parse_litellm_data(data);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_claude_model() {
        let mut data = HashMap::new();
        data.insert(
            "claude-sonnet-4-20250514".to_string(),
            json!({
                "input_cost_per_token": 3e-6,
                "output_cost_per_token": 15e-6,
                "cache_read_input_token_cost": 0.3e-6,
                "cache_creation_input_token_cost": 3.75e-6,
            }),
        );

        let result = parse_litellm_data(data);
        // Original key + normalized key (without "claude-")
        assert!(result.contains_key("claude-sonnet-4-20250514"));
        assert!(result.contains_key("sonnet-4-20250514"));

        let pricing = &result["claude-sonnet-4-20250514"];
        assert_eq!(pricing.input, 3e-6);
        assert_eq!(pricing.output, 15e-6);
        assert_eq!(pricing.cache_read, 0.3e-6);
        assert_eq!(pricing.cache_create, 3.75e-6);
    }

    #[test]
    fn test_parse_openai_model() {
        let mut data = HashMap::new();
        data.insert(
            "openai/gpt-4o".to_string(),
            make_litellm_entry(2.5e-6, 10e-6),
        );

        let result = parse_litellm_data(data);
        assert!(result.contains_key("openai/gpt-4o"));
        assert!(result.contains_key("gpt-4o")); // stripped prefix
    }

    #[test]
    fn test_parse_reasoning_cost_fallback() {
        let mut data = HashMap::new();
        data.insert(
            "claude-opus-4-20250514".to_string(),
            json!({
                "input_cost_per_token": 15e-6,
                "output_cost_per_token": 75e-6,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-opus-4-20250514"];
        // reasoning_output should fall back to output cost
        assert_eq!(pricing.reasoning_output, 75e-6);
    }

    // --- resolve_pricing_known tests ---

    #[test]
    fn test_resolve_exact_match() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("claude-sonnet-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_with_claude_prefix() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        // Query without prefix, should match "claude-sonnet-4"
        let result = resolve_pricing_known("sonnet-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_partial_match() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        // Partial match: "sonnet-4" is contained in "claude-sonnet-4-20250514"
        let result = resolve_pricing_known("claude-sonnet-4-20250514", &models);
        assert!(result.is_some());
    }

    #[test]
    fn test_resolve_no_match() {
        let models: HashMap<String, ModelPricing> = HashMap::new();
        let result = resolve_pricing_known("nonexistent-model", &models);
        assert!(result.is_none());
    }

    // --- fallback_pricing tests ---

    #[test]
    fn test_fallback_opus_4_5() {
        let p = fallback_pricing("claude-opus-4-5-20250514");
        assert_eq!(p.input, 5e-6);
        assert_eq!(p.output, 25e-6);
    }

    #[test]
    fn test_fallback_opus() {
        let p = fallback_pricing("claude-opus-4-20250514");
        assert_eq!(p.input, 15e-6);
        assert_eq!(p.output, 75e-6);
    }

    #[test]
    fn test_fallback_sonnet() {
        let p = fallback_pricing("claude-sonnet-4-20250514");
        assert_eq!(p.input, 3e-6);
        assert_eq!(p.output, 15e-6);
    }

    #[test]
    fn test_fallback_haiku() {
        let p = fallback_pricing("claude-haiku-3.5");
        assert_eq!(p.input, 0.8e-6);
        assert_eq!(p.output, 4e-6);
    }

    #[test]
    fn test_fallback_gpt5() {
        let p = fallback_pricing("gpt-5-turbo");
        assert_eq!(p.input, 1.25e-6);
        assert_eq!(p.output, 10e-6);
    }

    #[test]
    fn test_fallback_codex() {
        let p = fallback_pricing("codex-mini");
        assert_eq!(p.input, 1.25e-6);
    }

    #[test]
    fn test_fallback_unknown_defaults_to_sonnet() {
        let p = fallback_pricing("totally-unknown-model");
        assert_eq!(p.input, 3e-6);
        assert_eq!(p.output, 15e-6);
    }

    // --- resolve_pricing_known: partial matching boundary tests ---

    #[test]
    fn test_resolve_case_insensitive_partial() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        // Mixed case query should still match via case-insensitive containment
        let result = resolve_pricing_known("Claude-Sonnet-4-20250514", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_partial_substring_in_model_name() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        // Query is a substring of a stored key
        let result = resolve_pricing_known("sonnet-4-20250514", &models);
        assert!(result.is_some());
    }

    #[test]
    fn test_resolve_partial_model_name_in_query() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        // Stored key is a substring of the query (bidirectional containment)
        let result = resolve_pricing_known("claude-sonnet-4-20250514", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_longest_match_wins() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet".to_string(),
            ModelPricing {
                input: 1e-6,
                ..Default::default()
            },
        );
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        // Both match "sonnet-4-20250514", but the longer key should win
        let result = resolve_pricing_known("sonnet-4-20250514", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_exact_match_takes_priority_over_partial() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 99e-6,
                ..Default::default()
            },
        );
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        // Exact match should be returned immediately, not partial
        let result = resolve_pricing_known("sonnet-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 99e-6);
    }

    #[test]
    fn test_resolve_claude_prefix_fallback() {
        let mut models = HashMap::new();
        models.insert(
            "claude-opus-4".to_string(),
            ModelPricing {
                input: 15e-6,
                ..Default::default()
            },
        );

        // "opus-4" → tries "claude-opus-4" prefix match before partial
        let result = resolve_pricing_known("opus-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 15e-6);
    }

    #[test]
    fn test_resolve_empty_models_map() {
        let models: HashMap<String, ModelPricing> = HashMap::new();
        assert!(resolve_pricing_known("claude-sonnet-4", &models).is_none());
    }

    #[test]
    fn test_resolve_empty_model_string() {
        let mut models = HashMap::new();
        models.insert("claude-sonnet-4".to_string(), ModelPricing::default());

        // Empty string is contained in every string, so it matches
        let result = resolve_pricing_known("", &models);
        // "" → tries exact match (no), tries "claude-" prefix (no),
        // then partial: "" is contained in every key → matches longest
        assert!(result.is_some());
    }

    // --- parse_litellm_data boundary tests ---

    #[test]
    fn test_parse_gpt_prefix_without_openai_slash() {
        let mut data = HashMap::new();
        data.insert("gpt-4o".to_string(), make_litellm_entry(2.5e-6, 10e-6));

        let result = parse_litellm_data(data);
        // "gpt-" prefix is recognized as OpenAI
        assert!(result.contains_key("gpt-4o"));
        // No "openai/" prefix to strip, so no extra key
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_anthropic_dot_prefix_normalized() {
        let mut data = HashMap::new();
        data.insert(
            "anthropic.claude-sonnet-4".to_string(),
            make_litellm_entry(3e-6, 15e-6),
        );

        let result = parse_litellm_data(data);
        // Original key stored
        assert!(result.contains_key("anthropic.claude-sonnet-4"));
        // Normalized: remove "claude-" and "anthropic." → "sonnet-4"
        assert!(result.contains_key("sonnet-4"));
    }

    #[test]
    fn test_parse_reasoning_cost_primary_key() {
        let mut data = HashMap::new();
        data.insert(
            "claude-opus-4".to_string(),
            json!({
                "input_cost_per_token": 15e-6,
                "output_cost_per_token": 75e-6,
                "reasoning_output_cost_per_token": 100e-6,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-opus-4"];
        // Primary key takes precedence
        assert_eq!(pricing.reasoning_output, 100e-6);
    }

    #[test]
    fn test_parse_reasoning_cost_alternate_key() {
        let mut data = HashMap::new();
        data.insert(
            "claude-opus-4".to_string(),
            json!({
                "input_cost_per_token": 15e-6,
                "output_cost_per_token": 75e-6,
                "reasoning_cost_per_token": 80e-6,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-opus-4"];
        // Falls back to reasoning_cost_per_token
        assert_eq!(pricing.reasoning_output, 80e-6);
    }

    #[test]
    fn test_parse_missing_cost_fields_default_to_zero() {
        let mut data = HashMap::new();
        data.insert("claude-test".to_string(), json!({}));

        let result = parse_litellm_data(data);
        let pricing = &result["claude-test"];
        assert_eq!(pricing.input, 0.0);
        assert_eq!(pricing.output, 0.0);
        assert_eq!(pricing.cache_read, 0.0);
        assert_eq!(pricing.cache_create, 0.0);
        // reasoning_output defaults to output (which is 0.0)
        assert_eq!(pricing.reasoning_output, 0.0);
    }

    // --- fallback_pricing boundary tests ---

    #[test]
    fn test_fallback_opus_4_5_dot_variant() {
        let p = fallback_pricing("claude-opus-4.5");
        assert_eq!(p.input, 5e-6);
        assert_eq!(p.output, 25e-6);
    }

    #[test]
    fn test_fallback_case_insensitive() {
        let p = fallback_pricing("Claude-OPUS-4-20250514");
        assert_eq!(p.input, 15e-6);

        let p2 = fallback_pricing("CLAUDE-HAIKU-3.5");
        assert_eq!(p2.input, 0.8e-6);
    }

    #[test]
    fn test_fallback_gpt4() {
        let p = fallback_pricing("gpt-4o-mini");
        assert_eq!(p.input, 2.5e-6);
        assert_eq!(p.output, 10e-6);
    }
}
