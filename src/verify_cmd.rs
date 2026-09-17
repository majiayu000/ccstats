use serde::Serialize;

use crate::app::{CommandContext, print_json};
use crate::output::OutputFormat;
use crate::pricing::{PricingDb, calculate_cost};
use crate::source::{ALL_SOURCES, Source, all_sources, get_source, load_entries};

#[derive(Debug, Default, Serialize)]
struct ReasonCounts {
    unknown_model: u64,
    cache_pricing: u64,
    tiered_pricing: u64,
    priced: u64,
}

#[derive(Debug, Serialize)]
struct SourceVerify {
    source: String,
    display_name: String,
    records: u64,
    recorded_usd: f64,
    estimated_usd: Option<f64>,
    delta_pct: Option<f64>,
    reasons: ReasonCounts,
}

pub(crate) fn handle(ctx: &CommandContext<'_>) {
    let reports = collect(ctx);
    match ctx.cli.output_format() {
        OutputFormat::Json | OutputFormat::Csv => {
            let json = serde_json::to_string(&serde_json::json!({ "sources": reports }))
                .unwrap_or_else(|_| "{}".to_string());
            if ctx.cli.csv {
                print_csv(&reports);
            } else {
                print_json(&json, ctx.jq_filter);
            }
        }
        OutputFormat::Table => print_table(&reports),
    }
}

fn selected_sources(ctx: &CommandContext<'_>) -> Vec<&'static dyn Source> {
    match ctx.cli.source.as_deref() {
        None => all_sources().collect(),
        Some(name) if name.eq_ignore_ascii_case(ALL_SOURCES) => all_sources().collect(),
        Some(name) => get_source(name).into_iter().collect(),
    }
}

fn collect(ctx: &CommandContext<'_>) -> Vec<SourceVerify> {
    let mut reports = Vec::new();
    for source in selected_sources(ctx) {
        if let Some(report) = verify_source(source, ctx) {
            reports.push(report);
        }
    }
    reports
}

fn verify_source(source: &dyn Source, ctx: &CommandContext<'_>) -> Option<SourceVerify> {
    let (entries, _, _) = load_entries(source, ctx.filter, ctx.timezone);
    let mut records = 0u64;
    let mut recorded_usd = 0.0;
    let mut estimated_usd = 0.0;
    let mut estimated_finite = true;
    let mut reasons = ReasonCounts::default();
    for entry in &entries {
        let Some(recorded) = entry.recorded_cost_usd.filter(|v| v.is_finite()) else {
            continue;
        };
        records += 1;
        recorded_usd += recorded;
        let estimated = estimated_token_cost(entry, ctx.pricing_db);
        classify(&mut reasons, entry, estimated);
        if estimated.is_finite() {
            estimated_usd += estimated;
        } else {
            estimated_finite = false;
        }
    }
    if records == 0 {
        return None;
    }
    let estimated = estimated_finite.then_some(estimated_usd);
    let delta_pct = estimated
        .filter(|_| recorded_usd > 0.0)
        .map(|est| (est - recorded_usd) / recorded_usd * 100.0);
    Some(SourceVerify {
        source: source.name().to_string(),
        display_name: source.display_name().to_string(),
        records,
        recorded_usd,
        estimated_usd: estimated,
        delta_pct,
        reasons,
    })
}

fn estimated_token_cost(entry: &crate::core::RawEntry, pricing_db: &PricingDb) -> f64 {
    let mut stats = entry.to_stats();
    stats.recorded_cost_usd = 0.0;
    stats.recorded_cost_entries = 0;
    calculate_cost(&stats, &entry.model, pricing_db)
}

fn classify(reasons: &mut ReasonCounts, entry: &crate::core::RawEntry, estimated: f64) {
    if !estimated.is_finite() {
        reasons.unknown_model += 1;
        return;
    }
    if entry.to_stats().above_272k.input_tokens > 0
        || entry.to_stats().above_272k.output_tokens > 0
        || entry.to_stats().above_272k.cache_creation > 0
    {
        reasons.tiered_pricing += 1;
        return;
    }
    if entry.cache_read > 0 || entry.cache_creation > 0 {
        reasons.cache_pricing += 1;
        return;
    }
    reasons.priced += 1;
}

fn print_table(reports: &[SourceVerify]) {
    if reports.is_empty() {
        println!(
            "No source-recorded costs in the selected range.\nHint: try OpenCode, Cursor, Goose, Hermes, Grok, or Pi."
        );
        return;
    }
    println!(
        "{:<12} {:>8} {:>12} {:>12} {:>10}  reasons",
        "source", "records", "recorded", "ccstats", "delta"
    );
    for report in reports {
        let estimated = report
            .estimated_usd
            .map_or_else(|| "unknown".to_string(), |v| format!("${v:.4}"));
        let delta = report
            .delta_pct
            .map_or_else(|| "n/a".to_string(), |v| format!("{v:+.1}%"));
        println!(
            "{:<12} {:>8} {:>12} {:>12} {:>10}  unknown {} cache {} tiered {} priced {}",
            report.source,
            report.records,
            format!("${:.4}", report.recorded_usd),
            estimated,
            delta,
            report.reasons.unknown_model,
            report.reasons.cache_pricing,
            report.reasons.tiered_pricing,
            report.reasons.priced
        );
    }
}

fn print_csv(reports: &[SourceVerify]) {
    println!(
        "source,display_name,records,recorded_usd,estimated_usd,delta_pct,unknown_model,cache_pricing,tiered_pricing,priced"
    );
    for report in reports {
        println!(
            "{},{},{},{:.6},{},{},{},{},{},{}",
            report.source,
            report.display_name,
            report.records,
            report.recorded_usd,
            report
                .estimated_usd
                .map_or(String::new(), |v| format!("{v:.6}")),
            report
                .delta_pct
                .map_or(String::new(), |v| format!("{v:.2}")),
            report.reasons.unknown_model,
            report.reasons.cache_pricing,
            report.reasons.tiered_pricing,
            report.reasons.priced
        );
    }
}
