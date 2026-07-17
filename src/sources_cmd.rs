//! Handler and renderers for the `sources` subcommand (capability listing).
//!
//! Lives in its own module to keep `app.rs` under the module size limit.

use crate::app::{CommandContext, print_json};
use crate::output::OutputFormat;
use crate::source::{ALL_SOURCES, Capabilities, Source, all_capabilities, all_sources};
use serde_json::json;

pub(crate) fn handle_sources(ctx: &CommandContext<'_>) {
    let sources: Vec<&dyn Source> = all_sources().collect();
    let mut all_caps = all_capabilities();
    all_caps.has_projects = false;
    all_caps.has_billing_blocks = false;

    render_sources(&sources, &all_caps, ctx);
}

fn render_sources(sources: &[&dyn Source], all_caps: &Capabilities, ctx: &CommandContext<'_>) {
    match ctx.cli.output_format() {
        OutputFormat::Csv => render_sources_csv(sources, all_caps),
        OutputFormat::Json => render_sources_json(sources, all_caps, ctx),
        OutputFormat::Table => print_sources_table(sources, all_caps),
    }
}

fn render_sources_csv(sources: &[&dyn Source], all_caps: &Capabilities) {
    println!(
        "name,display_name,aliases,has_projects,has_billing_blocks,has_reasoning_tokens,has_cache_creation,has_cache_read,needs_dedup"
    );
    println!(
        "{},All Sources,,{},{},{},{},{},{}",
        ALL_SOURCES,
        all_caps.has_projects,
        all_caps.has_billing_blocks,
        all_caps.has_reasoning_tokens,
        all_caps.has_cache_creation,
        all_caps.has_cache_read,
        all_caps.needs_dedup
    );
    for source in sources {
        let caps = source.capabilities();
        let aliases = source.aliases().join("|");
        println!(
            "{},{},{},{},{},{},{},{},{}",
            source.name(),
            source.display_name(),
            aliases,
            caps.has_projects,
            caps.has_billing_blocks,
            caps.has_reasoning_tokens,
            caps.has_cache_creation,
            caps.has_cache_read,
            caps.needs_dedup
        );
    }
}

fn render_sources_json(sources: &[&dyn Source], all_caps: &Capabilities, ctx: &CommandContext<'_>) {
    let mut payload = vec![json!({
        "name": ALL_SOURCES,
        "display_name": "All Sources",
        "aliases": [],
        "capabilities": {
            "has_projects": all_caps.has_projects,
            "has_billing_blocks": all_caps.has_billing_blocks,
            "has_reasoning_tokens": all_caps.has_reasoning_tokens,
            "has_cache_creation": all_caps.has_cache_creation,
            "has_cache_read": all_caps.has_cache_read,
            "needs_dedup": all_caps.needs_dedup
        }
    })];
    payload.extend(sources.iter().map(|source| {
        let caps = source.capabilities();
        json!({
            "name": source.name(),
            "display_name": source.display_name(),
            "aliases": source.aliases(),
            "capabilities": {
                "has_projects": caps.has_projects,
                "has_billing_blocks": caps.has_billing_blocks,
                "has_reasoning_tokens": caps.has_reasoning_tokens,
                "has_cache_creation": caps.has_cache_creation,
                "has_cache_read": caps.has_cache_read,
                "needs_dedup": caps.needs_dedup
            }
        })
    }));
    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string());
    print_json(&json, ctx.jq_filter);
}

fn print_sources_table(sources: &[&dyn Source], all_caps: &Capabilities) {
    println!("Available sources:");
    println!(
        "- {} (All Sources) aliases: - | has_projects={} has_billing_blocks={} has_reasoning_tokens={} has_cache_creation={} has_cache_read={} needs_dedup={}",
        ALL_SOURCES,
        all_caps.has_projects,
        all_caps.has_billing_blocks,
        all_caps.has_reasoning_tokens,
        all_caps.has_cache_creation,
        all_caps.has_cache_read,
        all_caps.needs_dedup
    );
    for source in sources {
        let caps = source.capabilities();
        let aliases = if source.aliases().is_empty() {
            "-".to_string()
        } else {
            source.aliases().join(", ")
        };
        println!(
            "- {} ({}) aliases: {} | has_projects={} has_billing_blocks={} has_reasoning_tokens={} has_cache_creation={} has_cache_read={} needs_dedup={}",
            source.name(),
            source.display_name(),
            aliases,
            caps.has_projects,
            caps.has_billing_blocks,
            caps.has_reasoning_tokens,
            caps.has_cache_creation,
            caps.has_cache_read,
            caps.needs_dedup
        );
    }
    println!("Hint: use `--source <name|alias>` (e.g. `--source codex` or `--source cx`).");
}
