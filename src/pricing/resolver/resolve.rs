use std::collections::HashMap;

use super::super::types::ModelPricing;

pub(crate) fn resolve_pricing_known(
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
    if model_lower.is_empty() {
        return None;
    }
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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use serde_json::json;

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

        // Empty string should return None (rejected early)
        let result = resolve_pricing_known("", &models);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_glm_dot_alias_prefers_p_version() {
        // `glm-5.2` matches Fireworks `glm-5p2` (p == point) over older `glm-5`.
        use super::super::parse::parse_litellm_data;
        let mut data = HashMap::new();
        data.insert("fireworks_ai/glm-5p2".to_string(), json!({"input_cost_per_token": 1.4e-6, "output_cost_per_token": 4.4e-6, "cache_read_input_token_cost": 0.26e-6}));
        data.insert(
            "zai/glm-5".to_string(),
            json!({"input_cost_per_token": 1e-6, "output_cost_per_token": 3.2e-6}),
        );
        let pricing = resolve_pricing_known("glm-5.2", &parse_litellm_data(data)).unwrap();
        assert_eq!(pricing.input, 1.4e-6);
    }
}
