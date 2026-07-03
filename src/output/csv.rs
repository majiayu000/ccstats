use std::collections::HashMap;
use std::fmt::Write;

use super::session::compare_session_last_timestamp;
use crate::cli::SortOrder;
use crate::core::{BlockStats, DataQuality, DayStats, ProjectStats, SessionStats};
use crate::output::budget::{MonthlyBudgetOptions, MonthlyBudgetReport, monthly_budget_reports};
use crate::output::format::{compare_cost, csv_escape};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::output::pricing_meta;
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, PricingSource, calculate_display_cost,
    calculate_estimated_proxy_cost, model_cost_kind, pricing_source_for_model_maps,
    sum_display_model_costs, sum_estimated_proxy_model_costs, sum_model_costs,
};

#[cfg(test)]
pub(crate) fn output_period_csv(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    output_period_csv_with_quality(
        day_stats,
        period,
        pricing_db,
        order,
        breakdown,
        show_cost,
        currency,
        None,
        CostDisplayMode::Total,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn output_period_csv_with_quality(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
    data_quality: Option<DataQuality>,
    cost_mode: CostDisplayMode,
) -> String {
    let aggregated;
    let stats_ref = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };

    let mut rows: Vec<_> = stats_ref.iter().collect();
    match order {
        SortOrder::Asc => rows.sort_by(|a, b| a.0.cmp(b.0)),
        SortOrder::Desc => rows.sort_by(|a, b| b.0.cmp(a.0)),
    }
    let label = period.label();
    let mut out = String::new();
    let ctx = PeriodCsvContext {
        label,
        pricing_db,
        show_cost,
        currency,
        include_cost_kind: period_csv_includes_cost_kind(&rows, breakdown, show_cost),
        pricing_source: pricing_source_for_model_maps(
            rows.iter().map(|(_, stats)| &stats.models),
            pricing_db,
        ),
        cost_mode,
    };
    if breakdown {
        write_period_csv_breakdown(&mut out, &rows, &ctx);
    } else {
        write_period_csv_standard(&mut out, &rows, &ctx);
    }

    append_data_quality_csv_comment(&mut out, data_quality);
    out
}

struct PeriodCsvContext<'a> {
    label: &'a str,
    pricing_db: &'a PricingDb,
    show_cost: bool,
    currency: Option<&'a CurrencyConverter>,
    include_cost_kind: bool,
    pricing_source: crate::pricing::PricingSource,
    cost_mode: CostDisplayMode,
}

fn period_csv_includes_cost_kind(
    rows: &[(&String, &DayStats)],
    breakdown: bool,
    show_cost: bool,
) -> bool {
    show_cost
        && rows.iter().any(|(_, stats)| {
            if breakdown {
                stats
                    .models
                    .values()
                    .any(|model| model.cost_kind().as_str() != "real")
            } else {
                model_cost_kind(&stats.models).as_str() != "real"
            }
        })
}

fn write_period_cost_header(out: &mut String, ctx: &PeriodCsvContext<'_>) {
    if ctx.show_cost {
        let _ = write!(out, ",cost");
        if ctx.include_cost_kind {
            let _ = write!(out, ",cost_kind,estimated_cost");
        }
        pricing_meta::append_source_csv_header(out, ctx.pricing_source, ctx.pricing_db);
    }
    out.push('\n');
}

fn write_period_csv_breakdown(
    out: &mut String,
    rows: &[(&String, &DayStats)],
    ctx: &PeriodCsvContext<'_>,
) {
    let _ = write!(
        out,
        "{},model,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens",
        ctx.label
    );
    write_period_cost_header(out, ctx);

    for &(key, stats) in rows {
        let mut models: Vec<_> = stats.models.iter().collect();
        models.sort_by_key(|(name, _)| name.as_str());
        for (model, model_stats) in &models {
            let _ = write!(
                out,
                "{},{},{},{},{},{},{},{}",
                csv_escape(key),
                csv_escape(model),
                model_stats.input_tokens,
                model_stats.output_tokens,
                model_stats.reasoning_tokens,
                model_stats.cache_creation,
                model_stats.cache_read,
                model_stats.total_tokens(),
            );
            if ctx.show_cost {
                let cost =
                    calculate_display_cost(model_stats, model, ctx.pricing_db, ctx.cost_mode);
                let _ = write!(out, ",{}", csv_cost(cost, ctx.currency));
                if ctx.include_cost_kind {
                    let estimated_cost =
                        calculate_estimated_proxy_cost(model_stats, model, ctx.pricing_db);
                    let _ = write!(
                        out,
                        ",{},{}",
                        model_stats.cost_kind().as_str(),
                        csv_cost(estimated_cost, ctx.currency)
                    );
                }
                pricing_meta::append_model_csv_fields(
                    out,
                    model,
                    ctx.pricing_db,
                    pricing_meta::csv_has_cache_fields(ctx.pricing_source, ctx.pricing_db),
                );
            }
            out.push('\n');
        }
    }
}

fn write_period_csv_standard(
    out: &mut String,
    rows: &[(&String, &DayStats)],
    ctx: &PeriodCsvContext<'_>,
) {
    let _ = write!(
        out,
        "{},input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens",
        ctx.label
    );
    write_period_cost_header(out, ctx);

    for &(key, stats) in rows {
        let _ = write!(
            out,
            "{},{},{},{},{},{},{}",
            csv_escape(key),
            stats.stats.input_tokens,
            stats.stats.output_tokens,
            stats.stats.reasoning_tokens,
            stats.stats.cache_creation,
            stats.stats.cache_read,
            stats.stats.total_tokens(),
        );
        if ctx.show_cost {
            let cost = sum_display_model_costs(&stats.models, ctx.pricing_db, ctx.cost_mode);
            let _ = write!(out, ",{}", csv_cost(cost, ctx.currency));
            if ctx.include_cost_kind {
                let estimated_cost = sum_estimated_proxy_model_costs(&stats.models, ctx.pricing_db);
                let _ = write!(
                    out,
                    ",{},{}",
                    model_cost_kind(&stats.models).as_str(),
                    csv_cost(estimated_cost, ctx.currency)
                );
            }
            pricing_meta::append_csv_fields(
                out,
                &stats.models,
                ctx.pricing_db,
                pricing_meta::csv_has_cache_fields(ctx.pricing_source, ctx.pricing_db),
            );
        }
        out.push('\n');
    }
}

pub(crate) fn append_data_quality_csv_comment(out: &mut String, data_quality: Option<DataQuality>) {
    let Some(data_quality) = data_quality else {
        return;
    };
    if !data_quality.has_warnings() {
        return;
    }

    let _ = writeln!(
        out,
        "# data_quality,valid_entries,dedup_skipped_entries,parse_errors"
    );
    let _ = writeln!(
        out,
        "# data_quality,{},{},{}",
        data_quality.valid_entries, data_quality.dedup_skipped_entries, data_quality.parse_errors
    );
}

fn csv_float(value: f64) -> String {
    if value.is_nan() {
        "N/A".to_string()
    } else {
        format!("{value:.6}")
    }
}

fn csv_cost(usd: f64, currency: Option<&CurrencyConverter>) -> String {
    let amount = currency.map_or(usd, |conv| conv.convert(usd));
    csv_float(amount)
}

fn write_budget_header(out: &mut String) {
    let _ = write!(
        out,
        ",budget_limit,budget_spent,budget_projected,budget_remaining,budget_used_pct,budget_projected_pct,budget_status,budget_days_elapsed,budget_days_in_month"
    );
}

fn write_budget_fields(out: &mut String, report: &MonthlyBudgetReport) {
    let _ = write!(
        out,
        ",{},{},{},{},{},{},{},{},{}",
        csv_float(report.limit),
        csv_float(report.spent),
        csv_float(report.projected),
        csv_float(report.remaining),
        csv_float(report.used_pct),
        csv_float(report.projected_pct),
        report.status,
        report.days_elapsed,
        report.days_in_month
    );
}

fn budget_csv_pricing_source(
    reports: &[MonthlyBudgetReport],
    pricing_db: &PricingDb,
) -> PricingSource {
    reports
        .iter()
        .map(|report| report.pricing_source)
        .reduce(PricingSource::combine)
        .unwrap_or_else(|| pricing_db.source())
}

pub(crate) fn output_monthly_budget_csv(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    options: MonthlyBudgetOptions<'_>,
) -> String {
    let monthly = aggregate_day_stats_by_period(day_stats, Period::Month);
    let reports = monthly_budget_reports(
        day_stats,
        pricing_db,
        options.order,
        options.limit,
        options.as_of,
        options.currency,
        options.cost_mode,
    );
    let mut out = String::new();
    let pricing_source = budget_csv_pricing_source(&reports, pricing_db);
    let include_pricing_cache_fields =
        pricing_meta::csv_has_cache_fields(pricing_source, pricing_db);

    if options.breakdown {
        let _ = write!(
            out,
            "month,model,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        if options.show_cost {
            let _ = write!(out, ",cost");
            pricing_meta::append_source_csv_header(&mut out, pricing_source, pricing_db);
        }
        write_budget_header(&mut out);
        out.push('\n');

        for report in &reports {
            let Some(stats) = monthly.get(&report.month) else {
                continue;
            };
            let mut models: Vec<_> = stats.models.iter().collect();
            models.sort_by_key(|(name, _)| name.as_str());
            for (model, model_stats) in &models {
                let _ = write!(
                    out,
                    "{},{},{},{},{},{},{},{}",
                    csv_escape(&report.month),
                    csv_escape(model),
                    model_stats.input_tokens,
                    model_stats.output_tokens,
                    model_stats.reasoning_tokens,
                    model_stats.cache_creation,
                    model_stats.cache_read,
                    model_stats.total_tokens(),
                );
                if options.show_cost {
                    let cost =
                        calculate_display_cost(model_stats, model, pricing_db, options.cost_mode);
                    let _ = write!(out, ",{}", csv_cost(cost, options.currency));
                    pricing_meta::append_model_csv_fields(
                        &mut out,
                        model,
                        pricing_db,
                        include_pricing_cache_fields,
                    );
                }
                write_budget_fields(&mut out, report);
                out.push('\n');
            }
        }
    } else {
        let _ = write!(
            out,
            "month,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        if options.show_cost {
            let _ = write!(out, ",cost");
            pricing_meta::append_source_csv_header(&mut out, pricing_source, pricing_db);
        }
        write_budget_header(&mut out);
        out.push('\n');

        for report in &reports {
            let Some(stats) = monthly.get(&report.month) else {
                continue;
            };
            let _ = write!(
                out,
                "{},{},{},{},{},{},{}",
                csv_escape(&report.month),
                stats.stats.input_tokens,
                stats.stats.output_tokens,
                stats.stats.reasoning_tokens,
                stats.stats.cache_creation,
                stats.stats.cache_read,
                stats.stats.total_tokens(),
            );
            if options.show_cost {
                let cost = sum_display_model_costs(&stats.models, pricing_db, options.cost_mode);
                let _ = write!(out, ",{}", csv_cost(cost, options.currency));
                pricing_meta::append_csv_fields(
                    &mut out,
                    &stats.models,
                    pricing_db,
                    include_pricing_cache_fields,
                );
            }
            write_budget_fields(&mut out, report);
            out.push('\n');
        }
    }

    out
}

pub(crate) fn output_session_csv(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let mut sorted: Vec<_> = sessions.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| compare_session_last_timestamp(a, b)),
        SortOrder::Desc => sorted.sort_by(|a, b| compare_session_last_timestamp(b, a)),
    }

    let mut out = String::new();
    let include_cost_kind = show_cost
        && sorted
            .iter()
            .any(|session| sum_estimated_proxy_model_costs(&session.models, pricing_db) > 0.0);
    let pricing_source =
        pricing_source_for_model_maps(sorted.iter().map(|session| &session.models), pricing_db);
    let include_pricing_cache_fields =
        pricing_meta::csv_has_cache_fields(pricing_source, pricing_db);
    let _ = write!(
        out,
        "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
        if include_cost_kind {
            let _ = write!(out, ",cost_kind,estimated_cost");
        }
        pricing_meta::append_source_csv_header(&mut out, pricing_source, pricing_db);
    }
    out.push('\n');

    for s in &sorted {
        let _ = write!(
            out,
            "{},{},{},{},{},{},{},{},{},{}",
            csv_escape(&s.session_id),
            csv_escape(&s.project_path),
            csv_escape(&s.first_timestamp),
            csv_escape(&s.last_timestamp),
            s.stats.input_tokens,
            s.stats.output_tokens,
            s.stats.reasoning_tokens,
            s.stats.cache_creation,
            s.stats.cache_read,
            s.stats.total_tokens(),
        );
        if show_cost {
            let cost = sum_model_costs(&s.models, pricing_db);
            let _ = write!(out, ",{}", csv_cost(cost, currency));
            if include_cost_kind {
                let estimated_cost = sum_estimated_proxy_model_costs(&s.models, pricing_db);
                let _ = write!(
                    out,
                    ",{},{}",
                    model_cost_kind(&s.models).as_str(),
                    csv_cost(estimated_cost, currency)
                );
            }
            pricing_meta::append_csv_fields(
                &mut out,
                &s.models,
                pricing_db,
                include_pricing_cache_fields,
            );
        }
        out.push('\n');
    }

    out
}
pub(crate) fn output_project_csv(
    projects: &[ProjectStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let mut sorted: Vec<_> = projects.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| {
            compare_cost(
                sum_model_costs(&a.models, pricing_db),
                sum_model_costs(&b.models, pricing_db),
            )
        }),
        SortOrder::Desc => sorted.sort_by(|a, b| {
            compare_cost(
                sum_model_costs(&b.models, pricing_db),
                sum_model_costs(&a.models, pricing_db),
            )
        }),
    }

    let mut out = String::new();
    let include_cost_kind = show_cost
        && sorted
            .iter()
            .any(|project| sum_estimated_proxy_model_costs(&project.models, pricing_db) > 0.0);
    let pricing_source =
        pricing_source_for_model_maps(sorted.iter().map(|project| &project.models), pricing_db);
    let include_pricing_cache_fields =
        pricing_meta::csv_has_cache_fields(pricing_source, pricing_db);
    let _ = write!(
        out,
        "project_name,project_path,sessions,input_tokens,output_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
        if include_cost_kind {
            let _ = write!(out, ",cost_kind,estimated_cost");
        }
        pricing_meta::append_source_csv_header(&mut out, pricing_source, pricing_db);
    }
    out.push('\n');

    for p in &sorted {
        let _ = write!(
            out,
            "{},{},{},{},{},{}",
            csv_escape(&p.project_name),
            csv_escape(&p.project_path),
            p.session_count,
            p.stats.input_tokens,
            p.stats.output_tokens,
            p.stats.total_tokens(),
        );
        if show_cost {
            let cost = sum_model_costs(&p.models, pricing_db);
            let _ = write!(out, ",{}", csv_cost(cost, currency));
            if include_cost_kind {
                let estimated_cost = sum_estimated_proxy_model_costs(&p.models, pricing_db);
                let _ = write!(
                    out,
                    ",{},{}",
                    model_cost_kind(&p.models).as_str(),
                    csv_cost(estimated_cost, currency)
                );
            }
            pricing_meta::append_csv_fields(
                &mut out,
                &p.models,
                pricing_db,
                include_pricing_cache_fields,
            );
        }
        out.push('\n');
    }

    out
}

pub(crate) fn output_block_csv(
    blocks: &[BlockStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let mut sorted: Vec<_> = blocks.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| a.block_start.cmp(&b.block_start)),
        SortOrder::Desc => sorted.sort_by(|a, b| b.block_start.cmp(&a.block_start)),
    }

    let mut out = String::new();
    let include_cost_kind = show_cost
        && sorted
            .iter()
            .any(|block| sum_estimated_proxy_model_costs(&block.models, pricing_db) > 0.0);
    let pricing_source =
        pricing_source_for_model_maps(sorted.iter().map(|block| &block.models), pricing_db);
    let include_pricing_cache_fields =
        pricing_meta::csv_has_cache_fields(pricing_source, pricing_db);
    let _ = write!(
        out,
        "block_start,block_end,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
        if include_cost_kind {
            let _ = write!(out, ",cost_kind,estimated_cost");
        }
        pricing_meta::append_source_csv_header(&mut out, pricing_source, pricing_db);
    }
    out.push('\n');

    for b in &sorted {
        let _ = write!(
            out,
            "{},{},{},{},{},{},{}",
            csv_escape(&b.block_start),
            csv_escape(&b.block_end),
            b.stats.input_tokens,
            b.stats.output_tokens,
            b.stats.cache_creation,
            b.stats.cache_read,
            b.stats.total_tokens(),
        );
        if show_cost {
            let cost = sum_model_costs(&b.models, pricing_db);
            let _ = write!(out, ",{}", csv_cost(cost, currency));
            if include_cost_kind {
                let estimated_cost = sum_estimated_proxy_model_costs(&b.models, pricing_db);
                let _ = write!(
                    out,
                    ",{},{}",
                    model_cost_kind(&b.models).as_str(),
                    csv_cost(estimated_cost, currency)
                );
            }
            pricing_meta::append_csv_fields(
                &mut out,
                &b.models,
                pricing_db,
                include_pricing_cache_fields,
            );
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
#[path = "csv_tests.rs"]
mod csv_tests;
