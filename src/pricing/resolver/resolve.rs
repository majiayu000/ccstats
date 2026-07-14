use std::collections::{HashMap, HashSet};

use super::super::types::{ModelPricing, dot_version_variant};

pub(crate) fn resolve_pricing_known(
    model: &str,
    models: &HashMap<String, ModelPricing>,
) -> Option<ModelPricing> {
    let model_lower = model.to_lowercase();
    if model_lower.is_empty() {
        return None;
    }

    match resolve_direct_exact(model, &model_lower, models) {
        PricingResolution::Resolved(pricing) => return Some(pricing),
        PricingResolution::Ambiguous => return None,
        PricingResolution::NoMatch => {}
    }

    match resolve_normalized_exact(model, &model_lower, models) {
        PricingResolution::Resolved(pricing) => return Some(pricing),
        PricingResolution::Ambiguous => return None,
        PricingResolution::NoMatch => {}
    }

    match resolve_dated_base_exact(model, &model_lower, models) {
        PricingResolution::Resolved(pricing) => return Some(pricing),
        PricingResolution::Ambiguous => return None,
        PricingResolution::NoMatch => {}
    }

    resolve_dated_claude_variant(&model_lower, models)
}

enum PricingResolution {
    NoMatch,
    Resolved(ModelPricing),
    Ambiguous,
}

fn resolve_direct_exact(
    model: &str,
    model_lower: &str,
    models: &HashMap<String, ModelPricing>,
) -> PricingResolution {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for key in [model, model_lower] {
        if seen.insert(key)
            && let Some(pricing) = models.get(key)
        {
            candidates.push((key.to_lowercase(), pricing_signature(pricing), pricing));
        }
    }

    resolve_unique_pricing(candidates)
}

fn resolve_normalized_exact(
    model: &str,
    model_lower: &str,
    models: &HashMap<String, ModelPricing>,
) -> PricingResolution {
    let candidates = exact_candidate_keys(model, model_lower)
        .into_iter()
        .filter_map(|key| {
            models.get(&key).map(|pricing| {
                (
                    canonical_lookup_key(&key),
                    pricing_signature(pricing),
                    pricing,
                )
            })
        })
        .collect();

    resolve_unique_pricing(candidates)
}

fn resolve_dated_base_exact(
    model: &str,
    model_lower: &str,
    models: &HashMap<String, ModelPricing>,
) -> PricingResolution {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for key in dated_base_candidate_keys(model, model_lower) {
        if let Some(base) = strip_known_date_suffix(&key) {
            push_candidate_key(&mut keys, &mut seen, base);
        }
    }

    let candidates = keys
        .into_iter()
        .filter_map(|key| {
            models.get(&key).map(|pricing| {
                (
                    canonical_lookup_key(&key),
                    pricing_signature(pricing),
                    pricing,
                )
            })
        })
        .collect();

    resolve_unique_pricing(candidates)
}

fn dated_base_candidate_keys(model: &str, model_lower: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    push_candidate_key(&mut keys, &mut seen, model);
    push_candidate_key(&mut keys, &mut seen, model_lower);

    let base_keys = keys.clone();
    for key in base_keys {
        if let Some(stripped) = key.strip_prefix("anthropic.") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("anthropic/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("openai/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("xai/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
    }

    keys
}

fn exact_candidate_keys(model: &str, model_lower: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    push_candidate_key(&mut keys, &mut seen, model);
    push_candidate_key(&mut keys, &mut seen, model_lower);

    let base_keys = keys.clone();
    for key in base_keys {
        if let Some(stripped) = key.strip_prefix("anthropic.") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("anthropic/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("openai/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
        if let Some(stripped) = key.strip_prefix("xai/") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
    }

    let base_keys = keys.clone();
    for key in base_keys {
        if let Some(stripped) = key.strip_prefix("claude-") {
            push_candidate_key(&mut keys, &mut seen, stripped);
        }
    }

    let base_keys = keys.clone();
    for key in base_keys {
        if !key.as_bytes().contains(&b'/')
            && is_claude_family_alias(&key)
            && !key.starts_with("claude-")
        {
            push_candidate_key(&mut keys, &mut seen, format!("claude-{key}"));
        }
        if key.starts_with("gpt-") || key.starts_with("codex") {
            push_candidate_key(&mut keys, &mut seen, format!("openai/{key}"));
        }
        if key.starts_with("grok-") {
            push_candidate_key(&mut keys, &mut seen, format!("xai/{key}"));
        }
    }

    keys
}

fn push_candidate_key(keys: &mut Vec<String>, seen: &mut HashSet<String>, key: impl AsRef<str>) {
    let key = key.as_ref();
    if key.is_empty() {
        return;
    }
    if seen.insert(key.to_string()) {
        keys.push(key.to_string());
    }

    let dotted = dot_version_variant(key);
    if dotted != key && seen.insert(dotted.clone()) {
        keys.push(dotted);
    }
}

fn resolve_dated_claude_variant(
    model_lower: &str,
    models: &HashMap<String, ModelPricing>,
) -> Option<ModelPricing> {
    let bases = approved_dated_variant_bases(model_lower);
    if bases.is_empty() {
        return None;
    }

    let mut candidates = Vec::new();
    for (key, pricing) in models {
        let key_lower = key.to_lowercase();
        let canonical = canonical_claude_key(&key_lower);
        if strip_yyyymmdd_suffix(&canonical).is_some_and(|base| bases.contains(base)) {
            candidates.push((canonical, pricing_signature(pricing), pricing));
        }
    }

    unique_pricing(candidates)
}

fn approved_dated_variant_bases(model_lower: &str) -> HashSet<String> {
    exact_candidate_keys(model_lower, model_lower)
        .into_iter()
        .map(|key| canonical_claude_key(&key))
        .filter(|key| is_claude_family_alias(key))
        .filter(|key| strip_yyyymmdd_suffix(key).is_none())
        .collect()
}

fn canonical_claude_key(key: &str) -> String {
    let key = key.strip_prefix("anthropic.").unwrap_or(key);
    key.strip_prefix("claude-").unwrap_or(key).to_string()
}

fn canonical_lookup_key(key: &str) -> String {
    let lower = key.to_lowercase();
    let without_provider = lower
        .strip_prefix("anthropic.")
        .or_else(|| lower.strip_prefix("anthropic/"))
        .or_else(|| lower.strip_prefix("openai/"))
        .or_else(|| lower.strip_prefix("xai/"))
        .unwrap_or(&lower);

    canonical_claude_key(without_provider)
}

fn is_claude_family_alias(key: &str) -> bool {
    key.split(['-', '.', '/', '_'])
        .any(|segment| matches!(segment, "sonnet" | "opus" | "haiku"))
}

fn strip_yyyymmdd_suffix(key: &str) -> Option<&str> {
    let (base, suffix) = key.rsplit_once('-')?;
    (!base.is_empty()
        && suffix.len() == 8
        && suffix.chars().all(|character| character.is_ascii_digit()))
    .then_some(base)
}

fn strip_known_date_suffix(key: &str) -> Option<&str> {
    strip_yyyymmdd_suffix(key).or_else(|| strip_yyyy_mm_dd_suffix(key))
}

fn strip_yyyy_mm_dd_suffix(key: &str) -> Option<&str> {
    let date_start = key.len().checked_sub(10)?;
    let separator_index = date_start.checked_sub(1)?;
    if key.as_bytes().get(separator_index) != Some(&b'-') {
        return None;
    }

    let base = &key[..separator_index];
    let suffix = &key[date_start..];
    (!base.is_empty()
        && suffix.len() == 10
        && suffix.as_bytes().get(4) == Some(&b'-')
        && suffix.as_bytes().get(7) == Some(&b'-')
        && suffix
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()))
    .then_some(base)
}

fn unique_pricing(candidates: Vec<(String, [u64; 6], &ModelPricing)>) -> Option<ModelPricing> {
    match resolve_unique_pricing(candidates) {
        PricingResolution::Resolved(pricing) => Some(pricing),
        PricingResolution::NoMatch | PricingResolution::Ambiguous => None,
    }
}

fn resolve_unique_pricing(candidates: Vec<(String, [u64; 6], &ModelPricing)>) -> PricingResolution {
    let mut unique = HashMap::new();
    for (canonical, signature, pricing) in candidates {
        match unique.get(&canonical) {
            Some((existing_signature, _)) if *existing_signature != signature => {
                return PricingResolution::Ambiguous;
            }
            Some(_) => {}
            None => {
                unique.insert(canonical, (signature, pricing));
            }
        }
    }

    if unique.is_empty() {
        return PricingResolution::NoMatch;
    }
    if unique.len() != 1 {
        return PricingResolution::Ambiguous;
    }
    let (_, pricing) = unique
        .into_values()
        .next()
        .expect("unique pricing contains one value");
    PricingResolution::Resolved(pricing.clone())
}

fn pricing_signature(pricing: &ModelPricing) -> [u64; 6] {
    [
        pricing.input.to_bits(),
        pricing.output.to_bits(),
        pricing.reasoning_output.to_bits(),
        pricing.cache_read.to_bits(),
        pricing.cache_create.to_bits(),
        pricing.cache_create_1h.to_bits(),
    ]
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
    fn test_resolve_approved_dated_claude_variant() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("sonnet-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_anthropic_slash_provider_prefix_stripping() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("anthropic/claude-sonnet-4-20250514", &models);
        assert_eq!(result.map(|pricing| pricing.input), Some(3e-6));
    }

    #[test]
    fn test_resolve_dated_openai_base_variant() {
        let mut models = HashMap::new();
        models.insert(
            "gpt-4o-mini-audio-preview".to_string(),
            ModelPricing {
                input: 15e-8,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("gpt-4o-mini-audio-preview-2024-12-17", &models);
        assert_eq!(result.map(|pricing| pricing.input), Some(15e-8));
    }

    #[test]
    fn test_resolve_dated_claude_canonical_base_variant() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-5".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("claude-sonnet-4-5-20250929", &models);
        assert_eq!(result.map(|pricing| pricing.input), Some(3e-6));
    }

    #[test]
    fn test_resolve_no_match() {
        let models: HashMap<String, ModelPricing> = HashMap::new();
        let result = resolve_pricing_known("nonexistent-model", &models);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_case_insensitive_exact() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

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

        let result = resolve_pricing_known("sonnet-4-20250514", &models);
        assert!(result.is_some());
    }

    #[test]
    fn test_resolve_substring_model_name_in_query_returns_none() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("claude-sonnet-4-20250514", &models);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_ambiguous_dated_variants_return_none() {
        let mut models = HashMap::new();
        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input: 1e-6,
                ..Default::default()
            },
        );
        models.insert(
            "claude-sonnet-4-20260101".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("sonnet-4", &models);
        assert!(result.is_none());
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
    fn test_resolve_provider_prefix_stripping() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("anthropic.claude-sonnet-4", &models);
        assert!(result.is_some());
        assert_eq!(result.unwrap().input, 3e-6);
    }

    #[test]
    fn test_resolve_provider_normalized_ambiguity_returns_none() {
        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                ..Default::default()
            },
        );
        models.insert(
            "claude-sonnet-4".to_string(),
            ModelPricing {
                input: 4e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("anthropic.claude-sonnet-4", &models);
        assert!(result.is_none());
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
    fn test_resolve_short_substring_returns_none() {
        let mut models = HashMap::new();
        models.insert(
            "gpt-5.4-mini".to_string(),
            ModelPricing {
                input: 1e-6,
                ..Default::default()
            },
        );

        let result = resolve_pricing_known("mini", &models);
        assert!(result.is_none());
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
