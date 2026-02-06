use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::core::Stats;

use super::cache::{load_raw_cache, load_raw_cache_if_fresh, save_raw_cache};
use super::provider::fetch_litellm_raw;
use super::resolver::{fallback_pricing, parse_litellm_data, resolve_pricing_known};
use super::types::ModelPricing;

#[derive(Debug, Clone)]
enum ResolvedPricing {
    Known(ModelPricing),
    Unknown,
}

/// Pricing database loaded from LiteLLM or cache
#[derive(Debug)]
pub(crate) struct PricingDb {
    models: HashMap<String, ModelPricing>,
    resolved: RefCell<HashMap<String, ResolvedPricing>>,
    strict_unknown: bool,
}

const PRICING_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

impl PricingDb {
    fn from_raw_data(data: HashMap<String, serde_json::Value>, strict_unknown: bool) -> Self {
        Self {
            models: parse_litellm_data(data),
            resolved: RefCell::new(HashMap::new()),
            strict_unknown,
        }
    }

    fn load_from_cache(strict_unknown: bool) -> Option<Self> {
        let raw_data = load_raw_cache()?;
        Some(Self::from_raw_data(raw_data, strict_unknown))
    }

    fn load_from_cache_if_fresh(ttl: Duration, strict_unknown: bool) -> Option<(Self, Duration)> {
        let (raw_data, age) = load_raw_cache_if_fresh(ttl)?;
        Some((Self::from_raw_data(raw_data, strict_unknown), age))
    }

    pub(crate) fn load(offline: bool, strict_unknown: bool) -> Self {
        Self::load_internal(offline, strict_unknown, false)
    }

    pub(crate) fn load_quiet(offline: bool, strict_unknown: bool) -> Self {
        Self::load_internal(offline, strict_unknown, true)
    }

    fn load_internal(offline: bool, strict_unknown: bool, quiet: bool) -> Self {
        let start = Instant::now();

        if offline {
            if let Some(db) = Self::load_from_cache(strict_unknown) {
                if !quiet {
                    eprintln!(
                        "Using cached pricing ({:.2}ms)",
                        start.elapsed().as_secs_f64() * 1000.0
                    );
                }
                return db;
            }
            if !quiet {
                eprintln!(
                    "No cached pricing, using defaults ({:.2}ms)",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
            return Self {
                models: HashMap::new(),
                resolved: RefCell::new(HashMap::new()),
                strict_unknown,
            };
        }

        if let Some((db, age)) = Self::load_from_cache_if_fresh(PRICING_CACHE_TTL, strict_unknown) {
            if !quiet {
                eprintln!(
                    "Using cached pricing ({:.1}h old)",
                    age.as_secs_f64() / 3600.0
                );
            }
            return db;
        }

        if !quiet {
            eprint!("Fetching pricing from LiteLLM...");
        }
        if let Some(raw_data) = fetch_litellm_raw() {
            let fetch_time = start.elapsed();
            let db = Self::from_raw_data(raw_data.clone(), strict_unknown);
            save_raw_cache(&raw_data);
            if !quiet {
                eprintln!(
                    " {} models ({:.2}ms)",
                    db.models.len(),
                    fetch_time.as_secs_f64() * 1000.0
                );
            }
            return db;
        }

        if !quiet {
            eprintln!(" failed, trying cache...");
        }
        if let Some(db) = Self::load_from_cache(strict_unknown) {
            if !quiet {
                eprintln!(
                    "Using cached pricing ({:.2}ms)",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
            return db;
        }

        if !quiet {
            eprintln!(
                "Using defaults ({:.2}ms)",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        Self {
            models: HashMap::new(),
            resolved: RefCell::new(HashMap::new()),
            strict_unknown,
        }
    }

    fn get_pricing(&self, model: &str) -> Option<ModelPricing> {
        if let Some(cached) = self.resolved.borrow().get(model) {
            return match cached {
                ResolvedPricing::Known(pricing) => Some(pricing.clone()),
                ResolvedPricing::Unknown => None,
            };
        }

        let pricing = resolve_pricing_known(model, &self.models).or_else(|| {
            if self.strict_unknown {
                None
            } else {
                Some(fallback_pricing(model))
            }
        });

        let cached = match &pricing {
            Some(pricing) => ResolvedPricing::Known(pricing.clone()),
            None => ResolvedPricing::Unknown,
        };
        self.resolved.borrow_mut().insert(model.to_string(), cached);
        pricing
    }
}

impl Default for PricingDb {
    fn default() -> Self {
        Self {
            models: HashMap::new(),
            resolved: RefCell::new(HashMap::new()),
            strict_unknown: false,
        }
    }
}

pub(crate) fn calculate_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    match pricing_db.get_pricing(model) {
        Some(pricing) => {
            stats.input_tokens as f64 * pricing.input
                + stats.output_tokens as f64 * pricing.output
                + stats.reasoning_tokens as f64 * pricing.reasoning_output
                + stats.cache_creation as f64 * pricing.cache_create
                + stats.cache_read as f64 * pricing.cache_read
        }
        None => f64::NAN,
    }
}

/// Sum total cost across model breakdown map.
pub(crate) fn sum_model_costs(models: &HashMap<String, Stats>, pricing_db: &PricingDb) -> f64 {
    let mut total = 0.0;
    for (model, stats) in models {
        let cost = calculate_cost(stats, model, pricing_db);
        if cost.is_nan() {
            return f64::NAN;
        }
        total += cost;
    }
    total
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

    #[test]
    fn calculate_cost_basic() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                reasoning_output: 15e-6,
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            },
        );

        let stats = Stats {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_creation: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            count: 1,
            skipped_chunks: 0,
        };

        let cost = calculate_cost(&stats, "sonnet-4", &db);
        // 1M * $3/M + 100K * $15/M = $3 + $1.5 = $4.5
        assert!((cost - 4.5).abs() < 0.001);
    }

    #[test]
    fn calculate_cost_with_cache() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                reasoning_output: 15e-6,
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            },
        );

        let stats = Stats {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation: 1_000_000,
            cache_read: 1_000_000,
            reasoning_tokens: 0,
            count: 1,
            skipped_chunks: 0,
        };

        let cost = calculate_cost(&stats, "sonnet-4", &db);
        // 1M * $3.75/M + 1M * $0.3/M = $3.75 + $0.3 = $4.05
        assert!((cost - 4.05).abs() < 0.001);
    }

    #[test]
    fn calculate_cost_zero_tokens() {
        let db = PricingDb::default();
        let stats = Stats::default();
        let cost = calculate_cost(&stats, "unknown-model", &db);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn get_pricing_fallback_for_unknown_model() {
        let db = PricingDb::default();
        let pricing = db.get_pricing("sonnet-4");
        // Should fallback to sonnet pricing
        assert!(pricing.as_ref().is_some_and(|p| p.input > 0.0));
        assert!(pricing.as_ref().is_some_and(|p| p.output > 0.0));
    }

    #[test]
    fn strict_mode_marks_unknown_model_as_nan_cost() {
        let db = PricingDb {
            strict_unknown: true,
            ..PricingDb::default()
        };
        let stats = Stats {
            input_tokens: 10,
            ..Default::default()
        };
        let cost = calculate_cost(&stats, "totally-unknown-model", &db);
        assert!(cost.is_nan());
    }
}
