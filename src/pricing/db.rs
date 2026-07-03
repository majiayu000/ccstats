use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::core::Stats;

use super::cache::{
    CacheReadError, CacheWriteError, load_raw_cache, load_raw_cache_if_fresh, save_raw_cache,
};
use super::provider::fetch_litellm_raw;
use super::resolver::{fallback_pricing, parse_litellm_data, resolve_pricing_known};
use super::types::ModelPricing;

#[derive(Debug, Clone)]
enum ResolvedPricing {
    Known(ModelPricing),
    Unknown,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PricingLoadError {
    #[error("failed to load pricing cache: {0}")]
    Cache(#[from] CacheReadError),
}

/// Pricing database loaded from `LiteLLM` or cache
#[derive(Debug)]
pub(crate) struct PricingDb {
    models: HashMap<String, ModelPricing>,
    resolved: RefCell<HashMap<String, ResolvedPricing>>,
    strict_unknown: bool,
}

const PRICING_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

impl PricingDb {
    fn empty(strict_unknown: bool) -> Self {
        Self {
            models: HashMap::new(),
            resolved: RefCell::new(HashMap::new()),
            strict_unknown,
        }
    }

    fn from_raw_data(data: HashMap<String, serde_json::Value>, strict_unknown: bool) -> Self {
        Self {
            models: parse_litellm_data(data),
            resolved: RefCell::new(HashMap::new()),
            strict_unknown,
        }
    }

    fn load_from_cache(strict_unknown: bool) -> Result<Option<Self>, CacheReadError> {
        Ok(load_raw_cache()?.map(|raw_data| Self::from_raw_data(raw_data, strict_unknown)))
    }

    fn load_from_cache_if_fresh(
        ttl: Duration,
        strict_unknown: bool,
    ) -> Result<Option<(Self, Duration)>, CacheReadError> {
        Ok(load_raw_cache_if_fresh(ttl)?
            .map(|(raw_data, age)| (Self::from_raw_data(raw_data, strict_unknown), age)))
    }

    pub(crate) fn load(offline: bool, strict_unknown: bool) -> Self {
        Self::try_load(offline, strict_unknown).unwrap_or_else(|error| {
            eprintln!("Error: {error}");
            std::process::exit(1);
        })
    }

    pub(crate) fn load_quiet(offline: bool, strict_unknown: bool) -> Self {
        Self::try_load_quiet(offline, strict_unknown).unwrap_or_else(|error| {
            eprintln!("Error: {error}");
            std::process::exit(1);
        })
    }

    pub(crate) fn try_load(offline: bool, strict_unknown: bool) -> Result<Self, PricingLoadError> {
        Self::load_internal(offline, strict_unknown, false)
    }

    pub(crate) fn try_load_quiet(
        offline: bool,
        strict_unknown: bool,
    ) -> Result<Self, PricingLoadError> {
        Self::load_internal(offline, strict_unknown, true)
    }

    fn load_internal(
        offline: bool,
        strict_unknown: bool,
        quiet: bool,
    ) -> Result<Self, PricingLoadError> {
        let start = Instant::now();

        if offline {
            return Self::finish_offline_cache_load(
                Self::load_from_cache(strict_unknown),
                strict_unknown,
                quiet,
                start,
            );
        }

        match Self::load_from_cache_if_fresh(PRICING_CACHE_TTL, strict_unknown) {
            Ok(Some((db, age))) => {
                if !quiet {
                    eprintln!(
                        "Using cached pricing ({:.1}h old)",
                        age.as_secs_f64() / 3600.0
                    );
                }
                return Ok(db);
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("Warning: ignoring invalid pricing cache before refresh: {error}");
            }
        }

        if !quiet {
            eprint!("Fetching pricing from LiteLLM...");
        }
        if let Some(raw_data) = fetch_litellm_raw() {
            let fetch_time = start.elapsed();
            let save_result = save_raw_cache(&raw_data);
            let db = Self::from_raw_data(raw_data, strict_unknown);
            if !quiet {
                eprintln!(
                    " {} models ({:.2}ms)",
                    db.models.len(),
                    fetch_time.as_secs_f64() * 1000.0
                );
            }
            warn_cache_write_error(save_result);
            return Ok(db);
        }

        if !quiet {
            eprintln!(" failed, trying cache...");
        }
        match Self::load_from_cache(strict_unknown) {
            Ok(Some(db)) => {
                if !quiet {
                    eprintln!(
                        "Using cached pricing ({:.2}ms)",
                        start.elapsed().as_secs_f64() * 1000.0
                    );
                }
                return Ok(db);
            }
            Ok(None) => {}
            Err(error) => return Err(error.into()),
        }

        if !quiet {
            eprintln!(
                "Using defaults ({:.2}ms)",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        Ok(Self::empty(strict_unknown))
    }

    fn finish_offline_cache_load(
        cache_result: Result<Option<Self>, CacheReadError>,
        strict_unknown: bool,
        quiet: bool,
        start: Instant,
    ) -> Result<Self, PricingLoadError> {
        match cache_result {
            Ok(Some(db)) => {
                if !quiet {
                    eprintln!(
                        "Using cached pricing ({:.2}ms)",
                        start.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Ok(db)
            }
            Ok(None) => {
                if !quiet {
                    eprintln!(
                        "No cached pricing, using defaults ({:.2}ms)",
                        start.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Ok(Self::empty(strict_unknown))
            }
            Err(error) => Err(error.into()),
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
                fallback_pricing(model)
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

fn warn_cache_write_error(result: Result<(), CacheWriteError>) {
    if let Err(error) = result {
        eprintln!("Warning: failed to save pricing cache: {error}");
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
///
/// Unknown models (`calculate_cost` returns NaN because pricing did not
/// resolve) are skipped, so a single unpriced model does not erase the total
/// of every other model in the same row. An empty map returns 0.0; a map where
/// every model is unknown returns NaN so the caller surfaces N/A instead of a
/// misleading $0.00.
pub(crate) fn sum_model_costs(models: &HashMap<String, Stats>, pricing_db: &PricingDb) -> f64 {
    if models.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    let mut any_known = false;
    for (model, stats) in models {
        let cost = calculate_cost(stats, model, pricing_db);
        if cost.is_nan() {
            continue;
        }
        total += cost;
        any_known = true;
    }
    if any_known { total } else { f64::NAN }
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sample_raw_pricing(model: &str) -> HashMap<String, serde_json::Value> {
        HashMap::from([(
            model.to_string(),
            json!({
                "input_cost_per_token": 1e-6,
                "output_cost_per_token": 2e-6,
            }),
        )])
    }

    fn malformed_cache_error() -> CacheReadError {
        let source =
            serde_json::from_str::<HashMap<String, serde_json::Value>>("{not json").unwrap_err();
        CacheReadError::Malformed {
            path: PathBuf::from("pricing.json"),
            source,
        }
    }

    #[test]
    fn offline_missing_cache_keeps_default_pricing_behavior() {
        let db = PricingDb::finish_offline_cache_load(Ok(None), true, true, Instant::now())
            .expect("missing cache should use defaults");

        assert!(db.models.is_empty());
        assert!(db.strict_unknown);
    }

    #[test]
    fn offline_malformed_cache_fails_closed() {
        let error = PricingDb::finish_offline_cache_load(
            Err(malformed_cache_error()),
            false,
            true,
            Instant::now(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("malformed"));
    }

    #[test]
    fn cache_read_distinguishes_missing_from_malformed_for_db_load() {
        let root = TempDir::new().unwrap();
        let missing_path = root.path().join("missing-pricing.json");
        assert!(
            super::super::cache::load_raw_cache_from_paths(&[missing_path])
                .unwrap()
                .is_none()
        );

        let malformed_path = root.path().join("pricing.json");
        fs::write(&malformed_path, "{not json").unwrap();
        let error = super::super::cache::load_raw_cache_from_paths(&[malformed_path]).unwrap_err();

        assert!(matches!(error, CacheReadError::Malformed { .. }));
    }

    #[test]
    fn fetched_pricing_remains_usable_after_cache_save_failure() {
        let root = TempDir::new().unwrap();
        let blocker = root.path().join("not-a-directory");
        fs::write(&blocker, "file").unwrap();
        let cache_path = blocker.join("pricing.json");
        let raw_data = sample_raw_pricing("gpt-5");

        let save_result = super::super::cache::save_raw_cache_to_path(&raw_data, &cache_path);
        let db = PricingDb::from_raw_data(raw_data, false);

        assert!(save_result.is_err());
        assert!(db.get_pricing("gpt-5").is_some());
    }

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
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );
        let stats = Stats::default();
        let cost = calculate_cost(&stats, "sonnet-4", &db);
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

    #[test]
    fn calculate_cost_with_reasoning_tokens() {
        let mut db = PricingDb::default();
        db.models.insert(
            "opus-4".to_string(),
            ModelPricing {
                input: 15e-6,
                output: 75e-6,
                reasoning_output: 75e-6,
                cache_create: 0.0,
                cache_read: 0.0,
            },
        );

        let stats = Stats {
            input_tokens: 100_000,
            output_tokens: 0,
            reasoning_tokens: 50_000,
            ..Default::default()
        };

        let cost = calculate_cost(&stats, "opus-4", &db);
        // 100K * $15/M + 50K * $75/M = $1.5 + $3.75 = $5.25
        assert!((cost - 5.25).abs() < 0.001);
    }

    #[test]
    fn sum_model_costs_multiple_models() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );
        db.models.insert(
            "haiku-3.5".to_string(),
            ModelPricing {
                input: 0.8e-6,
                output: 4e-6,
                ..Default::default()
            },
        );

        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            Stats {
                input_tokens: 1_000_000,
                output_tokens: 100_000,
                ..Default::default()
            },
        );
        models.insert(
            "haiku-3.5".to_string(),
            Stats {
                input_tokens: 500_000,
                output_tokens: 50_000,
                ..Default::default()
            },
        );

        let total = sum_model_costs(&models, &db);
        // sonnet: 1M*3e-6 + 100K*15e-6 = 3.0 + 1.5 = 4.5
        // haiku: 500K*0.8e-6 + 50K*4e-6 = 0.4 + 0.2 = 0.6
        assert!((total - 5.1).abs() < 0.001);
    }

    #[test]
    fn sum_model_costs_empty_map() {
        let db = PricingDb::default();
        let models: HashMap<String, Stats> = HashMap::new();
        let total = sum_model_costs(&models, &db);
        assert_eq!(total, 0.0);
    }

    #[test]
    fn sum_model_costs_returns_nan_when_all_unknown() {
        let db = PricingDb {
            strict_unknown: true,
            ..PricingDb::default()
        };

        let mut models = HashMap::new();
        models.insert(
            "totally-unknown-xyz".to_string(),
            Stats {
                input_tokens: 100,
                ..Default::default()
            },
        );

        let total = sum_model_costs(&models, &db);
        assert!(total.is_nan());
    }

    #[test]
    fn sum_model_costs_skips_unknown_keeps_known() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        let mut models = HashMap::new();
        models.insert(
            "sonnet-4".to_string(),
            Stats {
                input_tokens: 1_000_000,
                output_tokens: 0,
                ..Default::default()
            },
        );
        models.insert(
            "totally-unknown-xyz".to_string(),
            Stats {
                input_tokens: 999_999_999, // would dominate the total if mispriced
                ..Default::default()
            },
        );

        // Unknown model is skipped; total reflects only sonnet-4 ($3.0),
        // not NaN and not a Sonnet-fallback guess on the unknown volume.
        let total = sum_model_costs(&models, &db);
        assert!((total - 3.0).abs() < 0.001);
    }

    #[test]
    fn attach_costs_computes_per_item() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        // Simple wrapper: Vec of (label, model_map)
        let items: Vec<(String, HashMap<String, Stats>)> = vec![
            (
                "day1".to_string(),
                HashMap::from([(
                    "sonnet-4".to_string(),
                    Stats {
                        input_tokens: 1_000_000,
                        output_tokens: 0,
                        ..Default::default()
                    },
                )]),
            ),
            (
                "day2".to_string(),
                HashMap::from([(
                    "sonnet-4".to_string(),
                    Stats {
                        input_tokens: 0,
                        output_tokens: 100_000,
                        ..Default::default()
                    },
                )]),
            ),
        ];

        let costed = attach_costs(&items, |item| &item.1, &db);
        assert_eq!(costed.len(), 2);
        // day1: 1M * 3e-6 = $3.0
        assert!((costed[0].cost - 3.0).abs() < 0.001);
        // day2: 100K * 15e-6 = $1.5
        assert!((costed[1].cost - 1.5).abs() < 0.001);
        assert_eq!(costed[0].item.0, "day1");
        assert_eq!(costed[1].item.0, "day2");
    }

    #[test]
    fn attach_costs_empty_slice() {
        let db = PricingDb::default();
        let items: Vec<(String, HashMap<String, Stats>)> = vec![];
        let costed = attach_costs(&items, |item| &item.1, &db);
        assert!(costed.is_empty());
    }

    #[test]
    fn get_pricing_caches_resolved_result() {
        let mut db = PricingDb::default();
        db.models.insert(
            "sonnet-4".to_string(),
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );

        // First call resolves and caches
        let p1 = db.get_pricing("sonnet-4");
        assert!(p1.is_some());
        assert!(db.resolved.borrow().contains_key("sonnet-4"));

        // Second call hits cache
        let p2 = db.get_pricing("sonnet-4");
        assert_eq!(p1.unwrap().input, p2.unwrap().input);
    }

    #[test]
    fn get_pricing_caches_unknown_in_strict_mode() {
        let db = PricingDb {
            strict_unknown: true,
            ..PricingDb::default()
        };

        let p = db.get_pricing("nonexistent-xyz-model");
        assert!(p.is_none());

        // Verify it cached as Unknown
        let resolved = db.resolved.borrow();
        assert!(resolved.contains_key("nonexistent-xyz-model"));
        assert!(matches!(
            resolved.get("nonexistent-xyz-model"),
            Some(ResolvedPricing::Unknown)
        ));
    }

    #[test]
    fn default_pricing_db_has_empty_models() {
        let db = PricingDb::default();
        assert!(db.models.is_empty());
        assert!(db.resolved.borrow().is_empty());
        assert!(!db.strict_unknown);
    }
}
