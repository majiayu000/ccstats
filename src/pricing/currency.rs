//! Currency conversion with exchange rate caching
//!
//! Fetches rates from open.er-api.com (free, no API key required).
//! Caches to ~/.cache/ccstats/exchange_rates.json for 24h.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const EXCHANGE_RATE_URL: &str = "https://open.er-api.com/v6/latest/USD";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

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
}

fn currency_symbol(code: &str) -> String {
    match code {
        "CNY" | "RMB" => "¥".to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        "JPY" => "¥".to_string(),
        "KRW" => "₩".to_string(),
        "INR" => "₹".to_string(),
        "BRL" => "R$".to_string(),
        "CAD" | "AUD" | "USD" | "HKD" | "SGD" | "NZD" | "TWD" => "$".to_string(),
        _ => format!("{code} "),
    }
}

fn cache_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".cache")
            .join("ccstats")
            .join("exchange_rates.json"),
    )
}

fn load_cached_rates() -> Option<HashMap<String, f64>> {
    let path = cache_path()?;
    let meta = std::fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > CACHE_TTL {
        return None;
    }
    let file = File::open(&path).ok()?;
    serde_json::from_reader(file).ok()
}

fn load_any_cached_rates() -> Option<HashMap<String, f64>> {
    let path = cache_path()?;
    let file = File::open(&path).ok()?;
    serde_json::from_reader(file).ok()
}

fn save_cached_rates(rates: &HashMap<String, f64>) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Warning: failed to create cache dir: {e}");
            return;
        }
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
        assert_eq!(conv.convert(10.0), 10.0);
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
}
