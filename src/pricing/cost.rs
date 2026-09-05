use std::collections::HashMap;

use crate::core::{CostKind, CostTokens, Stats};

use super::db::PricingDb;
use super::source::PricingSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostDisplayMode {
    Total,
    RealOnly,
}

fn calculate_token_cost(tokens: CostTokens, model: &str, pricing_db: &PricingDb) -> f64 {
    if !tokens.has_entries() {
        return 0.0;
    }
    match pricing_db.get_pricing(model) {
        Some(pricing) => {
            let long_ttl_tokens = tokens.cache_creation_1h.min(tokens.cache_creation);
            let short_ttl_tokens = tokens.cache_creation - long_ttl_tokens;
            let base_cost = tokens.input_tokens as f64 * pricing.input
                + tokens.output_tokens as f64 * pricing.output
                + tokens.reasoning_tokens as f64 * pricing.reasoning_output
                + short_ttl_tokens as f64 * pricing.cache_create
                + long_ttl_tokens as f64 * pricing.cache_create_1h
                + tokens.cache_read as f64 * pricing.cache_read;
            // Only the token subset classified before aggregation receives the
            // long-context rates. Recorded provider costs still bypass pricing.
            let long = tokens.above_272k;
            let adjustment = pricing.above_272k.as_ref().map_or(0.0, |rate| {
                long.input_tokens as f64 * (rate.input - pricing.input)
                    + long.output_tokens as f64 * (rate.output - pricing.output)
                    + long.reasoning_tokens as f64 * (rate.output - pricing.reasoning_output)
                    + long.cache_read as f64 * (rate.cache_read - pricing.cache_read)
                    + (long.cache_creation - long.cache_creation_1h) as f64
                        * (rate.cache_create - pricing.cache_create)
                    + long.cache_creation_1h as f64 * (rate.cache_create - pricing.cache_create_1h)
            });
            base_cost + adjustment
        }
        None => f64::NAN,
    }
}

fn combine_recorded_and_priced(recorded_usd: f64, priced: f64, priced_tokens: CostTokens) -> f64 {
    if priced.is_nan() {
        if priced_tokens.has_entries() && recorded_usd == 0.0 {
            f64::NAN
        } else {
            recorded_usd
        }
    } else {
        recorded_usd + priced
    }
}

pub(crate) fn calculate_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    if stats.recorded_cost_entries > 0 {
        combine_recorded_and_priced(
            stats.recorded_cost_usd,
            calculate_token_cost(stats.priced_tokens, model, pricing_db),
            stats.priced_tokens,
        )
    } else {
        calculate_token_cost(stats.cost_tokens(), model, pricing_db)
    }
}

pub(crate) fn calculate_real_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    if stats.recorded_cost_entries > 0 {
        let priced_real = stats.priced_tokens.saturating_sub(&stats.estimated_proxy);
        combine_recorded_and_priced(
            stats.recorded_cost_usd,
            calculate_token_cost(priced_real, model, pricing_db),
            priced_real,
        )
    } else {
        calculate_token_cost(stats.real_cost_tokens(), model, pricing_db)
    }
}

pub(crate) fn calculate_estimated_proxy_cost(
    stats: &Stats,
    model: &str,
    pricing_db: &PricingDb,
) -> f64 {
    calculate_token_cost(stats.estimated_proxy, model, pricing_db)
}

pub(crate) fn calculate_display_cost(
    stats: &Stats,
    model: &str,
    pricing_db: &PricingDb,
    mode: CostDisplayMode,
) -> f64 {
    match mode {
        CostDisplayMode::Total => calculate_cost(stats, model, pricing_db),
        CostDisplayMode::RealOnly => calculate_real_cost(stats, model, pricing_db),
    }
}

/// Sum total cost across model breakdown map.
///
/// Unknown models are skipped, so a single unpriced model does not erase the
/// total of every other model in the same row. An empty map returns 0.0; a map
/// where every selected cost bucket is unknown returns NaN so callers surface
/// N/A instead of a misleading $0.00.
pub(crate) fn sum_model_costs(models: &HashMap<String, Stats>, pricing_db: &PricingDb) -> f64 {
    sum_model_costs_by(models, pricing_db, calculate_cost)
}

pub(crate) fn sum_real_model_costs(models: &HashMap<String, Stats>, pricing_db: &PricingDb) -> f64 {
    sum_model_costs_by(models, pricing_db, calculate_real_cost)
}

pub(crate) fn sum_estimated_proxy_model_costs(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
) -> f64 {
    sum_token_costs_by(models, pricing_db, |stats| stats.estimated_proxy)
}

pub(crate) fn sum_display_model_costs(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
    mode: CostDisplayMode,
) -> f64 {
    match mode {
        CostDisplayMode::Total => sum_model_costs(models, pricing_db),
        CostDisplayMode::RealOnly => sum_real_model_costs(models, pricing_db),
    }
}

fn stats_has_cost_input(stats: &Stats) -> bool {
    stats.cost_tokens().has_entries()
        || stats.recorded_cost_entries > 0
        || stats.recorded_cost_usd > 0.0
}

fn sum_model_costs_by(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
    cost_of: impl Fn(&Stats, &str, &PricingDb) -> f64,
) -> f64 {
    if models.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    let mut any_entries = false;
    let mut any_known = false;
    for (model, stats) in models {
        if !stats_has_cost_input(stats) {
            continue;
        }
        any_entries = true;
        let cost = cost_of(stats, model, pricing_db);
        if cost.is_nan() {
            continue;
        }
        total += cost;
        any_known = true;
    }
    if !any_entries {
        0.0
    } else if any_known {
        total
    } else {
        f64::NAN
    }
}

fn sum_token_costs_by(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
    tokens_of: impl Fn(&Stats) -> CostTokens,
) -> f64 {
    if models.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    let mut any_entries = false;
    let mut any_known = false;
    for (model, stats) in models {
        let tokens = tokens_of(stats);
        if !tokens.has_entries() {
            continue;
        }
        any_entries = true;
        let cost = calculate_token_cost(tokens, model, pricing_db);
        if cost.is_nan() {
            continue;
        }
        total += cost;
        any_known = true;
    }
    if !any_entries {
        0.0
    } else if any_known {
        total
    } else {
        f64::NAN
    }
}

pub(crate) fn model_cost_kind(models: &HashMap<String, Stats>) -> CostKind {
    let mut has_real = false;
    let mut has_estimated = false;
    for stats in models.values() {
        match stats.cost_kind() {
            CostKind::Real => has_real = true,
            CostKind::EstimatedProxy => has_estimated = true,
            CostKind::Mixed => {
                has_real = true;
                has_estimated = true;
            }
        }
    }
    match (has_real, has_estimated) {
        (true, true) => CostKind::Mixed,
        (false, true) => CostKind::EstimatedProxy,
        _ => CostKind::Real,
    }
}

pub(crate) fn pricing_source_for_models(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
) -> PricingSource {
    pricing_source_for_models_with_cost(models, pricing_db).unwrap_or_else(|| pricing_db.source())
}

fn pricing_source_for_models_with_cost(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
) -> Option<PricingSource> {
    let mut source: Option<PricingSource> = None;
    for (model, stats) in models {
        if stats.recorded_cost_entries > 0 {
            source = Some(match source {
                Some(current) => current.combine(PricingSource::Recorded),
                None => PricingSource::Recorded,
            });
        }
        let has_locally_priced_tokens = stats.priced_tokens.has_entries()
            || (stats.recorded_cost_entries == 0 && stats.cost_tokens().has_entries());
        if !has_locally_priced_tokens {
            continue;
        }
        let model_source = pricing_db
            .pricing_source_for_model(model)
            .unwrap_or(PricingSource::Unknown);
        source = Some(match source {
            Some(current) => current.combine(model_source),
            None => model_source,
        });
    }
    source
}

pub(crate) fn pricing_source_for_model_stats(
    model: &str,
    stats: &Stats,
    pricing_db: &PricingDb,
) -> PricingSource {
    let mut models = HashMap::with_capacity(1);
    models.insert(model.to_string(), stats.clone());
    pricing_source_for_models(&models, pricing_db)
}

pub(crate) fn pricing_source_for_model_maps<'a>(
    maps: impl IntoIterator<Item = &'a HashMap<String, Stats>>,
    pricing_db: &PricingDb,
) -> PricingSource {
    let mut source: Option<PricingSource> = None;
    for map in maps {
        if let Some(map_source) = pricing_source_for_models_with_cost(map, pricing_db) {
            source = Some(match source {
                Some(current) => current.combine(map_source),
                None => map_source,
            });
        }
    }
    source.unwrap_or_else(|| pricing_db.source())
}

/// Borrowed item with precomputed total cost.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CostedRef<'a, T> {
    pub(crate) item: &'a T,
    pub(crate) cost: f64,
}

/// Attach precomputed costs to a slice of items.
pub(crate) fn attach_costs<'a, T, F>(
    items: &'a [T],
    mut models_of: F,
    pricing_db: &PricingDb,
) -> Vec<CostedRef<'a, T>>
where
    F: FnMut(&T) -> &HashMap<String, Stats>,
{
    items
        .iter()
        .map(|item| CostedRef {
            item,
            cost: sum_model_costs(models_of(item), pricing_db),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CostTokens;

    fn pricing_db_with(model: &str, pricing: super::super::types::ModelPricing) -> PricingDb {
        let mut db = PricingDb::default();
        db.insert_model_for_tests(model.to_string(), pricing);
        db
    }

    fn astra_pricing_db() -> PricingDb {
        let data = HashMap::from([(
            "gpt-6-astra".to_string(),
            serde_json::json!({
                "input_cost_per_token": 0.00001,
                "output_cost_per_token": 0.00005,
                "cache_read_input_token_cost": 1e-6,
                "cache_creation_input_token_cost": 1.25e-5,
                "input_cost_per_token_above_272k_tokens": 0.00002,
                "output_cost_per_token_above_272k_tokens": 7.5e-5,
                "cache_read_input_token_cost_above_272k_tokens": 2e-6,
                "cache_creation_input_token_cost_above_272k_tokens": 2.5e-5,
            }),
        )]);
        let mut models = super::super::resolver::parse_litellm_data(data);
        pricing_db_with("gpt-6-astra", models.remove("gpt-6-astra").unwrap())
    }

    fn context_entry(input: i64) -> crate::core::RawEntry {
        crate::core::RawEntry {
            timestamp: String::new(),
            timestamp_ms: 0,
            date_str: String::new(),
            message_id: None,
            session_key: String::new(),
            session_id: String::new(),
            project_path: String::new(),
            model: "gpt-6-astra".to_string(),
            input_tokens: input,
            cache_read: 190_000,
            cache_creation: 10_000,
            output_tokens: 600,
            reasoning_tokens: 400,
            call_count: 1,
            cache_creation_1h: 0,
            reported_total_tokens: None,
            stop_reason: None,
            cost_kind: CostKind::Real,
            endpoint: crate::core::Endpoint::Unknown,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        }
    }

    #[test]
    fn long_context_prices_each_request_before_aggregation() {
        let db = astra_pricing_db();
        // Exactly 272K stays standard; 272K+1 changes the entire request tier,
        // including cached input, cache writes and reasoning output.
        let short = context_entry(72_000).to_stats();
        let long = context_entry(72_001).to_stats();
        assert!((calculate_cost(&short, "gpt-6-astra", &db) - 1.085).abs() < 1e-9);
        assert!((calculate_cost(&long, "gpt-6-astra", &db) - 2.14502).abs() < 1e-9);
        let mut combined = short.clone();
        combined.add(&short);
        // Total input is already >272K, but neither request crosses the boundary.
        assert!((calculate_cost(&combined, "gpt-6-astra", &db) - 2.17).abs() < 1e-9);
        combined.add(&long);
        assert!((calculate_cost(&combined, "gpt-6-astra", &db) - 4.31502).abs() < 1e-9);
        assert_eq!(combined.total_tokens(), 819_001);
    }

    #[test]
    fn long_context_preserves_recorded_cost_and_proxy_filtering() {
        let db = astra_pricing_db();
        let mut recorded = context_entry(72_001);
        recorded.recorded_cost_usd = Some(7.0);
        let mut stats = recorded.to_stats();
        stats.add(&context_entry(72_001).to_stats());
        let mut proxy = context_entry(72_001);
        proxy.cost_kind = CostKind::EstimatedProxy;
        stats.add(&proxy.to_stats());
        assert!((calculate_cost(&stats, "gpt-6-astra", &db) - 11.29004).abs() < 1e-9);
        assert!((calculate_real_cost(&stats, "gpt-6-astra", &db) - 9.14502).abs() < 1e-9);
        stats.retain_real_token_totals();
        assert!((calculate_cost(&stats, "gpt-6-astra", &db) - 9.14502).abs() < 1e-9);
    }

    fn fable_pricing() -> super::super::types::ModelPricing {
        super::super::types::ModelPricing {
            above_272k: None,
            input: 1e-5,
            output: 5e-5,
            reasoning_output: 5e-5,
            cache_create: 1.25e-5,
            cache_create_1h: 2e-5,
            cache_read: 1e-6,
        }
    }

    #[test]
    fn calculate_cost_with_1h_cache_creation() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            cache_creation: 1_000_000,
            cache_creation_1h: 600_000,
            ..Default::default()
        };

        let cost = calculate_cost(&stats, "fable-5", &db);
        // 400K * $12.5/M + 600K * $20/M = $5 + $12 = $17
        assert!((cost - 17.0).abs() < 0.001);
    }

    #[test]
    fn calculate_cost_clamps_1h_to_total_cache_creation() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            cache_creation: 100_000,
            cache_creation_1h: 500_000, // malformed: larger than total
            ..Default::default()
        };

        let cost = calculate_cost(&stats, "fable-5", &db);
        // All 100K billed at the 1h rate: 100K * $20/M = $2
        assert!((cost - 2.0).abs() < 0.001);
    }

    #[test]
    fn cost_tokens_1h_flows_through_real_cost() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            cache_creation: 200_000,
            cache_creation_1h: 200_000,
            estimated_proxy: CostTokens::default(),
            ..Default::default()
        };

        // 200K * $20/M = $4 — all cache creation billed at the 1h rate
        let cost = calculate_real_cost(&stats, "fable-5", &db);
        assert!((cost - 4.0).abs() < 0.001);
    }

    #[test]
    fn recorded_cost_bypasses_local_price_list() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            input_tokens: 1_000_000,
            count: 5,
            recorded_cost_usd: 5.134_352_2,
            recorded_cost_entries: 1,
            ..Default::default()
        };

        assert!((calculate_cost(&stats, "fable-5", &db) - 5.134_352_2).abs() < 1e-9);
        assert!((calculate_real_cost(&stats, "fable-5", &db) - 5.134_352_2).abs() < 1e-9);
        assert!((calculate_estimated_proxy_cost(&stats, "fable-5", &db) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn recorded_zero_cost_is_not_replaced_by_priced_tokens() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            input_tokens: 1_000_000,
            count: 1,
            recorded_cost_usd: 0.0,
            recorded_cost_entries: 1,
            ..Default::default()
        };

        assert!((calculate_cost(&stats, "fable-5", &db) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn recorded_and_locally_priced_entries_report_mixed_provenance() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let stats = Stats {
            recorded_cost_entries: 1,
            recorded_cost_usd: 1.0,
            priced_tokens: CostTokens {
                input_tokens: 10,
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            pricing_source_for_model_stats("fable-5", &stats, &db),
            PricingSource::Mixed
        );
    }

    #[test]
    fn recorded_cost_preserves_unrelated_proxy_estimate_after_aggregation() {
        let db = pricing_db_with("fable-5", fable_pricing());
        let unrelated_proxy = CostTokens {
            input_tokens: 1_000_000,
            count: 1,
            ..Default::default()
        };
        let stats = Stats {
            input_tokens: 1_000_000,
            count: 1,
            recorded_cost_entries: 1,
            recorded_cost_usd: 2.5,
            priced_tokens: unrelated_proxy,
            estimated_proxy: unrelated_proxy,
            ..Default::default()
        };

        assert!((calculate_cost(&stats, "fable-5", &db) - 12.5).abs() < 1e-12);
    }
}
