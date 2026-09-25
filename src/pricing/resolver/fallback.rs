use super::super::types::{LongContextPricing, ModelPricing};

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

// Standard API rates: https://developers.openai.com/api/docs/pricing
// Before GPT-5.6, cache writes carry the uncached input rate.
fn openai_pricing(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        above_272k: None,
        input,
        output,
        reasoning_output: output,
        cache_create: input,
        cache_create_1h: input,
        cache_read,
    }
}

fn openai_pricing_with_cache_write(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    let mut pricing = openai_pricing(input, output, cache_read);
    pricing.cache_create = input * 1.25;
    pricing.cache_create_1h = pricing.cache_create;
    pricing
}

fn with_long_context(mut pricing: ModelPricing) -> ModelPricing {
    pricing.above_272k = Some(LongContextPricing {
        input: pricing.input * 2.0,
        output: pricing.output * 1.5,
        cache_read: pricing.cache_read * 2.0,
        cache_create: pricing.cache_create * 2.0,
    });
    pricing
}

fn matches_openai_model(model: &str, name: &str) -> bool {
    let bare = model.rsplit('/').next().unwrap_or(model);
    let bare = bare.strip_prefix("openai.").unwrap_or(bare);
    let Some(suffix) = bare.strip_prefix(name) else {
        return false;
    };
    if suffix.is_empty() {
        return true;
    }
    let Some(snapshot) = suffix.strip_prefix('-') else {
        return false;
    };
    let bytes = snapshot.as_bytes();
    (matches!(bytes.len(), 4 | 8) && bytes.iter().all(u8::is_ascii_digit))
        || (bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()))
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

fn openai_fallback_pricing(model: &str) -> Option<ModelPricing> {
    Some(if matches_openai_model(model, "gpt-6-astra") {
        with_long_context(openai_pricing_with_cache_write(10e-6, 50e-6, 1e-6))
    } else if matches_openai_model(model, "gpt-6-sol") {
        with_long_context(openai_pricing_with_cache_write(2e-6, 10e-6, 0.2e-6))
    } else if matches_openai_model(model, "gpt-6-luna") {
        with_long_context(openai_pricing_with_cache_write(0.1e-6, 0.5e-6, 0.01e-6))
    } else if matches_openai_model(model, "gpt-5.6-cyber") {
        with_long_context(openai_pricing_with_cache_write(12.5e-6, 75e-6, 1.25e-6))
    } else if matches_openai_model(model, "gpt-5.6-terra") {
        with_long_context(openai_pricing_with_cache_write(2e-6, 12e-6, 0.2e-6))
    } else if matches_openai_model(model, "gpt-5.6-luna") {
        with_long_context(openai_pricing_with_cache_write(0.2e-6, 1.2e-6, 0.02e-6))
    } else if matches_openai_model(model, "gpt-5.6-sol") || matches_openai_model(model, "gpt-5.6") {
        // Promotional standard rates are published through at least 2026-11-21.
        with_long_context(openai_pricing_with_cache_write(4e-6, 20e-6, 0.4e-6))
    } else if matches_openai_model(model, "gpt-5.5-pro")
        || matches_openai_model(model, "gpt-5.4-pro")
    {
        with_long_context(openai_pricing(30e-6, 180e-6, 30e-6))
    } else if matches_openai_model(model, "gpt-5.2-pro") {
        openai_pricing(21e-6, 168e-6, 21e-6)
    } else if matches_openai_model(model, "gpt-5-pro") {
        openai_pricing(15e-6, 120e-6, 15e-6)
    } else if matches_openai_model(model, "gpt-5.5") {
        with_long_context(openai_pricing(5e-6, 30e-6, 0.5e-6))
    } else if matches_openai_model(model, "gpt-5.4-mini") {
        openai_pricing(0.75e-6, 4.5e-6, 0.075e-6)
    } else if matches_openai_model(model, "gpt-5.4-nano") {
        openai_pricing(0.2e-6, 1.25e-6, 0.02e-6)
    } else if matches_openai_model(model, "gpt-5.4") {
        with_long_context(openai_pricing(2.5e-6, 15e-6, 0.25e-6))
    } else if matches_openai_model(model, "gpt-5.3-chat-latest")
        || matches_openai_model(model, "gpt-5.2-chat-latest")
        || matches_openai_model(model, "gpt-5.2")
    {
        openai_pricing(1.75e-6, 14e-6, 0.175e-6)
    } else if matches_openai_model(model, "gpt-5-mini") {
        openai_pricing(0.25e-6, 2e-6, 0.025e-6)
    } else if matches_openai_model(model, "gpt-5-nano") {
        openai_pricing(0.05e-6, 0.4e-6, 0.005e-6)
    } else if model.contains("gpt-5.1-codex-mini") {
        openai_pricing(0.25e-6, 2e-6, 0.025e-6)
    } else if model.contains("gpt-5.2-codex") || model.contains("gpt-5.3-codex") {
        openai_pricing(1.75e-6, 14e-6, 0.175e-6)
    } else if model.contains("gpt-5-codex") || model.contains("gpt-5.1-codex") {
        openai_pricing(1.25e-6, 10e-6, 0.125e-6)
    } else if model.contains("codex-mini") {
        openai_pricing(1.5e-6, 6e-6, 0.375e-6)
    } else if model.contains("codex") || model.contains("gpt-5") {
        openai_pricing(1.25e-6, 10e-6, 0.125e-6)
    } else if matches_openai_model(model, "gpt-4.1-mini") {
        openai_pricing(0.4e-6, 1.6e-6, 0.1e-6)
    } else if matches_openai_model(model, "gpt-4.1-nano") {
        openai_pricing(0.1e-6, 0.4e-6, 0.025e-6)
    } else if matches_openai_model(model, "gpt-4.1") {
        openai_pricing(2e-6, 8e-6, 0.5e-6)
    } else if matches_openai_model(model, "gpt-4o-mini") {
        openai_pricing(0.15e-6, 0.6e-6, 0.075e-6)
    } else if matches_openai_model(model, "gpt-4o") {
        openai_pricing(2.5e-6, 10e-6, 1.25e-6)
    } else if matches_openai_model(model, "gpt-4-turbo") {
        openai_pricing(10e-6, 30e-6, 10e-6)
    } else if matches_openai_model(model, "gpt-4") {
        openai_pricing(30e-6, 60e-6, 30e-6)
    } else if model.contains("gpt-4") {
        openai_pricing(2.5e-6, 10e-6, 0.0)
    } else {
        return None;
    })
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
        } else if let Some(pricing) = openai_fallback_pricing(&model_lower) {
            pricing
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

    fn assert_openai_rates(model: &str, base: [f64; 4], long: Option<[f64; 4]>) {
        // USD per million tokens: input, output, cached input, cache write.
        let pricing = fallback_pricing(model).unwrap();
        let actual = [
            pricing.input,
            pricing.output,
            pricing.cache_read,
            pricing.cache_create,
        ];
        for (actual, expected) in actual.into_iter().zip(base) {
            assert!((actual * 1e6 - expected).abs() < 1e-9, "{model}");
        }
        assert_eq!(pricing.cache_create_1h, pricing.cache_create, "{model}");
        assert_eq!(pricing.reasoning_output, pricing.output, "{model}");

        match (pricing.above_272k, long) {
            (Some(actual), Some(expected)) => {
                for (actual, expected) in [
                    actual.input,
                    actual.output,
                    actual.cache_read,
                    actual.cache_create,
                ]
                .into_iter()
                .zip(expected)
                {
                    assert!(
                        (actual * 1e6 - expected).abs() < 1e-9,
                        "{model} long context"
                    );
                }
            }
            (None, None) => {}
            _ => panic!("unexpected long-context pricing for {model}"),
        }
    }

    #[test]
    fn all_gpt_6_models_have_standard_and_long_context_fallback_rates() {
        let cases = [
            (
                "gpt-6-astra",
                [10.0, 50.0, 1.0, 12.5],
                [20.0, 75.0, 2.0, 25.0],
            ),
            (
                "openai/gpt-6-sol",
                [2.0, 10.0, 0.2, 2.5],
                [4.0, 15.0, 0.4, 5.0],
            ),
            (
                "openai.gpt-6-luna",
                [0.1, 0.5, 0.01, 0.125],
                [0.2, 0.75, 0.02, 0.25],
            ),
        ];
        for (model, base, long) in cases {
            assert_openai_rates(model, base, Some(long));
        }
    }

    #[test]
    fn gpt_fallback_rates_match_published_standard_prices() {
        let cases = [
            (
                "gpt-5.6-cyber",
                [12.5, 75.0, 1.25, 15.625],
                Some([25.0, 112.5, 2.5, 31.25]),
            ),
            (
                "gpt-5.6-sol",
                [4.0, 20.0, 0.4, 5.0],
                Some([8.0, 30.0, 0.8, 10.0]),
            ),
            (
                "gpt-5.6",
                [4.0, 20.0, 0.4, 5.0],
                Some([8.0, 30.0, 0.8, 10.0]),
            ),
            (
                "gpt-5.6-terra",
                [2.0, 12.0, 0.2, 2.5],
                Some([4.0, 18.0, 0.4, 5.0]),
            ),
            (
                "gpt-5.6-luna",
                [0.2, 1.2, 0.02, 0.25],
                Some([0.4, 1.8, 0.04, 0.5]),
            ),
            (
                "gpt-5.5-pro",
                [30.0, 180.0, 30.0, 30.0],
                Some([60.0, 270.0, 60.0, 60.0]),
            ),
            (
                "gpt-5.4-pro",
                [30.0, 180.0, 30.0, 30.0],
                Some([60.0, 270.0, 60.0, 60.0]),
            ),
            ("gpt-5.2-pro", [21.0, 168.0, 21.0, 21.0], None),
            ("gpt-5-pro", [15.0, 120.0, 15.0, 15.0], None),
            (
                "gpt-5.5",
                [5.0, 30.0, 0.5, 5.0],
                Some([10.0, 45.0, 1.0, 10.0]),
            ),
            (
                "gpt-5.4",
                [2.5, 15.0, 0.25, 2.5],
                Some([5.0, 22.5, 0.5, 5.0]),
            ),
            ("gpt-5.4-mini", [0.75, 4.5, 0.075, 0.75], None),
            ("gpt-5.4-nano", [0.2, 1.25, 0.02, 0.2], None),
            ("gpt-5.2", [1.75, 14.0, 0.175, 1.75], None),
            ("gpt-5.3-chat-latest", [1.75, 14.0, 0.175, 1.75], None),
            ("gpt-5-mini", [0.25, 2.0, 0.025, 0.25], None),
            ("gpt-5-nano", [0.05, 0.4, 0.005, 0.05], None),
            ("gpt-4.1-mini", [0.4, 1.6, 0.1, 0.4], None),
            ("gpt-4.1-nano", [0.1, 0.4, 0.025, 0.1], None),
            ("gpt-4.1", [2.0, 8.0, 0.5, 2.0], None),
            ("gpt-4o-mini", [0.15, 0.6, 0.075, 0.15], None),
            ("gpt-4o", [2.5, 10.0, 1.25, 2.5], None),
            ("gpt-4-turbo", [10.0, 30.0, 10.0, 10.0], None),
            ("gpt-4", [30.0, 60.0, 30.0, 30.0], None),
        ];
        for (model, base, long) in cases {
            assert_openai_rates(model, base, long);
        }
    }

    #[test]
    fn openai_model_match_accepts_snapshots_without_cross_matching_variants() {
        assert!(matches_openai_model("gpt-5.5-2026-04-23", "gpt-5.5"));
        assert!(matches_openai_model(
            "openai/gpt-5.4-mini-20260317",
            "gpt-5.4-mini"
        ));
        assert!(!matches_openai_model("gpt-5.5-pro", "gpt-5.5"));
        assert!(!matches_openai_model(
            "gpt-4o-mini-transcribe",
            "gpt-4o-mini"
        ));
        assert!(!matches_openai_model("gpt-6-sol-preview", "gpt-6-sol"));
    }

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
        assert_eq!(p.input, 0.15e-6);
        assert_eq!(p.output, 0.6e-6);
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
