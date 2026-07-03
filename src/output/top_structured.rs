use std::fmt::Write;

use serde_json::json;

use crate::cli::TopDimension;
use crate::output::format::csv_escape;
use crate::output::top::{
    ShareBasis, TopRow, share_basis, share_of, sum_cost, sum_tokens, take_top,
};
use crate::pricing::{CurrencyConverter, PricingSource};

/// JSON output. Always includes share, basis, and full stats so downstream
/// tooling does not have to recompute them.
pub(crate) fn output_top_json(
    rows: &[TopRow],
    dim: TopDimension,
    limit: usize,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let limited = take_top(rows, limit);
    let total_cost = sum_cost(&limited);
    let total_tokens = sum_tokens(&limited);
    let basis = share_basis(&limited);
    let include_estimated = show_cost && limited.iter().any(|row| row.estimated_cost > 0.0);

    let entries: Vec<serde_json::Value> = limited
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let share = share_of(row, total_cost, total_tokens, basis);
            let mut obj = json!({
                "rank": idx + 1,
                "name": row.name,
                "count": row.count,
                "input_tokens": row.stats.input_tokens,
                "output_tokens": row.stats.output_tokens,
                "cache_creation": row.stats.cache_creation,
                "cache_read": row.stats.cache_read,
                "reasoning_tokens": row.stats.reasoning_tokens,
                "total_tokens": row.stats.total_tokens(),
                "share_percent": (share * 100.0).round() / 100.0,
            });
            if show_cost {
                obj["cost_usd"] = if row.cost.is_nan() {
                    serde_json::Value::Null
                } else {
                    json!((row.cost * 100_000.0).round() / 100_000.0)
                };
                add_pricing_source_json(&mut obj, row);
                if let Some(conv) = currency
                    && !row.cost.is_nan()
                {
                    obj["cost_local"] = json!(conv.format(row.cost));
                }
                if include_estimated {
                    obj["cost_kind"] = json!(row.cost_kind.as_str());
                    obj["estimated_cost_usd"] = if row.estimated_cost.is_nan() {
                        serde_json::Value::Null
                    } else {
                        json!((row.estimated_cost * 100_000.0).round() / 100_000.0)
                    };
                    if let Some(conv) = currency
                        && !row.estimated_cost.is_nan()
                    {
                        obj["estimated_cost_local"] = json!(conv.format(row.estimated_cost));
                    }
                }
            }
            obj
        })
        .collect();

    json!({
        "dimension": match dim {
            TopDimension::Model => "model",
            TopDimension::Project => "project",
        },
        "limit": limit,
        "displayed": limited.len(),
        "total_rows": rows.len(),
        "share_basis": match basis {
            ShareBasis::Cost => "cost",
            ShareBasis::Tokens => "tokens",
        },
        "entries": entries,
    })
    .to_string()
}

/// CSV output. Header columns mirror the JSON keys.
pub(crate) fn output_top_csv(
    rows: &[TopRow],
    dim: TopDimension,
    limit: usize,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let limited = take_top(rows, limit);
    let total_cost = sum_cost(&limited);
    let total_tokens = sum_tokens(&limited);
    let basis = share_basis(&limited);

    let mut out = String::new();
    let dim_col = match dim {
        TopDimension::Model => "model",
        TopDimension::Project => "project",
    };
    let _ = write!(
        out,
        "rank,{dim_col},count,input_tokens,output_tokens,cache_creation,cache_read,reasoning_tokens,total_tokens,share_percent"
    );
    if show_cost {
        out.push_str(",cost_usd");
        if currency.is_some() {
            out.push_str(",cost_local");
        }
        if limited.iter().any(|row| row.estimated_cost > 0.0) {
            out.push_str(",cost_kind,estimated_cost_usd");
            if currency.is_some() {
                out.push_str(",estimated_cost_local");
            }
        }
        append_pricing_source_csv_header(&mut out, &limited);
    }
    out.push('\n');
    let include_estimated = show_cost && limited.iter().any(|row| row.estimated_cost > 0.0);

    for (idx, row) in limited.iter().enumerate() {
        let share = share_of(row, total_cost, total_tokens, basis);
        let _ = write!(
            out,
            "{},{},{},{},{},{},{},{},{},{:.2}",
            idx + 1,
            csv_escape(&row.name),
            row.count,
            row.stats.input_tokens,
            row.stats.output_tokens,
            row.stats.cache_creation,
            row.stats.cache_read,
            row.stats.reasoning_tokens,
            row.stats.total_tokens(),
            share,
        );
        if show_cost {
            if row.cost.is_nan() {
                out.push(',');
                if currency.is_some() {
                    out.push(',');
                }
            } else {
                let _ = write!(out, ",{:.6}", row.cost);
                if let Some(conv) = currency {
                    let _ = write!(out, ",{}", csv_escape(&conv.format(row.cost)));
                }
            }
            if include_estimated {
                let _ = write!(out, ",{}", row.cost_kind.as_str());
                if row.estimated_cost.is_nan() {
                    out.push(',');
                } else {
                    let _ = write!(out, ",{:.6}", row.estimated_cost);
                }
                if let Some(conv) = currency {
                    if row.estimated_cost.is_nan() {
                        out.push(',');
                    } else {
                        let _ = write!(out, ",{}", csv_escape(&conv.format(row.estimated_cost)));
                    }
                }
            }
            append_pricing_source_csv_fields(&mut out, row, &limited);
        }
        out.push('\n');
    }
    out
}

fn add_pricing_source_json(obj: &mut serde_json::Value, row: &TopRow) {
    obj["pricing_source"] = json!(row.pricing_source.as_str());
    if needs_cache_fields(row) {
        if let Some(age) = row.pricing_cache_age_seconds {
            obj["pricing_cache_age_seconds"] = json!(age);
        }
        if let Some(mtime) = row.pricing_cache_mtime_epoch_seconds {
            obj["pricing_cache_mtime_epoch_seconds"] = json!(mtime);
        }
    }
}

fn append_pricing_source_csv_header(out: &mut String, rows: &[TopRow]) {
    out.push_str(",pricing_source");
    if rows
        .iter()
        .any(|row| row.pricing_cache_age_seconds.is_some())
    {
        out.push_str(",pricing_cache_age_seconds,pricing_cache_mtime_epoch_seconds");
    }
}

fn append_pricing_source_csv_fields(out: &mut String, row: &TopRow, rows: &[TopRow]) {
    let _ = write!(out, ",{}", row.pricing_source.as_str());
    if !rows
        .iter()
        .any(|row| row.pricing_cache_age_seconds.is_some())
    {
        return;
    }
    if needs_cache_fields(row) {
        let age = row.pricing_cache_age_seconds.unwrap_or_default();
        let mtime = row.pricing_cache_mtime_epoch_seconds.unwrap_or_default();
        let _ = write!(out, ",{age},{mtime}");
    } else {
        out.push_str(",,");
    }
}

fn needs_cache_fields(row: &TopRow) -> bool {
    matches!(
        row.pricing_source,
        PricingSource::Cache | PricingSource::CacheStale | PricingSource::Mixed
    ) && row.pricing_cache_age_seconds.is_some()
}
