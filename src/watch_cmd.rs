use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use chrono::{Datelike, Utc};
use serde_json::json;

use crate::app::{CommandContext, print_json};
use crate::core::DateFilter;
use crate::limits_cmd::collect_limits;
use crate::output::{LimitWindow, format_cost};
use crate::pricing::sum_model_costs;
use crate::source::{ALL_SOURCES, Source, all_sources, get_source, load_daily};

const REFRESH: Duration = Duration::from_secs(5);

struct PeriodCost {
    label: &'static str,
    cost: f64,
    shares: Vec<(String, f64)>,
}

pub(crate) fn handle(once: bool, ctx: &CommandContext<'_>) {
    let warn_pct = ctx.cli.watch_warn_pct();
    loop {
        let frame = render_frame(once, ctx);
        if ctx.cli.json {
            print_json(&frame.json, ctx.jq_filter);
        } else {
            if !once && io::stdout().is_terminal() {
                print!("\x1b[2J\x1b[H");
            }
            print!("{}", frame.text);
            let _ = io::stdout().flush();
        }
        if once {
            if frame.hot && warn_pct > 0.0 {
                std::process::exit(1);
            }
            return;
        }
        thread::sleep(REFRESH);
    }
}

struct Frame {
    text: String,
    json: String,
    hot: bool,
}

fn render_frame(once: bool, ctx: &CommandContext<'_>) -> Frame {
    let today = ctx.timezone.to_fixed_offset(Utc::now()).date_naive();
    let week_start =
        today - chrono::Duration::days(i64::from(today.weekday().num_days_from_monday()));
    let today_cost = period_cost("today", &DateFilter::new(Some(today), Some(today)), ctx);
    let week_cost = period_cost("week", &DateFilter::new(Some(week_start), Some(today)), ctx);
    let snapshot = collect_limits(ctx, ctx.cli.source.as_deref()).ok();
    let windows = snapshot.as_ref().map(super_windows).unwrap_or_default();
    let hot = snapshot
        .as_ref()
        .is_some_and(|snap| snap.hot_official(ctx.cli.watch_warn_pct()));

    let mut text = String::new();
    let _ = writeln!(
        text,
        "ccstats watch  {today} {}",
        ctx.timezone.to_fixed_offset(Utc::now()).format("%H:%M")
    );
    text.push_str(&format_period_line(&today_cost, ctx));
    text.push_str(&format_period_line(&week_cost, ctx));
    text.push('\n');
    if windows.is_empty() {
        text.push_str("No provider windows. Run `ccstats limits` or `ccstats doctor`.\n");
    } else {
        for window in &windows {
            text.push_str(&format_window_line(window));
            text.push('\n');
        }
    }
    if !once {
        text.push_str("\nrefreshing every 5s  Ctrl-C to stop\n");
    }

    let json = json!({
        "today": cost_json(&today_cost),
        "week": cost_json(&week_cost),
        "windows": windows,
        "hot": hot,
    })
    .to_string();

    Frame { text, json, hot }
}

fn super_windows(snapshot: &crate::limits_cmd::LimitsSnapshot) -> Vec<LimitWindow> {
    snapshot.windows().to_vec()
}

fn selected_sources(ctx: &CommandContext<'_>) -> Vec<&'static dyn Source> {
    match ctx.cli.source.as_deref() {
        None => all_sources().collect(),
        Some(name) if name.eq_ignore_ascii_case(ALL_SOURCES) => all_sources().collect(),
        Some(name) => get_source(name).into_iter().collect(),
    }
}

fn period_cost(label: &'static str, filter: &DateFilter, ctx: &CommandContext<'_>) -> PeriodCost {
    let mut shares = Vec::new();
    let mut total = 0.0;
    for source in selected_sources(ctx) {
        let result = load_daily(source, filter, ctx.timezone, true, ctx.cli.debug);
        let mut cost = 0.0;
        for day in result.day_stats.values() {
            let part = sum_model_costs(&day.models, ctx.pricing_db);
            if part.is_finite() {
                cost += part;
            }
        }
        if cost > 0.0 || !result.day_stats.is_empty() {
            shares.push((source.display_name().to_string(), cost));
            if cost.is_finite() {
                total += cost;
            }
        }
    }
    shares.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    PeriodCost {
        label,
        cost: total,
        shares,
    }
}

fn format_period_line(period: &PeriodCost, ctx: &CommandContext<'_>) -> String {
    let amount = if period.cost.is_finite() && ctx.cli.show_cost() {
        format_cost(period.cost, ctx.currency)
    } else if ctx.cli.show_cost() {
        "unknown".to_string()
    } else {
        String::new()
    };
    let mut line = format!("{:<6} {amount}", period.label);
    if period.cost.is_finite() && period.cost > 0.0 && period.shares.len() > 1 {
        let parts: Vec<String> = period
            .shares
            .iter()
            .take(4)
            .map(|(name, cost)| {
                let pct = cost / period.cost * 100.0;
                format!("{name} {pct:.0}%")
            })
            .collect();
        line.push_str("  (");
        line.push_str(&parts.join(", "));
        line.push(')');
    }
    line.push('\n');
    line
}

fn format_window_line(window: &LimitWindow) -> String {
    let pct = window.used_pct.unwrap_or(f64::NAN);
    let bar = progress_bar(pct);
    let pct_text = if pct.is_finite() {
        let suffix = if window.source == "official" {
            String::new()
        } else {
            " est.".to_string()
        };
        format!("{pct:.0}%{suffix}")
    } else {
        "unknown".to_string()
    };
    let reset = window.resets_at.as_deref().unwrap_or("");
    let stale = if window.stale { " stale" } else { "" };
    let burn = window
        .burn_pct_per_hour
        .map(|rate| format!("  burn {rate:.1}%/h"))
        .unwrap_or_default();
    format!(
        "{:<7} {:<12} {bar} {pct_text}  {reset}{stale}{burn}",
        window.provider, window.window
    )
}

fn progress_bar(pct: f64) -> String {
    if !pct.is_finite() {
        return "[??????????]".to_string();
    }
    let filled = ((pct / 10.0).round() as usize).min(10);
    format!("[{}{}]", "#".repeat(filled), "-".repeat(10 - filled))
}

fn cost_json(period: &PeriodCost) -> serde_json::Value {
    json!({
        "label": period.label,
        "cost": if period.cost.is_finite() { serde_json::Value::from(period.cost) } else { serde_json::Value::Null },
        "shares": period.shares.iter().map(|(name, cost)| json!({
            "source": name,
            "cost": if cost.is_finite() { serde_json::Value::from(*cost) } else { serde_json::Value::Null },
        })).collect::<Vec<_>>(),
    })
}
