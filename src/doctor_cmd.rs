//! Read-only source diagnostics for first-run setup and troubleshooting.

use crate::app::{CommandContext, print_json};
use crate::output::{OutputFormat, csv_escape};
use crate::source::{DiagnosticStatus, Source, SourceDiagnostic, all_sources};
use serde_json::json;

struct DiagnosticRow {
    source: &'static dyn Source,
    diagnostic: SourceDiagnostic,
}

pub(crate) fn handle_doctor(ctx: &CommandContext<'_>) {
    let rows = all_sources()
        .map(|source| DiagnosticRow {
            source,
            diagnostic: source.diagnose(),
        })
        .collect::<Vec<_>>();

    match ctx.cli.output_format() {
        OutputFormat::Table => render_table(&rows),
        OutputFormat::Json => render_json(&rows, ctx),
        OutputFormat::Csv => render_csv(&rows),
    }
}

fn render_table(rows: &[DiagnosticRow]) {
    println!("Source diagnostics (read-only; remote providers are not contacted):");
    for row in rows {
        println!(
            "- {:<10} {:<16} {}",
            row.diagnostic.status.as_str(),
            row.source.display_name(),
            row.diagnostic.detail
        );
        if row.diagnostic.status == DiagnosticStatus::Missing {
            println!("  Setup: {}.", row.source.setup_hint());
        }
    }

    if rows
        .iter()
        .all(|row| row.diagnostic.status != DiagnosticStatus::Detected)
    {
        println!(
            "\nNo source data detected. Complete one setup step above, then rerun `ccstats doctor --json`."
        );
    } else {
        println!("\nNext: run `ccstats daily --source all` for a combined report.");
    }
}

fn render_json(rows: &[DiagnosticRow], ctx: &CommandContext<'_>) {
    let payload = rows
        .iter()
        .map(|row| {
            json!({
                "name": row.source.name(),
                "display_name": row.source.display_name(),
                "status": row.diagnostic.status.as_str(),
                "files": row.diagnostic.files,
                "detail": row.diagnostic.detail,
                "setup": row.source.setup_hint(),
            })
        })
        .collect::<Vec<_>>();
    match serde_json::to_string(&payload) {
        Ok(json) => print_json(&json, ctx.jq_filter),
        Err(error) => {
            eprintln!("Error: failed to serialize doctor output: {error}");
            std::process::exit(1);
        }
    }
}

fn render_csv(rows: &[DiagnosticRow]) {
    println!("name,display_name,status,files,detail,setup");
    for row in rows {
        println!(
            "{},{},{},{},{},{}",
            csv_escape(row.source.name()),
            csv_escape(row.source.display_name()),
            row.diagnostic.status.as_str(),
            row.diagnostic.files,
            csv_escape(&row.diagnostic.detail),
            csv_escape(row.source.setup_hint())
        );
    }
}
