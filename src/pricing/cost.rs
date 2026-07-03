use std::collections::HashMap;

use crate::core::{CostKind, CostTokens, Stats};

use super::db::PricingDb;

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
            tokens.input_tokens as f64 * pricing.input
                + tokens.output_tokens as f64 * pricing.output
                + tokens.reasoning_tokens as f64 * pricing.reasoning_output
                + tokens.cache_creation as f64 * pricing.cache_create
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
