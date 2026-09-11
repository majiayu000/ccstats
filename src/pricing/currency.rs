//! Currency conversion with exchange rate caching
//!
//! Fetches rates from open.er-api.com (free, no API key required).
//! Caches to the platform cache directory (`ccstats/exchange_rates.json`),
//! with `~/.cache/ccstats/exchange_rates.json` as a legacy read fallback.

use crate::utils::paths as dirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const EXCHANGE_RATE_URL: &str = "https://open.er-api.com/v6/latest/USD";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const EXCHANGE_CACHE_FILE: &str = "exchange_rates.json";

#[derive(Debug, Serialize, Deserialize)]
struct ExchangeRateResponse {
    rates: HashMap<String, f64>,
}

/// Currency converter with cached exchange rates
#[derive(Debug, Clone)]
pub(crate) struct CurrencyConverter {
    currency: String,
    rate: f64,
    symbol: String,
}

impl CurrencyConverter {
    /// Load converter for the given currency code (e.g., "CNY", "EUR")
    pub(crate) fn load(currency: &str, offline: bool) -> Option<Self> {
        let upper = currency.to_uppercase();
        if upper == "USD" {
            return Some(Self {
                currency: "USD".to_string(),
                rate: 1.0,
                symbol: "$".to_string(),
            });
        }

        let rates = load_rates(offline)?;
        let rate = rates.get(&upper)?;
        let symbol = currency_symbol(&upper);

        Some(Self {
            currency: upper,
            rate: *rate,
            symbol,
        })
    }

    /// Convert USD amount to target currency
    pub(crate) fn convert(&self, usd: f64) -> f64 {
        if usd.is_nan() {
            return f64::NAN;
        }
        usd * self.rate
    }

    /// Format a converted amount with currency symbol
    pub(crate) fn format(&self, usd: f64) -> String {
        let converted = self.convert(usd);
        if converted.is_nan() {
            return "N/A".to_string();
        }
        format!("{}{converted:.2}", self.symbol)
    }

    pub(crate) fn currency_code(&self) -> &str {
        &self.currency
    }

    #[cfg(test)]
    pub(crate) fn from_rate_for_test(currency: &str, rate: f64, symbol: &str) -> Self {
        Self {
            currency: currency.to_string(),
            rate,
            symbol: symbol.to_string(),
        }
    }
}

fn currency_symbol(code: &str) -> String {
    match code {
        "CNY" | "RMB" | "JPY" => "¥".to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        "KRW" => "₩".to_string(),
        "INR" => "₹".to_string(),
        "BRL" => "R$".to_string(),
        "CAD" | "AUD" | "USD" | "HKD" | "SGD" | "NZD" | "TWD" => "$".to_string(),
        _ => format!("{code} "),
    }
}

fn exchange_cache_file(root: &Path) -> PathBuf {
    root.join("ccstats").join(EXCHANGE_CACHE_FILE)
}

fn legacy_exchange_cache_file(home: &Path) -> PathBuf {
    home.join(".cache")
        .join("ccstats")
        .join(EXCHANGE_CACHE_FILE)
}

fn select_exchange_cache_paths(
    platform_cache_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> (Option<PathBuf>, Vec<PathBuf>) {
    let preferred = platform_cache_dir.map(exchange_cache_file);
    let legacy = home_dir.map(legacy_exchange_cache_file);
    let write_path = preferred.clone().or_else(|| legacy.clone());

    let mut read_paths = Vec::new();
    if let Some(path) = &write_path {
        read_paths.push(path.clone());
    }
    if let Some(path) = legacy
        && !read_paths.contains(&path)
    {
        read_paths.push(path);
    }

    (write_path, read_paths)
}

fn cache_paths() -> (Option<PathBuf>, Vec<PathBuf>) {
    select_exchange_cache_paths(dirs::cache_dir().as_deref(), dirs::home_dir().as_deref())
}

fn load_cached_rates() -> Option<HashMap<String, f64>> {
    let (_, read_paths) = cache_paths();
    for path in read_paths {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(age) = SystemTime::now().duration_since(modified) else {
            continue;
        };
        if age > CACHE_TTL {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        if let Ok(rates) = serde_json::from_reader(file) {
            return Some(rates);
        }
    }
    None
}

fn load_any_cached_rates() -> Option<HashMap<String, f64>> {
    let (_, read_paths) = cache_paths();
    for path in read_paths {
        let Ok(file) = File::open(&path) else {
            continue;
        };
        if let Ok(rates) = serde_json::from_reader(file) {
            return Some(rates);
        }
    }
    None
}

fn save_cached_rates(rates: &HashMap<String, f64>) {
    let Some(path) = cache_paths().0 else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("Warning: failed to create cache dir: {e}");
        return;
    }
    match File::create(&path) {
        Ok(file) => {
            if let Err(e) = serde_json::to_writer(file, rates) {
                eprintln!("Warning: failed to write exchange rate cache: {e}");
            }
        }
        Err(e) => eprintln!("Warning: failed to create exchange rate cache: {e}"),
    }
}

fn fetch_rates() -> Option<HashMap<String, f64>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let response = agent.get(EXCHANGE_RATE_URL).call().ok()?;
    let parsed: ExchangeRateResponse =
        serde_json::from_reader(response.into_body().as_reader()).ok()?;
    Some(parsed.rates)
}

fn load_rates(offline: bool) -> Option<HashMap<String, f64>> {
    if offline {
        return load_any_cached_rates();
    }

    // Try fresh cache first
    if let Some(rates) = load_cached_rates() {
        return Some(rates);
    }

    // Fetch fresh rates
    if let Some(rates) = fetch_rates() {
        save_cached_rates(&rates);
        return Some(rates);
    }

    // Fall back to any cached data
    load_any_cached_rates()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usd_converter_is_identity() {
        let conv = CurrencyConverter::load("USD", true).unwrap();
        assert!((conv.convert(10.0) - 10.0).abs() < f64::EPSILON);
        assert_eq!(conv.format(10.0), "$10.00");
    }

    #[test]
    fn usd_converter_handles_nan() {
        let conv = CurrencyConverter::load("USD", true).unwrap();
        assert!(conv.convert(f64::NAN).is_nan());
        assert_eq!(conv.format(f64::NAN), "N/A");
    }

    #[test]
    fn currency_symbol_known() {
        assert_eq!(currency_symbol("CNY"), "¥");
        assert_eq!(currency_symbol("EUR"), "€");
        assert_eq!(currency_symbol("GBP"), "£");
        assert_eq!(currency_symbol("JPY"), "¥");
    }

    #[test]
    fn currency_symbol_unknown_uses_code() {
        assert_eq!(currency_symbol("XYZ"), "XYZ ");
    }

    #[test]
    fn converter_format_with_rate() {
        let conv = CurrencyConverter {
            currency: "CNY".to_string(),
            rate: 7.25,
            symbol: "¥".to_string(),
        };
        assert_eq!(conv.format(1.0), "¥7.25");
        assert_eq!(conv.format(10.0), "¥72.50");
    }

    #[test]
    fn currency_code_accessor() {
        let conv = CurrencyConverter::load("USD", true).unwrap();
        assert_eq!(conv.currency_code(), "USD");
    }

    #[test]
    fn exchange_cache_paths_prefer_platform_cache_dir() {
        let platform = PathBuf::from("/tmp/xdg-cache");
        let home = PathBuf::from("/tmp/home");
        let (write_path, read_paths) =
            select_exchange_cache_paths(Some(platform.as_path()), Some(home.as_path()));
        assert_eq!(
            write_path,
            Some(platform.join("ccstats").join(EXCHANGE_CACHE_FILE))
        );
        assert_eq!(
            read_paths,
            vec![
                platform.join("ccstats").join(EXCHANGE_CACHE_FILE),
                home.join(".cache")
                    .join("ccstats")
                    .join(EXCHANGE_CACHE_FILE),
            ]
        );
    }

    #[test]
    fn exchange_cache_paths_fall_back_to_home_cache() {
        let home = PathBuf::from("/tmp/home");
        let (write_path, read_paths) = select_exchange_cache_paths(None, Some(home.as_path()));
        let legacy = home
            .join(".cache")
            .join("ccstats")
            .join(EXCHANGE_CACHE_FILE);
        assert_eq!(write_path, Some(legacy.clone()));
        assert_eq!(read_paths, vec![legacy]);
    }
}
