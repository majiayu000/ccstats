use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crate::core::Stats;

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
    resolved: RefCell<HashMap<String, ModelPricing>>,
}

const PRICING_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

impl PricingDb {
    pub fn get_cache_path() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        Some(home.join(".cache").join("ccstats").join("pricing.json"))
    }

    pub fn load_from_cache() -> Option<Self> {
        let path = Self::get_cache_path()?;
        let file = File::open(&path).ok()?;
        let data: HashMap<String, serde_json::Value> = serde_json::from_reader(file).ok()?;
        Some(Self::parse_litellm_data(data))
    }

    fn load_from_cache_if_fresh(ttl: Duration) -> Option<(Self, Duration)> {
        let path = Self::get_cache_path()?;
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;
        if age > ttl {
            return None;
        }
        let file = File::open(&path).ok()?;
        let data: HashMap<String, serde_json::Value> = serde_json::from_reader(file).ok()?;
        Some((Self::parse_litellm_data(data), age))
    }

    pub fn save_to_cache(&self, raw_data: &HashMap<String, serde_json::Value>) {
        let Some(path) = Self::get_cache_path() else {
            return;
        };
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
            // Load Claude models and OpenAI GPT models
            let is_claude = name.contains("claude");
            let is_openai = name.starts_with("openai/") || name.starts_with("gpt-");

            if !is_claude && !is_openai {
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
            if is_claude {
                let normalized = name.replace("claude-", "").replace("anthropic.", "");
                models.insert(normalized, pricing);
            } else if is_openai {
                // Store without openai/ prefix
                if let Some(stripped) = name.strip_prefix("openai/") {
                    models.insert(stripped.to_string(), pricing);
                }
            }
        }

        Self {
            models,
            resolved: RefCell::new(HashMap::new()),
        }
    }

    pub fn load(offline: bool) -> Self {
        Self::load_internal(offline, false)
    }

    pub fn load_quiet(offline: bool) -> Self {
        Self::load_internal(offline, true)
    }

    fn load_internal(offline: bool, quiet: bool) -> Self {
        let start = Instant::now();

        if offline {
            if let Some(db) = Self::load_from_cache() {
                if !quiet {
                    eprintln!("Using cached pricing ({:.2}ms)", start.elapsed().as_secs_f64() * 1000.0);
                }
                return db;
            }
            if !quiet {
                eprintln!("No cached pricing, using defaults ({:.2}ms)", start.elapsed().as_secs_f64() * 1000.0);
            }
            return Self::default();
        }

        if let Some((db, age)) = Self::load_from_cache_if_fresh(PRICING_CACHE_TTL) {
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
        if let Some((db, raw_data)) = Self::fetch_from_litellm() {
            let fetch_time = start.elapsed();
            db.save_to_cache(&raw_data);
            if !quiet {
                eprintln!(" {} models ({:.2}ms)", db.models.len(), fetch_time.as_secs_f64() * 1000.0);
            }
            return db;
        }

        if !quiet {
            eprintln!(" failed, trying cache...");
        }
        if let Some(db) = Self::load_from_cache() {
            if !quiet {
                eprintln!("Using cached pricing ({:.2}ms)", start.elapsed().as_secs_f64() * 1000.0);
            }
            return db;
        }

        if !quiet {
            eprintln!("Using defaults ({:.2}ms)", start.elapsed().as_secs_f64() * 1000.0);
        }
        Self::default()
    }

    pub fn get_pricing(&self, model: &str) -> ModelPricing {
        if let Some(cached) = self.resolved.borrow().get(model) {
            return cached.clone();
        }

        // Try exact match first
        let pricing = if let Some(p) = self.models.get(model) {
            p.clone()
        } else {
            // Try with claude- prefix
            let with_prefix = format!("claude-{}", model);
            if let Some(p) = self.models.get(&with_prefix) {
                p.clone()
            } else {
                // Try partial matching
                let model_lower = model.to_lowercase();
                let mut candidates: Vec<(&String, &ModelPricing)> = self
                    .models
                    .iter()
                    .filter(|(name, _)| {
                        let name_lower = name.to_lowercase();
                        name_lower.contains(&model_lower) || model_lower.contains(&name_lower)
                    })
                    .collect();
                candidates.sort_by(|(a, _), (b, _)| {
                    b.len().cmp(&a.len()).then_with(|| a.cmp(b))
                });

                if let Some((_, p)) = candidates.first() {
                    (*p).clone()
                } else if model_lower.contains("opus-4-5") || model_lower.contains("opus-4.5") {
                    ModelPricing {
                        input: 5e-6,          // $5/M
                        output: 25e-6,        // $25/M
                        cache_create: 6.25e-6, // $6.25/M
                        cache_read: 0.5e-6,   // $0.5/M
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
                } else if model_lower.contains("gpt-5") || model_lower.contains("codex") {
                    // GPT-5 / Codex pricing (approximate)
                    ModelPricing {
                        input: 1.25e-6,      // $1.25/M
                        output: 10e-6,       // $10/M
                        cache_create: 0.0,
                        cache_read: 0.125e-6, // $0.125/M
                    }
                } else if model_lower.contains("gpt-4") {
                    ModelPricing {
                        input: 2.5e-6,
                        output: 10e-6,
                        cache_create: 0.0,
                        cache_read: 0.0,
                    }
                } else {
                    // Default to sonnet pricing for unknown models
                    ModelPricing {
                        input: 3e-6,
                        output: 15e-6,
                        cache_create: 3.75e-6,
                        cache_read: 0.3e-6,
                    }
                }
            }
        };

        self.resolved
            .borrow_mut()
            .insert(model.to_string(), pricing.clone());

        pricing
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
        assert!(pricing.input > 0.0);
        assert!(pricing.output > 0.0);
    }
}
