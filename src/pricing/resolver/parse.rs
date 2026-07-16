use std::collections::HashMap;

use super::super::types::{ModelPricing, dot_version_variant};

pub(crate) fn parse_litellm_data(
    data: HashMap<String, serde_json::Value>,
) -> HashMap<String, ModelPricing> {
    let mut models = HashMap::new();

    for (name, value) in data {
        // Load Claude, OpenAI GPT/Codex, xAI Grok, and CN vendor model names.
        let is_claude = name.contains("claude");
        let is_openai =
            name.starts_with("openai/") || name.starts_with("gpt-") || name.starts_with("codex");
        let is_xai = name.starts_with("xai/") || name.starts_with("grok-");
        // Chinese vendors: DeepSeek, Qwen (Alibaba), GLM (Zhipu/zai), Moonshot/Kimi.
        let is_cn = name.contains("deepseek")
            || name.contains("qwen")
            || name.contains("glm")
            || name.starts_with("moonshot/")
            || name.contains("kimi");

        if !is_claude && !is_openai && !is_xai && !is_cn {
            continue;
        }

        let input = value
            .get("input_cost_per_token")
            .and_then(serde_json::Value::as_f64);
        let output = value
            .get("output_cost_per_token")
            .and_then(serde_json::Value::as_f64);
        // Skip metadata-only entries; missing pricing must not load as $0 known.
        if input.is_none() && output.is_none() {
            continue;
        }
        let input = input.unwrap_or(0.0);
        let output = output.unwrap_or(0.0);
        let reasoning_output = value
            .get("reasoning_output_cost_per_token")
            .or_else(|| value.get("reasoning_cost_per_token"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(output);

        let cache_create = value
            .get("cache_creation_input_token_cost")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);

        let pricing = ModelPricing {
            input,
            output,
            reasoning_output,
            cache_read: value
                .get("cache_read_input_token_cost")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0),
            cache_create,
            cache_create_1h: value
                .get("cache_creation_input_token_cost_above_1hr")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(cache_create),
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
        } else if is_xai && let Some(stripped) = name.strip_prefix("xai/") {
            models.insert(stripped.to_string(), pricing.clone());
            if stripped == "grok-build-0.1" {
                models.insert("grok-build".to_string(), pricing);
            }
        } else if is_cn {
            // Store the bare name (last path segment) plus a dot-version variant
            // so hoster spellings like `glm-5p2` (p == point) match a dot-spelled
            // `glm-5.2` alias. Official vendor prefixes win over hosters on
            // name collisions.
            let bare = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
            let name_lower = name.to_lowercase();
            let official = name.starts_with("zai/")
                || name.starts_with("deepseek/")
                || name.starts_with("dashscope/")
                || name.starts_with("moonshot/")
                || !name.contains('/');
            for variant in [bare.clone(), dot_version_variant(&bare)] {
                if variant != name_lower && !variant.is_empty() {
                    if official {
                        models.insert(variant, pricing.clone());
                    } else {
                        models.entry(variant).or_insert(pricing.clone());
                    }
                }
            }
        }
    }

    models
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_litellm_entry(input: f64, output: f64) -> serde_json::Value {
        json!({
            "input_cost_per_token": input,
            "output_cost_per_token": output,
        })
    }

    #[test]
    fn test_parse_filters_non_claude_non_openai() {
        let mut data = HashMap::new();
        data.insert("mistral/large".to_string(), make_litellm_entry(1e-6, 2e-6));
        data.insert("google/gemini".to_string(), make_litellm_entry(1e-6, 2e-6));

        let result = parse_litellm_data(data);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_cache_create_1h_rate() {
        let mut data = HashMap::new();
        data.insert(
            "claude-fable-5".to_string(),
            json!({
                "input_cost_per_token": 1e-5,
                "output_cost_per_token": 5e-5,
                "cache_creation_input_token_cost": 1.25e-5,
                "cache_creation_input_token_cost_above_1hr": 2e-5,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-fable-5"];
        assert_eq!(pricing.cache_create, 1.25e-5);
        assert_eq!(pricing.cache_create_1h, 2e-5);
    }

    #[test]
    fn test_parse_cache_create_1h_falls_back_to_5m_rate() {
        let mut data = HashMap::new();
        data.insert(
            "claude-legacy".to_string(),
            json!({
                "input_cost_per_token": 3e-6,
                "output_cost_per_token": 15e-6,
                "cache_creation_input_token_cost": 3.75e-6,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-legacy"];
        assert_eq!(pricing.cache_create_1h, 3.75e-6);
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
    fn test_parse_codex_model() {
        let mut data = HashMap::new();
        data.insert(
            "codex-mini-latest".to_string(),
            json!({
                "input_cost_per_token": 1.5e-6,
                "output_cost_per_token": 6e-6,
                "cache_read_input_token_cost": 0.375e-6,
            }),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["codex-mini-latest"];
        assert_eq!(pricing.input, 1.5e-6);
        assert_eq!(pricing.output, 6e-6);
        assert_eq!(pricing.reasoning_output, 6e-6);
        assert_eq!(pricing.cache_read, 0.375e-6);
    }

    #[test]
    fn test_parse_xai_model() {
        let mut data = HashMap::new();
        data.insert(
            "xai/grok-4.3".to_string(),
            json!({
                "input_cost_per_token": 1.25e-6,
                "output_cost_per_token": 2.5e-6,
                "cache_read_input_token_cost": 0.2e-6,
            }),
        );

        let result = parse_litellm_data(data);
        assert!(result.contains_key("xai/grok-4.3"));
        assert!(result.contains_key("grok-4.3"));
        let pricing = &result["grok-4.3"];
        assert_eq!(pricing.input, 1.25e-6);
        assert_eq!(pricing.output, 2.5e-6);
        assert_eq!(pricing.cache_read, 0.2e-6);
    }

    #[test]
    fn test_parse_xai_grok_build_alias() {
        let mut data = HashMap::new();
        data.insert(
            "xai/grok-build-0.1".to_string(),
            make_litellm_entry(1e-6, 2e-6),
        );

        let result = parse_litellm_data(data);
        assert!(result.contains_key("grok-build-0.1"));
        assert!(result.contains_key("grok-build"));
        assert_eq!(result["grok-build"].input, 1e-6);
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
        // input present, output/cache missing -> 0. Fully price-less entries are skipped.
        data.insert(
            "claude-test".to_string(),
            json!({"input_cost_per_token": 15e-6}),
        );

        let result = parse_litellm_data(data);
        let pricing = &result["claude-test"];
        assert_eq!(pricing.input, 15e-6);
        assert_eq!(pricing.output, 0.0);
        assert_eq!(pricing.cache_read, 0.0);
        assert_eq!(pricing.cache_create, 0.0);
        assert_eq!(pricing.reasoning_output, 0.0);
    }

    #[test]
    fn test_parse_cn_vendors_normalized_and_kept() {
        let mut data = HashMap::new();
        data.insert(
            "zai/glm-5".to_string(),
            json!({"input_cost_per_token": 1e-6, "output_cost_per_token": 3.2e-6, "cache_read_input_token_cost": 0.2e-6}),
        );
        data.insert(
            "fireworks_ai/glm-5".to_string(),
            json!({"input_cost_per_token": 99e-6, "output_cost_per_token": 99e-6}),
        );
        data.insert(
            "deepseek/deepseek-chat".to_string(),
            json!({"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}),
        );
        data.insert(
            "dashscope/qwen-max".to_string(),
            json!({"input_cost_per_token": 2e-6, "output_cost_per_token": 6e-6}),
        );
        data.insert(
            "moonshot/moonshot-v1".to_string(),
            json!({"input_cost_per_token": 1e-6, "output_cost_per_token": 3e-6}),
        );
        data.insert("zai/glm-meta".to_string(), json!({"mode": "chat"})); // priceless -> skip

        let result = parse_litellm_data(data);
        // Not filtered; bare names normalized; official zai/ wins over hoster
        assert_eq!(result["glm-5"].input, 1e-6);
        assert_eq!(result["deepseek-chat"].input, 1e-6);
        assert_eq!(result["qwen-max"].input, 2e-6);
        assert_eq!(result["moonshot-v1"].input, 1e-6);
        assert!(result.iter().all(|(k, _)| !k.contains("glm-meta"))); // not loaded as $0
    }
}
