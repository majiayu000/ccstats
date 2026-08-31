//! Grok CLI data source
//!
//! Complete token totals come from session `turn_completed.usage` records.
//! Durable `logs/unified.jsonl` inference records provide the subset that can
//! be priced at public API rates.

mod config;
mod cost_report;
mod ledger_lock;
mod parser;
mod unified;
mod usage;

pub(crate) use config::GrokSource;
pub(crate) use cost_report::{GrokCostReport, load_daily_with_cost_reports};

fn canonical_model_name(model: &str) -> String {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.contains("grok-4.6") {
        "grok-4.6".to_string()
    } else if normalized.contains("grok-4.5") {
        "grok-4.5".to_string()
    } else {
        model.trim().to_string()
    }
}
