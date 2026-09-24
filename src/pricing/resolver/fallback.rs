use super::super::types::ModelPricing;

// Standard Claude API rates: https://platform.claude.com/docs/en/about-claude/pricing
fn claude_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: input * 1.25,
        cache_create_1h: input * 2.0,
        cache_read,
    }
}

fn matches_claude_version(model: &str, version: &str) -> bool {
    model.match_indices(version).any(|(start, _)| {
        let suffix = &model[start + version.len()..];
        let numbered_suffix = suffix.strip_prefix('-').unwrap_or(suffix);
        let digits = numbered_suffix
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        digits == 0 || (suffix.starts_with('-') && digits == 8)
    })
}

fn openai_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: input * 1.25,
        cache_create_1h: input * 1.25,
        cache_read,
    }
}

fn xai_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: 0.0,
        cache_create_1h: 0.0,
        cache_read,
    }
}

fn moonshot_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: 0.0,
        cache_create_1h: 0.0,
        cache_read,
    }
}

fn google_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: 0.0,
        cache_create_1h: 0.0,
        cache_read,
    }
}

pub(crate) fn fallback_pricing(model: &str) -> Option<ModelPricing> {
    let model_lower = model.to_lowercase();
    let claude_model = model_lower.replace('.', "-");
    Some(
        if matches_claude_version(&claude_model, "fable-5-1")
            || matches_claude_version(&claude_model, "mythos-5-1")
        {
            claude_pricing(10e-6, 50e-6, 0.25e-6)
        } else if matches_claude_version(&claude_model, "fable-5")
            || matches_claude_version(&claude_model, "mythos-5")
        {
            claude_pricing(10e-6, 50e-6, 1e-6)
        } else if matches_claude_version(&claude_model, "opus-5-5") {
            claude_pricing(4e-6, 20e-6, 0.2e-6)
        } else if ["opus-4-5", "opus-4-6", "opus-4-7", "opus-4-8", "opus-5"]
            .iter()
            .any(|version| matches_claude_version(&claude_model, version))
        {
            claude_pricing(5e-6, 25e-6, 0.5e-6)
        } else if claude_model.contains("opus") {
            claude_pricing(15e-6, 75e-6, 1.5e-6)
        } else if matches_claude_version(&claude_model, "sonnet-5") {
            claude_pricing(2e-6, 10e-6, 0.2e-6)
        } else if claude_model.contains("sonnet") {
            claude_pricing(3e-6, 15e-6, 0.3e-6)
        } else if matches_claude_version(&claude_model, "haiku-4-5") {
            claude_pricing(1e-6, 5e-6, 0.1e-6)
        } else if claude_model.contains("haiku") {
            claude_pricing(0.8e-6, 4e-6, 0.08e-6)
        } else if model_lower.contains("grok-build") {
            xai_pricing(1e-6, 2e-6, 0.2e-6)
        } else if model_lower.contains("grok") {
            xai_pricing(1.25e-6, 2.5e-6, 0.2e-6)
        } else if model_lower.contains("kimi") {
            // Kimi Code subscription models (e.g. `kimi-code/k3`) have no public
            // per-token price; use Moonshot's official `kimi-k2.6` API rates as
            // the reference estimate.
            moonshot_pricing(0.95e-6, 4e-6, 0.16e-6)
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
        } else if model_lower.contains("gemini-2.5-flash-lite") {
            google_pricing(1e-7, 4e-7, 1e-8)
        } else if model_lower.contains("gemini-2.5-pro") {
            google_pricing(1.25e-6, 1e-5, 1.25e-7)
        } else if model_lower.contains("gemini-2.5-flash") {
            google_pricing(3e-7, 2.5e-6, 3e-8)
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
    fn current_claude_fallback_rates_match_published_standard_prices() {
        // USD per million tokens: input, output, 5m write, 1h write, read.
        let cases = [
            ("claude-fable-5-1", [10.0, 50.0, 12.5, 20.0, 0.25]),
            ("claude-mythos-5.1", [10.0, 50.0, 12.5, 20.0, 0.25]),
            ("claude-fable-5", [10.0, 50.0, 12.5, 20.0, 1.0]),
            ("claude-mythos-5", [10.0, 50.0, 12.5, 20.0, 1.0]),
            ("opus-5-5", [4.0, 20.0, 5.0, 8.0, 0.2]),
            ("claude-opus-5-5-v1:0", [4.0, 20.0, 5.0, 8.0, 0.2]),
            ("claude-opus-5", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4-8", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4.7", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4-6", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4-6-20260401", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4-5", [5.0, 25.0, 6.25, 10.0, 0.5]),
            ("claude-opus-4-1", [15.0, 75.0, 18.75, 30.0, 1.5]),
            ("sonnet-5", [2.0, 10.0, 2.5, 4.0, 0.2]),
            ("claude-sonnet-4-6", [3.0, 15.0, 3.75, 6.0, 0.3]),
            ("claude-haiku-4-5", [1.0, 5.0, 1.25, 2.0, 0.1]),
            ("haiku-3.5", [0.8, 4.0, 1.0, 1.6, 0.08]),
        ];

        for (model, expected) in cases {
            let pricing = fallback_pricing(model).unwrap();
            let actual = [
                pricing.input,
                pricing.output,
                pricing.cache_create,
                pricing.cache_create_1h,
                pricing.cache_read,
            ];
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual * 1e6 - expected).abs() < 1e-9, "{model}");
            }
            assert_eq!(pricing.reasoning_output, pricing.output, "{model}");
        }
    }

    #[test]
    fn current_claude_versions_do_not_match_future_version_numbers() {
        assert!(fallback_pricing("claude-fable-5-10").is_none());
        assert!(fallback_pricing("claude-mythos-5-10").is_none());
        assert_eq!(fallback_pricing("claude-opus-5-50").unwrap().input, 15e-6);
        assert_eq!(fallback_pricing("claude-sonnet-5-1").unwrap().input, 3e-6);
        assert_eq!(fallback_pricing("claude-haiku-4-50").unwrap().input, 0.8e-6);
    }

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
    fn test_fallback_kimi_code_model() {
        let p = fallback_pricing("kimi-code/k3").unwrap();
        assert_eq!(p.input, 0.95e-6);
        assert_eq!(p.output, 4e-6);
        assert_eq!(p.cache_read, 0.16e-6);
        assert_eq!(p.cache_create, 0.0);
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

    #[test]
    fn test_fallback_gemini_2_5_flash_lite() {
        let p = fallback_pricing("gemini-2.5-flash-lite").unwrap();
        assert_eq!(p.input, 1e-7);
        assert_eq!(p.output, 4e-7);
        assert_eq!(p.cache_read, 1e-8);
        assert_eq!(p.reasoning_output, 4e-7);
        assert_eq!(p.cache_create, 0.0);
        assert_eq!(p.cache_create_1h, 0.0);
    }

    #[test]
    fn test_fallback_gemini_2_5_flash() {
        let p = fallback_pricing("google/gemini-2.5-flash").unwrap();
        assert_eq!(p.input, 3e-7);
        assert_eq!(p.output, 2.5e-6);
        assert_eq!(p.cache_read, 3e-8);
        assert_eq!(p.reasoning_output, 2.5e-6);
    }

    #[test]
    fn test_fallback_gemini_2_5_pro() {
        let p = fallback_pricing("gemini-2.5-pro").unwrap();
        assert_eq!(p.input, 1.25e-6);
        assert_eq!(p.output, 1e-5);
        assert_eq!(p.cache_read, 1.25e-7);
        assert_eq!(p.reasoning_output, 1e-5);
    }

    #[test]
    fn test_fallback_gemini_flash_lite_not_flash() {
        let lite = fallback_pricing("gemini-2.5-flash-lite").unwrap();
        let flash = fallback_pricing("gemini-2.5-flash").unwrap();
        assert_ne!(lite.input, flash.input);
        assert_eq!(lite.input, 1e-7);
    }

    #[test]
    fn test_fallback_unknown_gemini_returns_none() {
        assert!(fallback_pricing("gemini-1.5-pro").is_none());
    }
}
