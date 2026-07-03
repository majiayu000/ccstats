use std::{collections::HashMap, fmt::Write};

use crate::core::Stats;
use crate::pricing::{
    PricingDb, PricingSource, pricing_source_for_model_maps, pricing_source_for_models,
};

fn needs_cache_fields(source: PricingSource, pricing_db: &PricingDb) -> bool {
    matches!(
        source,
        PricingSource::Cache | PricingSource::CacheStale | PricingSource::Mixed
    ) && pricing_db.cache_age_seconds().is_some()
}

pub(super) fn add_json(
    obj: &mut serde_json::Value,
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
) {
    add_source_json(
        obj,
        pricing_source_for_models(models, pricing_db),
        pricing_db,
    );
}

pub(super) fn add_model_json(obj: &mut serde_json::Value, model: &str, pricing_db: &PricingDb) {
    let source = pricing_db
        .pricing_source_for_model(model)
        .unwrap_or_else(|| pricing_db.source());
    add_source_json(obj, source, pricing_db);
}

pub(super) fn add_json_for_maps<'a>(
    obj: &mut serde_json::Value,
    maps: impl IntoIterator<Item = &'a HashMap<String, Stats>>,
    pricing_db: &PricingDb,
) {
    add_source_json(
        obj,
        pricing_source_for_model_maps(maps, pricing_db),
        pricing_db,
    );
}

pub(super) fn add_source_json(
    obj: &mut serde_json::Value,
    source: PricingSource,
    pricing_db: &PricingDb,
) {
    obj["pricing_source"] = serde_json::json!(source.as_str());
    if needs_cache_fields(source, pricing_db) {
        if let Some(age) = pricing_db.cache_age_seconds() {
            obj["pricing_cache_age_seconds"] = serde_json::json!(age);
        }
        if let Some(mtime) = pricing_db.cache_modified_epoch_seconds() {
            obj["pricing_cache_mtime_epoch_seconds"] = serde_json::json!(mtime);
        }
    }
}

pub(super) fn csv_has_cache_fields(source: PricingSource, pricing_db: &PricingDb) -> bool {
    needs_cache_fields(source, pricing_db)
}

pub(super) fn append_source_csv_header(
    out: &mut String,
    source: PricingSource,
    pricing_db: &PricingDb,
) {
    out.push_str(",pricing_source");
    if csv_has_cache_fields(source, pricing_db) {
        out.push_str(",pricing_cache_age_seconds,pricing_cache_mtime_epoch_seconds");
    }
}

pub(super) fn append_csv_fields(
    out: &mut String,
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
    include_cache_fields: bool,
) {
    append_source_csv_fields(
        out,
        pricing_source_for_models(models, pricing_db),
        pricing_db,
        include_cache_fields,
    );
}

pub(super) fn append_model_csv_fields(
    out: &mut String,
    model: &str,
    pricing_db: &PricingDb,
    include_cache_fields: bool,
) {
    let source = pricing_db
        .pricing_source_for_model(model)
        .unwrap_or_else(|| pricing_db.source());
    append_source_csv_fields(out, source, pricing_db, include_cache_fields);
}

pub(super) fn append_source_csv_fields(
    out: &mut String,
    source: PricingSource,
    pricing_db: &PricingDb,
    include_cache_fields: bool,
) {
    out.push(',');
    out.push_str(source.as_str());
    if !include_cache_fields {
        return;
    }
    if needs_cache_fields(source, pricing_db) {
        let age = pricing_db.cache_age_seconds().unwrap_or_default();
        let mtime = pricing_db
            .cache_modified_epoch_seconds()
            .unwrap_or_default();
        let _ = write!(out, ",{age},{mtime}");
    } else {
        out.push_str(",,");
    }
}

pub(super) fn note(source: PricingSource, pricing_db: &PricingDb) -> Option<String> {
    match source {
        PricingSource::Live => None,
        PricingSource::Cache => Some(format!(
            "Pricing source: cache{}.",
            cache_age_suffix(pricing_db)
        )),
        PricingSource::CacheStale => Some(format!(
            "Pricing source: stale cache{}.",
            cache_age_suffix(pricing_db)
        )),
        PricingSource::Fallback => Some("Pricing source: fallback estimates.".to_string()),
        PricingSource::Mixed => Some(format!(
            "Pricing source: mixed{}.",
            cache_age_suffix(pricing_db)
        )),
    }
}

pub(super) fn note_for_maps<'a>(
    maps: impl IntoIterator<Item = &'a HashMap<String, Stats>>,
    pricing_db: &PricingDb,
) -> Option<String> {
    note(pricing_source_for_model_maps(maps, pricing_db), pricing_db)
}

fn cache_age_suffix(pricing_db: &PricingDb) -> String {
    let Some(age) = pricing_db.cache_age_seconds() else {
        return String::new();
    };
    let hours = age as f64 / 3600.0;
    match pricing_db.cache_modified_epoch_seconds() {
        Some(mtime) => format!(" ({hours:.1}h old, mtime {mtime})"),
        None => format!(" ({hours:.1}h old)"),
    }
}
