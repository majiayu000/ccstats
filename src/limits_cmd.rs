use crate::app::{CommandContext, print_json};
use crate::core::{BlockStats, select_active_block};
use crate::output::{
    BOTH_MISSING_HINT, CLAUDE_WINDOW_DISCLAIMER, ClaudeWindowView, CursorPlanView, LimitWindow,
    LimitsTableOptions, LimitsView, NO_ACTIVE_CLAUDE_WINDOW, OutputFormat, output_limits_csv,
    output_limits_json, print_limits_table,
};
use crate::pricing::sum_model_costs;
use crate::quota::{load_claude_quota_state, scale_observed_to_full_window};
use crate::quota_cmd::{LoadedQuota, load_quota};
use crate::source::{
    ALL_SOURCES, CursorPlanUsage, fetch_cursor_plan_usage, get_source, load_blocks,
};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitsScope {
    All,
    Codex,
    Claude,
    Cursor,
}

pub(crate) struct LimitsSnapshot {
    want_codex: bool,
    want_claude: bool,
    want_cursor: bool,
    loaded_quota: Option<LoadedQuota>,
    codex_error: Option<String>,
    claude_blocks: Vec<BlockStats>,
    cursor: Option<CursorPlanUsage>,
    cursor_error: Option<String>,
    windows: Vec<LimitWindow>,
    notes: Vec<String>,
}

fn limits_scope(source: Option<&str>) -> Result<LimitsScope, String> {
    let Some(name) = source else {
        return Ok(LimitsScope::All);
    };
    if name.eq_ignore_ascii_case(ALL_SOURCES) {
        return Ok(LimitsScope::All);
    }
    let Some(resolved) = get_source(name) else {
        return Err(
            "limits only supports --source claude, --source codex, --source cursor, or --source all"
                .to_string(),
        );
    };
    match resolved.name() {
        "codex" => Ok(LimitsScope::Codex),
        "claude" => Ok(LimitsScope::Claude),
        "cursor" => Ok(LimitsScope::Cursor),
        other => Err(format!(
            "limits does not support --source {other}; only claude, codex, cursor, or all"
        )),
    }
}

fn timestamp_secs(unix: i64) -> Option<String> {
    DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn window(
    provider: &str,
    name: &str,
    used_pct: Option<f64>,
    resets_at: Option<String>,
    source: &str,
    stale: bool,
) -> LimitWindow {
    LimitWindow {
        provider: provider.to_string(),
        window: name.to_string(),
        used_pct,
        resets_at,
        source: source.to_string(),
        stale,
        value_estimate_usd: None,
        burn_pct_per_hour: None,
    }
}

pub(crate) fn collect_limits(
    ctx: &CommandContext<'_>,
    source: Option<&str>,
) -> Result<LimitsSnapshot, String> {
    let scope = limits_scope(source)?;
    let want_codex = matches!(scope, LimitsScope::All | LimitsScope::Codex);
    let want_claude = matches!(scope, LimitsScope::All | LimitsScope::Claude);
    let want_cursor = matches!(scope, LimitsScope::All | LimitsScope::Cursor);
    let now = Utc::now();

    let mut notes = Vec::new();
    let mut loaded_quota = None;
    let mut codex_error = None;
    if want_codex {
        match load_quota(ctx) {
            Ok(loaded) => loaded_quota = Some(loaded),
            Err(error) => {
                let message = error.to_string();
                notes.push(format!("Codex weekly quota unavailable: {message}"));
                codex_error = Some(message);
            }
        }
    }

    let claude_blocks = if want_claude {
        get_source("claude").map_or_else(Vec::new, |source| {
            load_blocks(source, ctx.filter, ctx.timezone, true)
        })
    } else {
        Vec::new()
    };
    let claude_active = select_active_block(&claude_blocks, now);
    if want_claude {
        if claude_active.is_none() {
            notes.push(NO_ACTIVE_CLAUDE_WINDOW.to_string());
        }
        notes.push(CLAUDE_WINDOW_DISCLAIMER.to_string());
    }

    let mut cursor = None;
    let mut cursor_error = None;
    if want_cursor {
        match fetch_cursor_plan_usage(ctx.cli.debug) {
            Ok(plan) => cursor = Some(plan),
            Err(message) => {
                notes.push(format!("Cursor plan unavailable: {message}"));
                cursor_error = Some(message);
            }
        }
    }

    if want_codex
        && want_claude
        && loaded_quota.is_none()
        && claude_active.is_none()
        && (!want_cursor || cursor.is_none())
    {
        notes.push(BOTH_MISSING_HINT.replace('\n', " "));
    }

    let mut windows = Vec::new();
    extend_codex_windows(&mut windows, loaded_quota.as_ref());
    extend_claude_windows(
        &mut windows,
        &mut notes,
        want_claude,
        &claude_blocks,
        ctx,
        now,
    );
    extend_cursor_windows(&mut windows, cursor.as_ref());

    Ok(LimitsSnapshot {
        want_codex,
        want_claude,
        want_cursor,
        loaded_quota,
        codex_error,
        claude_blocks,
        cursor,
        cursor_error,
        windows,
        notes,
    })
}

fn extend_codex_windows(windows: &mut Vec<LimitWindow>, loaded: Option<&LoadedQuota>) {
    let Some(loaded) = loaded else {
        return;
    };
    let mut row = window(
        "codex",
        "weekly",
        Some(loaded.report.used_pct),
        Some(
            loaded
                .report
                .resets_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
        "official",
        false,
    );
    row.value_estimate_usd = loaded
        .value_estimate
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|estimate| estimate.estimated_weekly_value_usd);
    windows.push(row);
}

fn extend_claude_windows(
    windows: &mut Vec<LimitWindow>,
    notes: &mut Vec<String>,
    want_claude: bool,
    claude_blocks: &[BlockStats],
    ctx: &CommandContext<'_>,
    now: DateTime<Utc>,
) {
    if !want_claude {
        return;
    }
    let quota_state = load_claude_quota_state();
    let stale = quota_state.is_stale(now);
    let claude_active = select_active_block(claude_blocks, now);
    let claude_cost = claude_active
        .as_ref()
        .map(|(block, _)| sum_model_costs(&block.models, ctx.pricing_db));
    if let Some(tte) = quota_state.time_to_exhaustion(true) {
        notes.push(format!(
            "Claude 5h time-to-exhaustion ~{}m at current burn",
            tte.as_secs() / 60
        ));
    }
    if let Some(latest) = quota_state.latest() {
        if let Some(five) = &latest.five_hour {
            let mut row = window(
                "claude",
                "five_hour",
                five.used_percentage,
                five.resets_at.and_then(timestamp_secs),
                "official",
                stale,
            );
            row.value_estimate_usd = five.used_percentage.and_then(|pct| {
                claude_cost.and_then(|cost| scale_observed_to_full_window(cost, pct))
            });
            row.burn_pct_per_hour = quota_state.burn_pct_per_hour(true);
            windows.push(row);
        }
        if let Some(seven) = &latest.seven_day {
            let mut row = window(
                "claude",
                "seven_day",
                seven.used_percentage,
                seven.resets_at.and_then(timestamp_secs),
                "official",
                stale,
            );
            row.burn_pct_per_hour = quota_state.burn_pct_per_hour(false);
            windows.push(row);
        }
    }
    if !windows
        .iter()
        .any(|item| item.provider == "claude" && item.window == "five_hour")
        && let Some((block, remaining_ms)) = claude_active
    {
        let remaining_minutes = remaining_ms / 60_000;
        let mut row = window(
            "claude",
            "estimated_5h",
            None,
            Some(format!("{remaining_minutes}m remaining")),
            "estimated",
            false,
        );
        row.value_estimate_usd = ctx
            .cli
            .show_cost()
            .then(|| sum_model_costs(&block.models, ctx.pricing_db))
            .filter(|cost| cost.is_finite());
        windows.push(row);
    }
}

fn extend_cursor_windows(windows: &mut Vec<LimitWindow>, plan: Option<&CursorPlanUsage>) {
    let Some(plan) = plan else {
        return;
    };
    windows.push(window(
        "cursor",
        "billing_cycle",
        plan.used_pct,
        plan.billing_cycle_end.clone(),
        "official",
        false,
    ));
}

impl LimitsSnapshot {
    fn view(&self) -> LimitsView<'_> {
        let claude =
            select_active_block(&self.claude_blocks, Utc::now()).map(|(block, remaining_ms)| {
                ClaudeWindowView {
                    block,
                    remaining_ms,
                }
            });
        let cursor = self.cursor.as_ref().map(|plan| CursorPlanView {
            membership: plan.membership.as_deref(),
            used_pct: plan.used_pct,
            billing_cycle_start: plan.billing_cycle_start.as_deref(),
            billing_cycle_end: plan.billing_cycle_end.as_deref(),
        });
        LimitsView {
            want_codex: self.want_codex,
            want_claude: self.want_claude,
            want_cursor: self.want_cursor,
            codex: self
                .loaded_quota
                .as_ref()
                .map(|loaded| (&loaded.report, loaded.rendered())),
            codex_error: self.codex_error.as_deref(),
            claude,
            cursor,
            cursor_error: self.cursor_error.as_deref(),
            windows: &self.windows,
            notes: &self.notes,
        }
    }

    pub(crate) fn json(&self, ctx: &CommandContext<'_>) -> String {
        output_limits_json(
            &self.view(),
            ctx.pricing_db,
            ctx.cli.show_cost(),
            ctx.currency,
        )
    }

    pub(crate) fn hot_official(&self, warn_pct: f64) -> bool {
        self.windows.iter().any(|window| {
            window.source == "official"
                && window
                    .used_pct
                    .is_some_and(|pct| pct.is_finite() && pct >= warn_pct)
        })
    }

    pub(crate) fn windows(&self) -> &[LimitWindow] {
        &self.windows
    }
}

pub(crate) fn limits_json(
    ctx: &CommandContext<'_>,
    source: Option<&str>,
) -> Result<String, String> {
    Ok(collect_limits(ctx, source)?.json(ctx))
}

pub(crate) fn handle_limits(ctx: &CommandContext<'_>) {
    let snapshot = match collect_limits(ctx, ctx.cli.source.as_deref()) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            eprintln!("Error: {message}");
            std::process::exit(1);
        }
    };
    let view = snapshot.view();
    match ctx.cli.output_format() {
        OutputFormat::Json => print_json(&snapshot.json(ctx), ctx.jq_filter),
        OutputFormat::Csv => print!(
            "{}",
            output_limits_csv(&view, ctx.pricing_db, ctx.cli.show_cost(), ctx.currency)
        ),
        OutputFormat::Table => print_limits_table(
            &view,
            ctx.pricing_db,
            &LimitsTableOptions {
                timezone: ctx.timezone,
                number_format: ctx.number_format,
                use_color: ctx.cli.use_color(),
                show_cost: ctx.cli.show_cost(),
                currency: ctx.currency,
            },
        ),
    }
}
