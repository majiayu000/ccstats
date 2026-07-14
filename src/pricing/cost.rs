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
            tokens.input_tokens as f64 * pricing.input
                + tokens.output_tokens as f64 * pricing.output
                + tokens.reasoning_tokens as f64 * pricing.reasoning_output
                + short_ttl_tokens as f64 * pricing.cache_create
                + long_ttl_tokens as f64 * pricing.cache_create_1h
                + tokens.cache_read as f64 * pricing.cache_read
        }
        None => f64::NAN,
    }
}

pub(crate) fn calculate_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    calculate_token_cost(stats.cost_tokens(), model, pricing_db)
}

pub(crate) fn calculate_real_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    calculate_token_cost(stats.real_cost_tokens(), model, pricing_db)
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
    sum_model_costs_by(models, pricing_db, Stats::cost_tokens)
}

pub(crate) fn sum_real_model_costs(models: &HashMap<String, Stats>, pricing_db: &PricingDb) -> f64 {
    sum_model_costs_by(models, pricing_db, Stats::real_cost_tokens)
}

pub(crate) fn sum_estimated_proxy_model_costs(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
) -> f64 {
    sum_model_costs_by(models, pricing_db, |stats| stats.estimated_proxy)
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

fn sum_model_costs_by(
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
        if !stats.cost_tokens().has_entries() {
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

    fn fable_pricing() -> super::super::types::ModelPricing {
        super::super::types::ModelPricing {
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
}
