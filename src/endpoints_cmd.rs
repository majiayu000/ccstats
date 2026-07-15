//! Handler for the `endpoints` subcommand (native vs proxy breakdown).
//!
//! Lives in its own module to keep `app.rs` under the module size limit.

use crate::app::{CommandContext, print_json, print_no_data_hint};
use crate::output::{
    EndpointTableOptions, OutputFormat, output_endpoint_json, print_endpoint_table,
};
use crate::source::{Source, load_endpoints};

pub(crate) fn handle_endpoints(source: &dyn Source, ctx: &CommandContext<'_>) {
    if !source.capabilities().has_endpoints {
        println!(
            "{} does not support endpoint breakdown.\nHint: switch with `--source claude` (or alias `--source cc`).",
            source.display_name()
        );
        return;
    }

    let endpoints = load_endpoints(source, ctx.filter, ctx.timezone);
    if endpoints.is_empty() {
        print_no_data_hint(source.display_name(), "endpoint");
        return;
    }

    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            eprintln!("CSV output is not available for the endpoints view; use --json.");
        }
        OutputFormat::Json => {
            let json = output_endpoint_json(
                &endpoints,
                ctx.pricing_db,
                ctx.cli.show_cost(),
                ctx.currency,
            );
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => print_endpoint_table(
            &endpoints,
            ctx.pricing_db,
            EndpointTableOptions {
                use_color: ctx.cli.use_color(),
                show_cost: ctx.cli.show_cost(),
                source_label: source.display_name(),
                number_format: ctx.number_format,
                currency: ctx.currency,
            },
        ),
    }
}
