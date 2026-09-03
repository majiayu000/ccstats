use crate::app::{CommandContext, print_json};
use crate::core::select_active_block;
use crate::output::{
    BOTH_MISSING_HINT, CLAUDE_WINDOW_DISCLAIMER, ClaudeWindowView, LimitsTableOptions, LimitsView,
    NO_ACTIVE_CLAUDE_WINDOW, OutputFormat, output_limits_csv, output_limits_json,
    print_limits_table,
};
use crate::quota_cmd::load_quota;
use crate::source::{ALL_SOURCES, get_source, load_blocks};
use chrono::Utc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitsScope {
    All,
    Codex,
    Claude,
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
            "limits only supports --source claude, --source codex, or --source all".to_string(),
        );
    };
    match resolved.name() {
        "codex" => Ok(LimitsScope::Codex),
        "claude" => Ok(LimitsScope::Claude),
        other => Err(format!(
            "limits does not support --source {other}; only claude, codex, or all"
        )),
    }
}

pub(crate) fn handle_limits(ctx: &CommandContext<'_>) {
    let scope = match limits_scope(ctx.cli.source.as_deref()) {
        Ok(scope) => scope,
        Err(message) => {
            eprintln!("Error: {message}");
            std::process::exit(1);
        }
    };

    let want_codex = matches!(scope, LimitsScope::All | LimitsScope::Codex);
    let want_claude = matches!(scope, LimitsScope::All | LimitsScope::Claude);

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

    let claude_blocks = want_claude.then(|| {
        let Some(source) = get_source("claude") else {
            return Vec::new();
        };
        load_blocks(source, ctx.filter, ctx.timezone, true)
    });
    let claude = claude_blocks
        .as_deref()
        .and_then(|blocks| select_active_block(blocks, Utc::now()))
        .map(|(block, remaining_ms)| ClaudeWindowView {
            block,
            remaining_ms,
        });

    if want_claude {
        if claude.is_none() {
            notes.push(NO_ACTIVE_CLAUDE_WINDOW.to_string());
        }
        notes.push(CLAUDE_WINDOW_DISCLAIMER.to_string());
    }

    if want_codex && want_claude && loaded_quota.is_none() && claude.is_none() {
        notes.push(BOTH_MISSING_HINT.replace('\n', " "));
    }

    let view = LimitsView {
        want_codex,
        want_claude,
        codex: loaded_quota
            .as_ref()
            .map(|loaded| (&loaded.report, loaded.rendered())),
        codex_error: codex_error.as_deref(),
        claude,
        notes: &notes,
    };

    match ctx.cli.output_format() {
        OutputFormat::Json => print_json(
            &output_limits_json(&view, ctx.pricing_db, ctx.cli.show_cost(), ctx.currency),
            ctx.jq_filter,
        ),
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
