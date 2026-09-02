use super::*;
use std::collections::BTreeSet;

#[test]
fn usage_source_accepts_registry_names_and_aliases() {
    for source in crate::source::all_sources() {
        let expected = UsageSource::from_name(source.name()).expect("SDK source variant");
        assert_eq!(source.name().parse::<UsageSource>().unwrap(), expected);
        assert_eq!(
            source
                .name()
                .to_ascii_uppercase()
                .parse::<UsageSource>()
                .unwrap(),
            expected
        );
        assert_eq!(
            format!(" {} ", source.name())
                .parse::<UsageSource>()
                .unwrap(),
            expected
        );

        for alias in source.aliases() {
            assert_eq!(alias.parse::<UsageSource>().unwrap(), expected);
        }
    }

    let err = " unknown ".parse::<UsageSource>().unwrap_err();
    assert!(matches!(err, SdkError::InvalidSource { name } if name == "unknown"));
}

#[test]
fn extension_usage_sources_use_canonical_serde_names() {
    assert_eq!(
        serde_json::to_string(&UsageSource::RooCode).unwrap(),
        r#""roocode""#
    );
    assert_eq!(
        serde_json::to_string(&UsageSource::KiloCode).unwrap(),
        r#""kilocode""#
    );
    assert_eq!(
        serde_json::from_str::<UsageSource>(r#""roocode""#).unwrap(),
        UsageSource::RooCode
    );
    assert_eq!(
        serde_json::from_str::<UsageSource>(r#""kilocode""#).unwrap(),
        UsageSource::KiloCode
    );
}

#[test]
fn registry_concrete_sources_match_sdk_usage_sources() {
    let registry_sources = crate::source::all_sources()
        .map(Source::name)
        .collect::<BTreeSet<_>>();
    let sdk_sources = UsageSource::VARIANTS
        .iter()
        .map(|source| source.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(registry_sources, sdk_sources);
    assert!(crate::source::source_choices().contains(&crate::source::ALL_SOURCES));
    assert!(crate::source::ALL_SOURCES.parse::<UsageSource>().is_err());

    for source in crate::source::all_sources() {
        assert_eq!(
            source.name().parse::<UsageSource>().unwrap().as_str(),
            source.name()
        );
        for alias in source.aliases() {
            assert_eq!(
                alias.parse::<UsageSource>().unwrap().as_str(),
                source.name()
            );
        }
    }
}

#[test]
fn usage_range_this_week_starts_on_monday() {
    let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
    let (since, until) = UsageRange::ThisWeek.resolve(today).unwrap();
    assert_eq!(since, Some(NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()));
    assert_eq!(until, Some(today));
}

#[test]
fn usage_range_rejects_reversed_dates() {
    let range = UsageRange::DateRange {
        since: Some(NaiveDate::from_ymd_opt(2026, 5, 10).unwrap()),
        until: Some(NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()),
    };
    assert!(
        range
            .resolve(NaiveDate::from_ymd_opt(2026, 5, 9).unwrap())
            .is_err()
    );
}

#[test]
fn usage_range_accepts_exact_utc_timestamps_and_rejects_reversed_bounds() {
    let payload = serde_json::json!({
        "timestamp_range": {
            "since": "2026-08-21T05:41:00Z",
            "until": "2026-08-21T05:43:00Z"
        }
    });
    let range: UsageRange = serde_json::from_value(payload.clone()).expect("timestamp range");

    assert_eq!(
        serde_json::to_value(&range).expect("serialize range"),
        payload
    );

    let reversed: UsageRange = serde_json::from_value(serde_json::json!({
        "timestamp_range": {
            "since": "2026-08-21T05:43:00Z",
            "until": "2026-08-21T05:41:00Z"
        }
    }))
    .expect("deserialize reversed range");
    assert!(
        reversed
            .resolve(NaiveDate::from_ymd_opt(2026, 8, 21).expect("valid date"))
            .is_err()
    );
}

#[test]
fn token_breakdown_deserializes_legacy_json_without_cache_hit_rate() {
    let legacy = r#"{
        "input_tokens": 10,
        "output_tokens": 5,
        "reasoning_tokens": 0,
        "cache_creation_tokens": 3,
        "cache_read_tokens": 2,
        "reported_total_adjustment": 0,
        "total_tokens": 20
    }"#;
    let tokens: TokenBreakdown =
        serde_json::from_str(legacy).expect("legacy breakdown without cache_hit_rate");
    assert_eq!(tokens.cache_hit_rate, None);
    assert_eq!(tokens.total_tokens, 20);
}

#[test]
fn token_breakdown_treats_cache_creation_1h_as_subset() {
    let stats = Stats {
        input_tokens: 10,
        output_tokens: 5,
        cache_creation: 30,
        cache_creation_1h: 25,
        cache_read: 2,
        ..Stats::default()
    };
    let tokens = TokenBreakdown::from_stats(&stats, true);
    let serialized = serde_json::to_value(&tokens).expect("serialize breakdown");

    assert_eq!(tokens.cache_creation_tokens, 30);
    assert_eq!(tokens.cache_creation_1h_tokens, 25);
    assert_eq!(serialized["cache_creation_tokens"], 30);
    assert_eq!(serialized["cache_creation_1h_tokens"], 25);
    assert_eq!(tokens.total_tokens, 47);
    assert_eq!(serialized["total_tokens"], 47);
}

#[test]
fn model_summaries_use_model_name_as_equal_cost_tiebreaker() {
    let pricing_db = PricingDb::default();
    let mut models = HashMap::new();
    models.insert(
        "gpt-5-zeta".to_string(),
        Stats {
            input_tokens: 10,
            ..Stats::default()
        },
    );
    models.insert(
        "gpt-5-alpha".to_string(),
        Stats {
            input_tokens: 10,
            ..Stats::default()
        },
    );

    let rows = summarize_models(&models, &pricing_db, None, true);

    assert_eq!(rows[0].model, "gpt-5-alpha");
    assert_eq!(rows[1].model, "gpt-5-zeta");
    assert_eq!(rows[0].cost_usd, rows[1].cost_usd);
}

#[test]
fn model_summary_exposes_recorded_cost_provenance() {
    let pricing_db = PricingDb::default();
    let models = HashMap::from([(
        "source-priced-model".to_string(),
        Stats {
            input_tokens: 10,
            count: 1,
            recorded_cost_usd: 0.25,
            recorded_cost_entries: 1,
            ..Stats::default()
        },
    )]);

    let rows = summarize_models(&models, &pricing_db, None, false);

    assert_eq!(rows[0].cost_usd, Some(0.25));
    assert_eq!(rows[0].pricing_source, "recorded");
}

#[test]
fn cli_config_fills_sdk_summary_defaults() {
    let config = Config {
        offline: true,
        strict_pricing: true,
        timezone: Some("Asia/Shanghai".to_string()),
        currency: Some("CNY".to_string()),
        ..Config::default()
    };

    let options = apply_cli_config(
        SummaryOptions {
            source: UsageSource::Codex,
            range: UsageRange::Today,
            ..SummaryOptions::default()
        },
        &config,
    );

    assert_eq!(options.source, UsageSource::Codex);
    assert_eq!(options.range, UsageRange::Today);
    assert!(options.offline);
    assert!(options.strict_pricing);
    assert_eq!(options.timezone.as_deref(), Some("Asia/Shanghai"));
    assert_eq!(options.currency.as_deref(), Some("CNY"));
}

#[test]
fn explicit_sdk_summary_options_win_over_cli_config() {
    let config = Config {
        timezone: Some("Asia/Shanghai".to_string()),
        currency: Some("CNY".to_string()),
        ..Config::default()
    };

    let options = apply_cli_config(
        SummaryOptions {
            timezone: Some("UTC".to_string()),
            currency: Some("EUR".to_string()),
            ..SummaryOptions::default()
        },
        &config,
    );

    assert_eq!(options.timezone.as_deref(), Some("UTC"));
    assert_eq!(options.currency.as_deref(), Some("EUR"));
}

#[test]
fn requested_currency_requires_available_rate() {
    let err = load_requested_currency(Some("ZZZ"), true).expect_err("currency should fail");
    assert!(err.to_string().contains("failed to load exchange rate"));
}
