use super::super::types::ModelPricing;

fn openai_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        input,
        output,
        reasoning_output: output,
        cache_create: 0.0,
        cache_read,
    }
}

fn xai_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        input,
        output,
        reasoning_output: output,
        cache_create: 0.0,
        cache_read,
    }
}

pub(crate) fn fallback_pricing(model: &str) -> Option<ModelPricing> {
    let model_lower = model.to_lowercase();
    Some(
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
        } else if model_lower.contains("grok-build") {
            xai_pricing(1e-6, 2e-6, 0.2e-6)
        } else if model_lower.contains("grok") {
            xai_pricing(1.25e-6, 2.5e-6, 0.2e-6)
        } else if model_lower.contains("gpt-5.4-mini") {
            openai_pricing(0.75e-6, 4.5e-6, 0.075e-6)
        } else if model_lower.contains("gpt-5.4-nano") {
            openai_pricing(0.2e-6, 1.25e-6, 0.02e-6)
        } else if model_lower.contains("gpt-5.4") {
            openai_pricing(2.5e-6, 15e-6, 0.25e-6)
        } else if model_lower.contains("gpt-5.1-codex-mini") {
            openai_pricing(0.25e-6, 2e-6, 0.025e-6)
        } else if model_lower.contains("gpt-5.2-codex") || model_lower.contains("gpt-5.3-codex") {
            openai_pricing(1.75e-6, 14e-6, 0.175e-6)
        } else if model_lower.contains("gpt-5-codex") || model_lower.contains("gpt-5.1-codex") {
            openai_pricing(1.25e-6, 10e-6, 0.125e-6)
        } else if model_lower.contains("codex-mini") {
            openai_pricing(1.5e-6, 6e-6, 0.375e-6)
        } else if model_lower.contains("codex") || model_lower.contains("gpt-5") {
            openai_pricing(1.25e-6, 10e-6, 0.125e-6)
        } else if model_lower.contains("gpt-4") {
            openai_pricing(2.5e-6, 10e-6, 0.0)
        } else {
            // Unknown model: no fallback estimate. The caller surfaces N/A instead
            // of silently applying a sonnet-shaped guess.
            return None;
        },
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_opus_4_5() {
        let p = fallback_pricing("claude-opus-4-5-20250514").unwrap();
        assert_eq!(p.input, 5e-6);
        assert_eq!(p.output, 25e-6);
    }

    #[test]
    fn test_fallback_opus() {
        let p = fallback_pricing("claude-opus-4-20250514").unwrap();
        assert_eq!(p.input, 15e-6);
        assert_eq!(p.output, 75e-6);
    }

    #[test]
    fn test_fallback_sonnet() {
        let p = fallback_pricing("claude-sonnet-4-20250514").unwrap();
        assert_eq!(p.input, 3e-6);
        assert_eq!(p.output, 15e-6);
    }

    #[test]
    fn test_fallback_haiku() {
        let p = fallback_pricing("claude-haiku-3.5").unwrap();
        assert_eq!(p.input, 0.8e-6);
        assert_eq!(p.output, 4e-6);
    }

    #[test]
    fn test_fallback_grok_build() {
        let p = fallback_pricing("grok-build").unwrap();
        assert_eq!(p.input, 1e-6);
        assert_eq!(p.output, 2e-6);
        assert_eq!(p.cache_read, 0.2e-6);
    }

    #[test]
    fn test_fallback_grok_4_3() {
        let p = fallback_pricing("grok-4.3").unwrap();
        assert_eq!(p.input, 1.25e-6);
        assert_eq!(p.output, 2.5e-6);
        assert_eq!(p.cache_read, 0.2e-6);
    }

    #[test]
    fn test_fallback_gpt5() {
        let p = fallback_pricing("gpt-5-turbo").unwrap();
        assert_eq!(p.input, 1.25e-6);
        assert_eq!(p.output, 10e-6);
    }

    #[test]
    fn test_fallback_codex() {
        let p = fallback_pricing("codex-mini").unwrap();
        assert_eq!(p.input, 1.5e-6);
        assert_eq!(p.output, 6e-6);
        assert_eq!(p.cache_read, 0.375e-6);
    }

    #[test]
    fn test_fallback_gpt5_codex() {
        let p = fallback_pricing("gpt-5.1-codex").unwrap();
        assert_eq!(p.input, 1.25e-6);
        assert_eq!(p.output, 10e-6);
        assert_eq!(p.cache_read, 0.125e-6);
    }

    #[test]
    fn test_fallback_gpt5_codex_mini() {
        let p = fallback_pricing("gpt-5.1-codex-mini").unwrap();
        assert_eq!(p.input, 0.25e-6);
        assert_eq!(p.output, 2e-6);
        assert_eq!(p.cache_read, 0.025e-6);
    }

    #[test]
    fn test_fallback_gpt5_4_mini() {
        let p = fallback_pricing("gpt-5.4-mini").unwrap();
        assert_eq!(p.input, 0.75e-6);
        assert_eq!(p.output, 4.5e-6);
        assert_eq!(p.cache_read, 0.075e-6);
    }

    #[test]
    fn test_fallback_gpt5_2_codex() {
        let p = fallback_pricing("gpt-5.2-codex").unwrap();
        assert_eq!(p.input, 1.75e-6);
        assert_eq!(p.output, 14e-6);
        assert_eq!(p.cache_read, 0.175e-6);
    }

    #[test]
    fn test_fallback_unknown_returns_none() {
        assert!(fallback_pricing("totally-unknown-model").is_none());
    }

    #[test]
    fn test_fallback_opus_4_5_dot_variant() {
        let p = fallback_pricing("claude-opus-4.5").unwrap();
        assert_eq!(p.input, 5e-6);
        assert_eq!(p.output, 25e-6);
    }

    #[test]
    fn test_fallback_case_insensitive() {
        let p = fallback_pricing("Claude-OPUS-4-20250514").unwrap();
        assert_eq!(p.input, 15e-6);

        let p2 = fallback_pricing("CLAUDE-HAIKU-3.5").unwrap();
        assert_eq!(p2.input, 0.8e-6);
    }

    #[test]
    fn test_fallback_gpt4() {
        let p = fallback_pricing("gpt-4o-mini").unwrap();
        assert_eq!(p.input, 2.5e-6);
        assert_eq!(p.output, 10e-6);
    }
}
