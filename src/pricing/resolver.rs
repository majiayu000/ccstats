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
}
