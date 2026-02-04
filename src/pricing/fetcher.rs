use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use crate::data::Stats;

/// Model pricing info (per token, not per million)
#[derive(Debug, Clone, Default)]
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_create: f64,
}

/// Pricing database loaded from LiteLLM or cache
#[derive(Debug, Default)]
pub struct PricingDb {
    pub models: HashMap<String, ModelPricing>,
}

impl PricingDb {
    pub fn get_cache_path() -> PathBuf {
        let home = dirs::home_dir().expect("Cannot find home directory");
        home.join(".cache").join("ccstats").join("pricing.json")
    }

    pub fn load_from_cache() -> Option<Self> {
        let path = Self::get_cache_path();
        let file = File::open(&path).ok()?;
        let data: HashMap<String, serde_json::Value> = serde_json::from_reader(file).ok()?;
        Some(Self::parse_litellm_data(data))
    }

    pub fn save_to_cache(&self, raw_data: &HashMap<String, serde_json::Value>) {
        let path = Self::get_cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = File::create(&path) {
            let _ = serde_json::to_writer(&mut file, raw_data);
        }
    }

    pub fn fetch_from_litellm() -> Option<(Self, HashMap<String, serde_json::Value>)> {
        let url = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
        let response = ureq::get(url).call().ok()?;
        let mut body = response.into_body();
        let data: HashMap<String, serde_json::Value> = serde_json::from_reader(body.as_reader()).ok()?;
        let db = Self::parse_litellm_data(data.clone());
        Some((db, data))
    }

    pub fn parse_litellm_data(data: HashMap<String, serde_json::Value>) -> Self {
        let mut models = HashMap::new();

        for (name, value) in data {
            if !name.contains("claude") {
                continue;
            }

            let pricing = ModelPricing {
                input: value
                    .get("input_cost_per_token")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                output: value
                    .get("output_cost_per_token")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                cache_read: value
                    .get("cache_read_input_token_cost")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                cache_create: value
                    .get("cache_creation_input_token_cost")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            };

            // Store with multiple key variations for matching
            models.insert(name.clone(), pricing.clone());

            // Also store normalized versions
            let normalized = name.replace("claude-", "").replace("anthropic.", "");
            models.insert(normalized, pricing);
        }

        Self { models }
    }

    pub fn load(offline: bool) -> Self {
        Self::load_internal(offline, false)
    }

    pub fn load_quiet(offline: bool) -> Self {
        Self::load_internal(offline, true)
    }

    fn load_internal(offline: bool, quiet: bool) -> Self {
        if offline {
            if let Some(db) = Self::load_from_cache() {
                if !quiet {
                    eprintln!("Using cached pricing data");
                }
                return db;
            }
            if !quiet {
                eprintln!("No cached pricing, using built-in defaults");
            }
            return Self::default();
        }

        if !quiet {
            eprintln!("Fetching pricing from LiteLLM...");
        }
        if let Some((db, raw_data)) = Self::fetch_from_litellm() {
            db.save_to_cache(&raw_data);
            if !quiet {
                eprintln!("Loaded pricing for {} Claude models", db.models.len());
            }
            return db;
        }

        if !quiet {
            eprintln!("Failed to fetch pricing, trying cache...");
        }
        if let Some(db) = Self::load_from_cache() {
            return db;
        }

        if !quiet {
            eprintln!("Using built-in defaults");
        }
        Self::default()
    }

    pub fn get_pricing(&self, model: &str) -> ModelPricing {
        // Try exact match first
        if let Some(p) = self.models.get(model) {
            return p.clone();
        }

        // Try with claude- prefix
        let with_prefix = format!("claude-{}", model);
        if let Some(p) = self.models.get(&with_prefix) {
            return p.clone();
        }

        // Try partial matching
        let model_lower = model.to_lowercase();
        for (name, pricing) in &self.models {
            if name.to_lowercase().contains(&model_lower)
                || model_lower.contains(&name.to_lowercase())
            {
                return pricing.clone();
            }
        }

        // Fallback to hardcoded defaults based on model family
        if model_lower.contains("opus-4-5") || model_lower.contains("opus-4.5") {
            ModelPricing {
                input: 5e-6,         // $5/M
                output: 25e-6,       // $25/M
                cache_create: 6.25e-6, // $6.25/M
                cache_read: 0.5e-6,  // $0.5/M
            }
        } else if model_lower.contains("opus") {
            ModelPricing {
                input: 15e-6,
                output: 75e-6,
                cache_create: 18.75e-6,
                cache_read: 1.5e-6,
            }
        } else if model_lower.contains("sonnet") {
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            }
        } else if model_lower.contains("haiku") {
            ModelPricing {
                input: 0.8e-6,
                output: 4e-6,
                cache_create: 1e-6,
                cache_read: 0.08e-6,
            }
        } else {
            // Default to sonnet pricing
            ModelPricing {
                input: 3e-6,
                output: 15e-6,
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            }
        }
    }
}

pub fn calculate_cost(stats: &Stats, model: &str, pricing_db: &PricingDb) -> f64 {
    let pricing = pricing_db.get_pricing(model);
    stats.input_tokens as f64 * pricing.input
        + stats.output_tokens as f64 * pricing.output
        + stats.cache_creation as f64 * pricing.cache_create
        + stats.cache_read as f64 * pricing.cache_read
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
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            },
        );

        let stats = Stats {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_creation: 0,
            cache_read: 0,
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
                cache_create: 3.75e-6,
                cache_read: 0.3e-6,
            },
        );

        let stats = Stats {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation: 1_000_000,
            cache_read: 1_000_000,
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
        assert!(pricing.input > 0.0);
        assert!(pricing.output > 0.0);
    }
}
