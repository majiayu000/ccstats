use std::fmt::Write as _;

use comfy_table::Cell;
use serde_json::{Value, json};

use crate::core::BlockStats;
use crate::output::format::{
    NumberFormat, cost_json_value, create_styled_table, csv_escape, format_compact, format_cost,
    header_cell, right_cell,
};
use crate::output::{QuotaValueEstimate, output_quota_json, print_quota_table};
use crate::pricing::{CurrencyConverter, PricingDb, sum_model_costs};
use crate::source::CodexWeeklyQuota;
use crate::utils::Timezone;

pub(crate) const CLAUDE_WINDOW_DISCLAIMER: &str =
    "Estimated from local logs; not an official Anthropic billing reset.";

pub(crate) const BOTH_MISSING_HINT: &str = "No Codex weekly quota or Claude estimated session window is available.\nRun `ccstats doctor` to check local source setup.";

pub(crate) const NO_ACTIVE_CLAUDE_WINDOW: &str = "No active estimated 5-hour window";

pub(crate) struct ClaudeWindowView<'a> {
    pub block: &'a BlockStats,
    pub remaining_ms: i64,
}

pub(crate) struct LimitsView<'a> {
    pub want_codex: bool,
    pub want_claude: bool,
    pub codex: Option<(&'a CodexWeeklyQuota, QuotaValueEstimate<'a>)>,
    pub codex_error: Option<&'a str>,
    pub claude: Option<ClaudeWindowView<'a>>,
    pub notes: &'a [String],
}

impl LimitsView<'_> {
    fn both_missing(&self) -> bool {
        self.want_codex && self.want_claude && self.codex.is_none() && self.claude.is_none()
    }
}

fn remaining_minutes(remaining_ms: i64) -> i64 {
    remaining_ms / 60_000
}

fn format_remaining(remaining_ms: i64) -> String {
    let total_minutes = remaining_minutes(remaining_ms);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn claude_cost(block: &BlockStats, pricing_db: &PricingDb) -> f64 {
    sum_model_costs(&block.models, pricing_db)
}

fn quota_json_value(report: &CodexWeeklyQuota, value_estimate: QuotaValueEstimate<'_>) -> Value {
    serde_json::from_str(&output_quota_json(report, value_estimate)).unwrap_or(Value::Null)
}

fn claude_json_value(
    window: &ClaudeWindowView<'_>,
    pricing_db: &PricingDb,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> Value {
    let mut obj = json!({
        "block_start": window.block.block_start,
        "block_end": window.block.block_end,
        "input_tokens": window.block.stats.input_tokens,
        "output_tokens": window.block.stats.output_tokens,
        "cache_creation_tokens": window.block.stats.cache_creation,
        "cache_read_tokens": window.block.stats.cache_read,
        "total_tokens": window.block.stats.total_tokens(),
        "remaining_minutes": remaining_minutes(window.remaining_ms),
        "disclaimer": CLAUDE_WINDOW_DISCLAIMER,
    });
    if show_cost {
        obj["cost"] = cost_json_value(claude_cost(window.block, pricing_db), currency);
    }
    obj
}

pub(crate) fn output_limits_json(
    view: &LimitsView<'_>,
    pricing_db: &PricingDb,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let codex = view.codex.map_or(Value::Null, |(report, estimate)| {
        quota_json_value(report, estimate)
    });
    let claude_blocks = view.claude.as_ref().map_or(Value::Null, |window| {
        claude_json_value(window, pricing_db, show_cost, currency)
    });
    json!({
        "codex": codex,
        "claude_blocks": claude_blocks,
        "notes": view.notes,
    })
    .to_string()
}

const CSV_HEADER: &str = "section,source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at,observed_cost_usd,estimated_weekly_value_usd,block_start,block_end,input_tokens,output_tokens,total_tokens,remaining_minutes,cost,disclaimer,error";

fn csv_f64(value: Option<f64>, precision: usize) -> String {
    value.map_or(String::new(), |v| format!("{v:.precision$}"))
}

fn csv_i64(value: Option<i64>) -> String {
    value.map_or(String::new(), |v| v.to_string())
}

fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn quota_value_columns(value_estimate: QuotaValueEstimate<'_>) -> (String, String) {
    match value_estimate {
        Some(Ok(estimate)) => (
            format!("{:.6}", estimate.observed_cost_usd),
            format!("{:.6}", estimate.estimated_weekly_value_usd),
        ),
        Some(Err(_)) | None => (String::new(), String::new()),
    }
}

#[derive(Default)]
struct LimitsCsvRow<'a> {
    section: &'a str,
    source: &'a str,
    window: &'a str,
    window_minutes: Option<i64>,
    used_pct: Option<f64>,
    remaining_pct: Option<f64>,
    projected_pct: Option<f64>,
    status: &'a str,
    observed_at: &'a str,
    resets_at: &'a str,
    estimated_depletion_at: &'a str,
    observed_cost: &'a str,
    weekly_value: &'a str,
    block_start: &'a str,
    block_end: &'a str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    remaining_minutes: Option<i64>,
    cost: &'a str,
    disclaimer: &'a str,
    error: &'a str,
}

impl LimitsCsvRow<'_> {
    fn write_to(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.section,
            self.source,
            self.window,
            csv_i64(self.window_minutes),
            csv_f64(self.used_pct, 2),
            csv_f64(self.remaining_pct, 2),
            csv_f64(self.projected_pct, 2),
            self.status,
            self.observed_at,
            self.resets_at,
            self.estimated_depletion_at,
            self.observed_cost,
            self.weekly_value,
            csv_escape(self.block_start),
            csv_escape(self.block_end),
            csv_i64(self.input_tokens),
            csv_i64(self.output_tokens),
            csv_i64(self.total_tokens),
            csv_i64(self.remaining_minutes),
            self.cost,
            csv_escape(self.disclaimer),
            csv_escape(self.error),
        );
    }
}

pub(crate) fn output_limits_csv(
    view: &LimitsView<'_>,
    pricing_db: &PricingDb,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let mut out = format!("{CSV_HEADER}\n");
    if view.want_codex {
        if let Some((report, estimate)) = view.codex {
            let depletion = report
                .estimated_depletion_at
                .map(timestamp)
                .unwrap_or_default();
            let observed_at = timestamp(report.observed_at);
            let resets_at = timestamp(report.resets_at);
            let (observed_cost, weekly_value) = quota_value_columns(estimate);
            LimitsCsvRow {
                section: "codex",
                source: "codex",
                window: "weekly",
                window_minutes: Some(report.window_minutes),
                used_pct: Some(report.used_pct),
                remaining_pct: Some(report.remaining_pct),
                projected_pct: Some(report.projected_pct_at_reset),
                status: report.status.as_str(),
                observed_at: &observed_at,
                resets_at: &resets_at,
                estimated_depletion_at: &depletion,
                observed_cost: &observed_cost,
                weekly_value: &weekly_value,
                ..Default::default()
            }
            .write_to(&mut out);
        } else {
            LimitsCsvRow {
                section: "codex",
                source: "codex",
                window: "weekly",
                error: view.codex_error.unwrap_or_default(),
                ..Default::default()
            }
            .write_to(&mut out);
        }
    }
    if view.want_claude {
        if let Some(window) = &view.claude {
            let cost = if show_cost {
                format_cost(claude_cost(window.block, pricing_db), currency)
            } else {
                String::new()
            };
            LimitsCsvRow {
                section: "claude",
                source: "claude",
                window: "estimated_5h",
                block_start: &window.block.block_start,
                block_end: &window.block.block_end,
                input_tokens: Some(window.block.stats.input_tokens),
                output_tokens: Some(window.block.stats.output_tokens),
                total_tokens: Some(window.block.stats.total_tokens()),
                remaining_minutes: Some(remaining_minutes(window.remaining_ms)),
                cost: &cost,
                disclaimer: CLAUDE_WINDOW_DISCLAIMER,
                ..Default::default()
            }
            .write_to(&mut out);
        } else {
            LimitsCsvRow {
                section: "claude",
                source: "claude",
                window: "estimated_5h",
                disclaimer: CLAUDE_WINDOW_DISCLAIMER,
                error: NO_ACTIVE_CLAUDE_WINDOW,
                ..Default::default()
            }
            .write_to(&mut out);
        }
    }
    out
}

pub(crate) struct LimitsTableOptions<'a> {
    pub timezone: Timezone,
    pub number_format: NumberFormat,
    pub use_color: bool,
    pub show_cost: bool,
    pub currency: Option<&'a CurrencyConverter>,
}

pub(crate) fn print_limits_table(
    view: &LimitsView<'_>,
    pricing_db: &PricingDb,
    options: &LimitsTableOptions<'_>,
) {
    if view.both_missing() {
        println!("{BOTH_MISSING_HINT}");
        return;
    }

    if view.want_codex {
        println!("Codex weekly quota");
        if let Some((report, estimate)) = view.codex {
            print_quota_table(
                report,
                estimate,
                options.timezone,
                options.number_format,
                options.use_color,
            );
        } else {
            println!(
                "unavailable: {}",
                view.codex_error
                    .unwrap_or("Codex weekly quota is not available")
            );
        }
        if view.want_claude {
            println!();
        }
    }

    if view.want_claude {
        println!("Claude estimated session window");
        if let Some(window) = &view.claude {
            print_claude_window_table(window, pricing_db, options);
        } else {
            println!("{NO_ACTIVE_CLAUDE_WINDOW}");
        }
        println!("{CLAUDE_WINDOW_DISCLAIMER}");
    }
}

fn print_claude_window_table(
    window: &ClaudeWindowView<'_>,
    pricing_db: &PricingDb,
    options: &LimitsTableOptions<'_>,
) {
    let mut table = create_styled_table();
    let mut header = vec![
        header_cell("Window", options.use_color),
        header_cell("Tokens", options.use_color),
        header_cell("Remaining", options.use_color),
    ];
    if options.show_cost {
        header.push(header_cell("Cost", options.use_color));
    }
    table.set_header(header);

    let label = format!("{} - {}", window.block.block_start, window.block.block_end);
    let mut row = vec![
        Cell::new(label),
        right_cell(
            &format_compact(window.block.stats.total_tokens(), options.number_format),
            None,
            false,
        ),
        Cell::new(format_remaining(window.remaining_ms)),
    ];
    if options.show_cost {
        row.push(right_cell(
            &format_cost(claude_cost(window.block, pricing_db), options.currency),
            None,
            false,
        ));
    }
    table.add_row(row);
    println!("{table}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::CodexQuotaStatus;
    use chrono::DateTime;
    use std::collections::HashMap;

    use crate::core::Stats;
    use crate::sdk::CodexWeeklyValueEstimate;

    fn report() -> CodexWeeklyQuota {
        CodexWeeklyQuota {
            observed_at: "2026-08-22T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            resets_at: "2026-08-28T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            estimated_depletion_at: Some("2026-08-25T00:00:00Z".parse::<DateTime<_>>().unwrap()),
            window_minutes: 10_080,
            used_pct: 25.0,
            remaining_pct: 75.0,
            projected_pct_at_reset: 175.0,
            status: CodexQuotaStatus::LikelyExhausted,
        }
    }

    fn estimate() -> CodexWeeklyValueEstimate {
        CodexWeeklyValueEstimate {
            observed_at: "2026-08-22T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            window_started_at: "2026-08-21T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            resets_at: "2026-08-28T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            used_pct: 25.0,
            observed_cost_usd: 50.0,
            estimated_weekly_value_usd: 200.0,
            observed_tokens: 1_000_000,
            estimated_weekly_tokens: 4_000_000.0,
            valid_entries: 12,
            dedup_skipped_entries: 2,
        }
    }

    fn block() -> BlockStats {
        BlockStats {
            block_start: "2026-09-03 12:00".to_string(),
            block_end: "17:00".to_string(),
            start_ms: 1,
            stats: Stats {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            models: HashMap::new(),
        }
    }

    #[test]
    fn json_uses_null_for_missing_codex_without_zero_percent() {
        let block = block();
        let notes = vec![CLAUDE_WINDOW_DISCLAIMER.to_string()];
        let view = LimitsView {
            want_codex: true,
            want_claude: true,
            codex: None,
            codex_error: Some("snapshot missing"),
            claude: Some(ClaudeWindowView {
                block: &block,
                remaining_ms: 90 * 60 * 1000,
            }),
            notes: &notes,
        };
        let value: Value = serde_json::from_str(&output_limits_json(
            &view,
            &PricingDb::default(),
            false,
            None,
        ))
        .unwrap();
        assert!(value["codex"].is_null());
        assert!(value["codex"].get("used_pct").is_none());
        assert_eq!(value["claude_blocks"]["total_tokens"], 150);
        assert_eq!(value["claude_blocks"]["remaining_minutes"], 90);
        assert!(
            value["claude_blocks"]["disclaimer"]
                .as_str()
                .unwrap()
                .contains("not an official")
        );
        assert!(
            !value["claude_blocks"]
                .as_object()
                .unwrap()
                .contains_key("used_pct")
        );
    }

    #[test]
    fn csv_leaves_missing_sections_blank_not_zero() {
        let notes = Vec::<String>::new();
        let view = LimitsView {
            want_codex: true,
            want_claude: true,
            codex: None,
            codex_error: Some("snapshot missing"),
            claude: None,
            notes: &notes,
        };
        let csv = output_limits_csv(&view, &PricingDb::default(), false, None);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(CSV_HEADER));
        let codex = lines.next().unwrap();
        assert!(codex.starts_with("codex,codex,weekly,"));
        assert!(!codex.contains(",0.00,"));
        assert!(codex.contains("snapshot missing"));
        let claude = lines.next().unwrap();
        assert!(claude.starts_with("claude,claude,estimated_5h,"));
        assert!(!claude.contains(",0,") || claude.contains(NO_ACTIVE_CLAUDE_WINDOW));
        let fields: Vec<_> = claude.split(',').collect();
        // used_pct / remaining_pct / projected stay empty
        assert_eq!(fields[4], "");
        assert_eq!(fields[5], "");
        assert_eq!(fields[6], "");
    }

    #[test]
    fn json_includes_codex_quota_fields_when_present() {
        let report = report();
        let estimate = estimate();
        let notes = Vec::<String>::new();
        let view = LimitsView {
            want_codex: true,
            want_claude: false,
            codex: Some((&report, Some(Ok(&estimate)))),
            codex_error: None,
            claude: None,
            notes: &notes,
        };
        let value: Value = serde_json::from_str(&output_limits_json(
            &view,
            &PricingDb::default(),
            true,
            None,
        ))
        .unwrap();
        assert_eq!(value["codex"]["used_pct"], 25.0);
        assert!(value["claude_blocks"].is_null());
    }
}
