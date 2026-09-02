use std::time::Instant;

use chrono::{DateTime, Utc};

use super::{MultiCostSummary, MultiSummaryOptions, discovery_filter, resolve_ranges};
use crate::pricing::PricingDb;
use crate::sdk::{SdkError, UsageSource, build_cost_summary, load_requested_currency};
use crate::source::{CostCoverage, GrokCostReport, get_source, load_grok_daily_ranges_with_cost};
use crate::utils::Timezone;

pub(super) fn summarize_grok_cost_ranges(
    options: &MultiSummaryOptions,
) -> Result<MultiCostSummary, SdkError> {
    summarize_grok_cost_ranges_at(options, Utc::now())
}

pub(super) fn summarize_grok_cost_ranges_at(
    options: &MultiSummaryOptions,
    observed_at: DateTime<Utc>,
) -> Result<MultiCostSummary, SdkError> {
    let start = Instant::now();
    let timezone = Timezone::parse(options.timezone.as_deref())
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let today = timezone.to_fixed_offset(observed_at).date_naive();
    let resolved_ranges = resolve_ranges(&options.ranges, today)?;
    let source = get_source(UsageSource::Grok.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: UsageSource::Grok.as_str().to_string(),
    })?;
    let pricing_db = PricingDb::try_load_quiet(options.offline, options.strict_pricing)
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let currency = load_requested_currency(options.currency.as_deref(), options.offline)?;
    let currency_code = currency.as_ref().map_or_else(
        || "USD".to_string(),
        |converter| converter.currency_code().to_string(),
    );

    let filters: Vec<_> = resolved_ranges
        .iter()
        .map(|range| range.filter.clone())
        .collect();
    let results = load_grok_daily_ranges_with_cost(
        &discovery_filter(&resolved_ranges),
        &filters,
        timezone,
        true,
        false,
    );
    let summaries = resolved_ranges
        .into_iter()
        .zip(results)
        .map(|(range, (result, reports))| {
            let cost_coverage =
                CostCoverage::from_stats(result.day_stats.values().map(|day| &day.stats));
            let mut summary = build_cost_summary(
                UsageSource::Grok,
                source,
                range.range,
                range.since,
                range.until,
                &result,
                &pricing_db,
                currency.as_ref(),
                &currency_code,
                cost_coverage,
            );
            summary.grok_api_equivalent_cost = reports
                .values()
                .copied()
                .reduce(GrokCostReport::merge)
                .map(Into::into);
            summary
        })
        .collect();

    Ok(MultiCostSummary {
        source: UsageSource::Grok,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        currency: currency_code,
        generated_at: Utc::now().to_rfc3339(),
        summaries,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::sdk::UsageRange;

    fn date(value: &str) -> NaiveDate {
        value.parse().expect("valid date")
    }

    #[test]
    fn relative_ranges_reuse_one_observation_and_snapshot() {
        let root = tempfile::tempdir().expect("temp dir");
        let previous_grok_home = std::env::var_os("GROK_HOME");
        unsafe {
            std::env::set_var("GROK_HOME", root.path());
        }

        let summary = summarize_grok_cost_ranges_at(
            &MultiSummaryOptions {
                source: UsageSource::Grok,
                ranges: vec![UsageRange::Today, UsageRange::Today, UsageRange::ThisWeek],
                timezone: Some("Asia/Shanghai".to_string()),
                offline: true,
                strict_pricing: false,
                currency: None,
            },
            "2026-09-06T15:59:59Z"
                .parse()
                .expect("valid observation time"),
        )
        .expect("summarize Grok ranges");

        match previous_grok_home {
            Some(value) => unsafe { std::env::set_var("GROK_HOME", value) },
            None => unsafe { std::env::remove_var("GROK_HOME") },
        }

        assert_eq!(summary.summaries[0].since, Some(date("2026-09-06")));
        assert_eq!(summary.summaries[0].until, Some(date("2026-09-06")));
        assert_eq!(summary.summaries[1], summary.summaries[0]);
        assert_eq!(summary.summaries[2].since, Some(date("2026-08-31")));
        assert_eq!(summary.summaries[2].until, Some(date("2026-09-06")));
    }
}
