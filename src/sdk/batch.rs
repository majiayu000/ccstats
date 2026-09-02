use std::time::Instant;

use chrono::{DateTime, NaiveDate, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::{
    CostSummary, SdkError, UsageRange, UsageSource, build_cost_summary, load_cli_config,
    load_requested_currency,
};
use crate::config::Config;
use crate::consts::DATE_FORMAT;
use crate::core::{DateFilter, DedupAccumulator, LoadResult, RawEntry, aggregate_daily};
use crate::pricing::PricingDb;
use crate::source::{CostCoverage, Source, get_source};
use crate::utils::Timezone;

mod grok;

use grok::summarize_grok_cost_ranges;

/// Options for [`summarize_cost_ranges`].
///
/// Use [`summarize_cost_ranges_with_cli_config`] when SDK output should follow
/// the same persisted defaults as the CLI for timezone, pricing, and currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSummaryOptions {
    /// Usage source to read.
    pub source: UsageSource,
    /// Date or exact UTC timestamp ranges, preserving result order.
    pub ranges: Vec<UsageRange>,
    /// Optional timezone name, such as `UTC` or `Asia/Shanghai`.
    pub timezone: Option<String>,
    /// Use cached pricing only.
    pub offline: bool,
    /// Return unknown model costs as `None` instead of using fallback pricing.
    pub strict_pricing: bool,
    /// Optional display currency. Returns an error if rates are unavailable.
    pub currency: Option<String>,
}

impl Default for MultiSummaryOptions {
    fn default() -> Self {
        Self {
            source: UsageSource::Claude,
            ranges: vec![UsageRange::Today],
            timezone: None,
            offline: false,
            strict_pricing: false,
            currency: None,
        }
    }
}

/// Multi-range usage and cost summary for SDK consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiCostSummary {
    pub source: UsageSource,
    pub source_name: String,
    pub display_name: String,
    pub currency: String,
    pub generated_at: String,
    pub summaries: Vec<CostSummary>,
    pub elapsed_ms: f64,
}

#[derive(Debug, Clone)]
struct ResolvedRange {
    range: UsageRange,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    filter: DateFilter,
}

/// Summarize multiple local token usage ranges while reusing source and pricing work.
///
/// This preserves the ordering of `options.ranges` in `summaries`. Most sources
/// scan their files once; Grok evaluates each range through the single-range
/// SDK path so request reconciliation and API-equivalent estimates stay exact.
///
/// # Errors
///
/// Returns an error when no ranges are requested, when the source or timezone is
/// invalid, or when any explicit date or timestamp range is reversed.
pub fn summarize_cost_ranges(options: MultiSummaryOptions) -> Result<MultiCostSummary, SdkError> {
    if options.source == UsageSource::Grok {
        return summarize_grok_cost_ranges(&options);
    }

    let start = Instant::now();
    let MultiSummaryOptions {
        source: usage_source,
        ranges,
        timezone,
        offline,
        strict_pricing,
        currency: requested_currency,
    } = options;

    let timezone = Timezone::parse(timezone.as_deref())
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let resolved_ranges = resolve_ranges(&ranges, today)?;

    let source = get_source(usage_source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: usage_source.as_str().to_string(),
    })?;
    let pricing_db = PricingDb::try_load_quiet(offline, strict_pricing)
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let currency = load_requested_currency(requested_currency.as_deref(), offline)?;
    let currency_code = currency.as_ref().map_or_else(
        || "USD".to_string(),
        |conv| conv.currency_code().to_string(),
    );

    let results = load_daily_ranges(source, &resolved_ranges, timezone);
    let summaries = resolved_ranges
        .into_iter()
        .zip(results.iter())
        .map(|(range, result)| {
            let cost_coverage =
                CostCoverage::from_stats(result.day_stats.values().map(|day| &day.stats));
            build_cost_summary(
                usage_source,
                source,
                range.range,
                range.since,
                range.until,
                result,
                &pricing_db,
                currency.as_ref(),
                &currency_code,
                cost_coverage,
            )
        })
        .collect();

    Ok(MultiCostSummary {
        source: usage_source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        currency: currency_code,
        generated_at: Utc::now().to_rfc3339(),
        summaries,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Summarize multiple local token usage ranges using CLI-aligned config defaults.
///
/// This preserves the explicit SDK source and ranges, then fills unset timezone
/// and currency from config and applies config-enabled pricing flags.
///
/// # Errors
///
/// Returns an error when no ranges are requested, when the resolved source or
/// timezone is invalid, or when any explicit date range has `since` after
/// `until`, or when an exact timestamp range is reversed.
pub fn summarize_cost_ranges_with_cli_config(
    options: MultiSummaryOptions,
) -> Result<MultiCostSummary, SdkError> {
    let config = load_cli_config()?;
    summarize_cost_ranges(apply_cli_config_multi(options, &config))
}

fn apply_cli_config_multi(
    mut options: MultiSummaryOptions,
    config: &Config,
) -> MultiSummaryOptions {
    if !options.offline && config.offline {
        options.offline = true;
    }
    if !options.strict_pricing && config.strict_pricing {
        options.strict_pricing = true;
    }
    if options.timezone.is_none() {
        options.timezone.clone_from(&config.timezone);
    }
    if options.currency.is_none() {
        options.currency.clone_from(&config.currency);
    }

    options
}

fn resolve_ranges(ranges: &[UsageRange], today: NaiveDate) -> Result<Vec<ResolvedRange>, SdkError> {
    if ranges.is_empty() {
        return Err(SdkError::Configuration(
            "at least one usage range is required".to_string(),
        ));
    }

    ranges
        .iter()
        .map(|range| {
            let (since, until) = range.resolve(today)?;
            let mut filter = DateFilter::new(since, until);
            if let Some((since_timestamp, until_timestamp)) = range.timestamp_bounds() {
                filter = filter.with_exact_timestamp_range(since_timestamp, until_timestamp);
            }
            Ok(ResolvedRange {
                range: range.clone(),
                since,
                until,
                filter,
            })
        })
        .collect()
}

fn contains_any_range(entry: &RawEntry, date: NaiveDate, ranges: &[ResolvedRange]) -> bool {
    ranges.iter().any(|range| {
        if range.filter.has_timestamp_range() {
            range
                .filter
                .contains_entry_timestamp(&entry.timestamp, entry.timestamp_ms)
        } else {
            range.filter.contains(date)
        }
    })
}

fn load_daily_ranges(
    source: &dyn Source,
    ranges: &[ResolvedRange],
    timezone: Timezone,
) -> Vec<LoadResult> {
    let start = Instant::now();
    let discovery_filter = discovery_filter(ranges);
    let files = source.find_files_for_filter(&discovery_filter, timezone);
    if files.is_empty() {
        return ranges.iter().map(|_| LoadResult::default()).collect();
    }
    let needs_timestamp = ranges
        .iter()
        .any(|range| range.filter.has_timestamp_range());

    let (entries, parse_errors) = files
        .par_iter()
        .map(|path| {
            let parsed = source.parse_file(path, timezone, false);
            let entries = parsed
                .entries
                .into_iter()
                .filter_map(|mut entry| {
                    if needs_timestamp && entry.timestamp_ms == 0 {
                        entry.timestamp_ms = entry
                            .timestamp
                            .parse::<DateTime<Utc>>()
                            .ok()?
                            .timestamp_millis();
                    }
                    let date = normalize_entry_date(&mut entry, timezone)?;
                    contains_any_range(&entry, date, ranges).then_some(entry)
                })
                .collect::<Vec<_>>();
            (entries, parsed.errors)
        })
        .reduce(
            || (Vec::new(), 0usize),
            |(mut entries, errors), (partial_entries, partial_errors)| {
                entries.extend(partial_entries);
                (entries, errors + partial_errors)
            },
        );

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    ranges
        .iter()
        .map(|range| {
            let mut result = aggregate_entries_for_filter(&entries, &range.filter, source);
            result.parse_errors = parse_errors;
            result.elapsed_ms = elapsed_ms;
            result
        })
        .collect()
}

fn normalize_entry_date(entry: &mut RawEntry, timezone: Timezone) -> Option<NaiveDate> {
    if let Ok(date) = NaiveDate::parse_from_str(&entry.date_str, DATE_FORMAT) {
        return Some(date);
    }

    let utc_dt = entry.timestamp.parse::<DateTime<Utc>>().ok()?;
    let date = timezone.to_fixed_offset(utc_dt).date_naive();
    entry.date_str = date.format(DATE_FORMAT).to_string();
    entry.timestamp_ms = utc_dt.timestamp_millis();
    Some(date)
}

fn aggregate_entries_for_filter(
    entries: &[RawEntry],
    filter: &DateFilter,
    source: &dyn Source,
) -> LoadResult {
    let filtered: Vec<_> = entries
        .iter()
        .filter(|entry| {
            if filter.has_timestamp_range() {
                filter.contains_entry_timestamp(&entry.timestamp, entry.timestamp_ms)
            } else {
                NaiveDate::parse_from_str(&entry.date_str, DATE_FORMAT)
                    .is_ok_and(|date| filter.contains(date))
            }
        })
        .cloned()
        .collect();

    if filtered.is_empty() {
        return LoadResult::default();
    }

    if source.capabilities().needs_dedup {
        let mut accumulator = DedupAccumulator::new();
        accumulator.extend(filtered);
        let (deduped, skipped) = accumulator.finalize();
        return load_result_from_entries(source.finalize_entries(deduped), skipped);
    }

    load_result_from_entries(filtered, 0)
}

fn discovery_filter(ranges: &[ResolvedRange]) -> DateFilter {
    let timestamp_range_count = ranges
        .iter()
        .filter(|range| range.filter.has_timestamp_range())
        .count();
    if timestamp_range_count > 0 && timestamp_range_count < ranges.len() {
        return DateFilter::default();
    }

    let since = ranges
        .iter()
        .all(|range| range.since.is_some())
        .then(|| ranges.iter().filter_map(|range| range.since).min())
        .flatten();
    let until = ranges
        .iter()
        .all(|range| range.until.is_some())
        .then(|| ranges.iter().filter_map(|range| range.until).max())
        .flatten();
    let mut filter = DateFilter::new(since, until);
    if ranges
        .iter()
        .all(|range| range.filter.has_timestamp_range())
        && let (Some(since), Some(until)) = (
            ranges
                .iter()
                .filter_map(|range| range.range.timestamp_bounds().map(|bounds| bounds.0))
                .min(),
            ranges
                .iter()
                .filter_map(|range| range.range.timestamp_bounds().map(|bounds| bounds.1))
                .max(),
        )
    {
        filter = filter.with_exact_timestamp_range(since, until);
    }
    filter
}

fn load_result_from_entries(entries: Vec<RawEntry>, skipped: i64) -> LoadResult {
    let valid = entries.len() as i64;
    LoadResult {
        day_stats: aggregate_daily(entries),
        skipped,
        valid,
        parse_errors: 0,
        elapsed_ms: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    use crate::core::RawEntry;
    use crate::source::{Capabilities, ParseOutput};

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn entry_at(timestamp: &str) -> RawEntry {
        let timestamp_ms = timestamp
            .parse::<DateTime<Utc>>()
            .expect("valid timestamp")
            .timestamp_millis();
        RawEntry {
            timestamp: timestamp.to_string(),
            timestamp_ms,
            date_str: "2026-08-21".to_string(),
            message_id: Some(timestamp.to_string()),
            session_key: "session".to_string(),
            session_id: "session".to_string(),
            project_path: String::new(),
            model: "grok-4.6".to_string(),
            input_tokens: 10,
            output_tokens: 0,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            reported_total_tokens: None,
            stop_reason: Some("complete".to_string()),
            cost_kind: crate::core::CostKind::Real,
            endpoint: crate::core::Endpoint::Unknown,
            call_count: 1,
            recorded_cost_usd: Some(0.1),
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        }
    }

    struct CountingSource {
        files: Vec<PathBuf>,
        find_calls: AtomicUsize,
        parse_calls: AtomicUsize,
    }

    impl CountingSource {
        fn new(files: Vec<PathBuf>) -> Self {
            Self {
                files,
                find_calls: AtomicUsize::new(0),
                parse_calls: AtomicUsize::new(0),
            }
        }
    }

    impl Source for CountingSource {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn find_files(&self) -> Vec<PathBuf> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            self.files.clone()
        }

        fn parse_file(&self, _path: &Path, _timezone: Timezone, _debug: bool) -> ParseOutput {
            self.parse_calls.fetch_add(1, Ordering::SeqCst);
            ParseOutput {
                entries: vec![RawEntry {
                    timestamp: "2026-05-09T12:00:00Z".to_string(),
                    timestamp_ms: 1_778_326_400_000,
                    date_str: "2026-05-09".to_string(),
                    message_id: None,
                    session_key: "counting".to_string(),
                    session_id: "counting".to_string(),
                    project_path: String::new(),
                    model: "gpt-5".to_string(),
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_creation: 0,
                    cache_creation_1h: 0,
                    cache_read: 0,
                    reasoning_tokens: 0,
                    stop_reason: Some("complete".to_string()),
                    cost_kind: crate::core::CostKind::Real,
                    endpoint: crate::core::Endpoint::Unknown,
                    call_count: 1,
                    reported_total_tokens: None,
                    recorded_cost_usd: None,
                    api_equivalent_priced_tokens: 15,
                    api_equivalent_coverage_tokens: 15,
                }],
                errors: 0,
            }
        }
    }

    struct BoundedSource {
        unfiltered_calls: AtomicUsize,
        filtered_calls: AtomicUsize,
        since_ms: AtomicI64,
        until_ms: AtomicI64,
    }

    impl Default for BoundedSource {
        fn default() -> Self {
            Self {
                unfiltered_calls: AtomicUsize::new(0),
                filtered_calls: AtomicUsize::new(0),
                since_ms: AtomicI64::new(0),
                until_ms: AtomicI64::new(0),
            }
        }
    }

    impl Source for BoundedSource {
        fn name(&self) -> &'static str {
            "bounded"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn find_files(&self) -> Vec<PathBuf> {
            self.unfiltered_calls.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }

        fn find_files_for_filter(&self, filter: &DateFilter, _timezone: Timezone) -> Vec<PathBuf> {
            self.filtered_calls.fetch_add(1, Ordering::SeqCst);
            self.since_ms.store(
                filter.since_timestamp_ms.unwrap_or_default(),
                Ordering::SeqCst,
            );
            self.until_ms.store(
                filter.until_timestamp_ms.unwrap_or_default(),
                Ordering::SeqCst,
            );
            Vec::new()
        }

        fn parse_file(&self, _path: &Path, _timezone: Timezone, _debug: bool) -> ParseOutput {
            ParseOutput::default()
        }
    }

    #[test]
    fn batch_ranges_reject_empty_list() {
        let err = resolve_ranges(&[], d(2026, 5, 9)).expect_err("empty ranges should fail");

        assert!(err.to_string().contains("at least one usage range"));
    }

    #[test]
    fn batch_ranges_reject_reversed_date_range() {
        let ranges = vec![UsageRange::DateRange {
            since: Some(d(2026, 5, 10)),
            until: Some(d(2026, 5, 9)),
        }];

        let err = resolve_ranges(&ranges, d(2026, 5, 9)).expect_err("reversed range should fail");

        assert!(matches!(err, SdkError::InvalidDateRange { .. }));
    }

    #[test]
    fn range_membership_keeps_requested_days_and_excludes_gaps() {
        let ranges = resolve_ranges(
            &[
                UsageRange::DateRange {
                    since: Some(d(2026, 1, 10)),
                    until: Some(d(2026, 1, 10)),
                },
                UsageRange::DateRange {
                    since: Some(d(2026, 12, 10)),
                    until: Some(d(2026, 12, 10)),
                },
            ],
            d(2026, 12, 10),
        )
        .unwrap();
        let entry = entry_at("2026-08-21T05:42:00Z");

        assert!(contains_any_range(&entry, d(2026, 1, 10), &ranges));
        assert!(!contains_any_range(&entry, d(2026, 6, 1), &ranges));
        assert!(contains_any_range(&entry, d(2026, 12, 10), &ranges));
    }

    #[test]
    fn range_membership_honors_unbounded_sides() {
        let ranges = resolve_ranges(
            &[
                UsageRange::DateRange {
                    since: None,
                    until: Some(d(2026, 5, 6)),
                },
                UsageRange::DateRange {
                    since: Some(d(2026, 5, 10)),
                    until: None,
                },
            ],
            d(2026, 5, 9),
        )
        .unwrap();
        let entry = entry_at("2026-08-21T05:42:00Z");

        assert!(contains_any_range(&entry, d(2020, 1, 1), &ranges));
        assert!(!contains_any_range(&entry, d(2026, 5, 7), &ranges));
        assert!(contains_any_range(&entry, d(2026, 12, 31), &ranges));
    }

    #[test]
    fn timestamp_range_filters_entries_within_the_same_day() {
        let ranges = resolve_ranges(
            &[UsageRange::TimestampRange {
                since: "2026-08-21T05:41:00Z".parse().expect("valid since"),
                until: "2026-08-21T05:43:00Z".parse().expect("valid until"),
            }],
            d(2026, 8, 21),
        )
        .expect("resolve timestamp range");
        let entries = vec![
            entry_at("2026-08-21T05:40:00Z"),
            entry_at("2026-08-21T05:42:00Z"),
            entry_at("2026-08-21T05:44:00Z"),
        ];

        let source = CountingSource::new(Vec::new());
        let result = aggregate_entries_for_filter(&entries, &ranges[0].filter, &source);

        assert_eq!(result.valid, 1);
        assert_eq!(result.day_stats["2026-08-21"].stats.input_tokens, 10);
    }

    #[test]
    fn load_daily_ranges_discovers_and_parses_source_once() {
        let source =
            CountingSource::new(vec![PathBuf::from("one.jsonl"), PathBuf::from("two.jsonl")]);
        let ranges = resolve_ranges(
            &[
                UsageRange::DateRange {
                    since: Some(d(2026, 5, 9)),
                    until: Some(d(2026, 5, 9)),
                },
                UsageRange::DateRange {
                    since: Some(d(2026, 5, 1)),
                    until: Some(d(2026, 5, 31)),
                },
            ],
            d(2026, 5, 9),
        )
        .unwrap();

        let results = load_daily_ranges(&source, &ranges, Timezone::Named(chrono_tz::UTC));

        assert_eq!(source.find_calls.load(Ordering::SeqCst), 1);
        assert_eq!(source.parse_calls.load(Ordering::SeqCst), 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].valid, 2);
        assert_eq!(results[1].valid, 2);
        assert_eq!(
            CostCoverage::from_stats(results[0].day_stats.values().map(|day| &day.stats)),
            Some(CostCoverage {
                total_tokens: 30,
                priced_tokens: 30,
                estimated_proxy: 0,
            })
        );
    }

    #[test]
    fn timestamp_ranges_bound_shared_source_discovery() {
        let source = BoundedSource::default();
        let ranges = resolve_ranges(
            &[
                UsageRange::TimestampRange {
                    since: "2026-08-21T05:00:00Z".parse().expect("valid since"),
                    until: "2026-08-21T06:00:00Z".parse().expect("valid until"),
                },
                UsageRange::TimestampRange {
                    since: "2026-08-21T08:00:00Z".parse().expect("valid since"),
                    until: "2026-08-21T09:00:00Z".parse().expect("valid until"),
                },
            ],
            d(2026, 8, 21),
        )
        .expect("resolve ranges");

        let results = load_daily_ranges(&source, &ranges, Timezone::Named(chrono_tz::UTC));

        assert_eq!(results.len(), 2);
        assert_eq!(source.unfiltered_calls.load(Ordering::SeqCst), 0);
        assert_eq!(source.filtered_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            source.since_ms.load(Ordering::SeqCst),
            "2026-08-21T05:00:00Z"
                .parse::<DateTime<Utc>>()
                .expect("valid since")
                .timestamp_millis()
        );
        assert_eq!(
            source.until_ms.load(Ordering::SeqCst),
            "2026-08-21T09:00:00Z"
                .parse::<DateTime<Utc>>()
                .expect("valid until")
                .timestamp_millis()
        );
    }

    #[test]
    fn mixed_timestamp_and_date_ranges_leave_shared_discovery_unbounded() {
        let ranges = resolve_ranges(
            &[
                UsageRange::TimestampRange {
                    since: "2026-02-06T01:00:00Z".parse().expect("valid since"),
                    until: "2026-02-06T02:00:00Z".parse().expect("valid until"),
                },
                UsageRange::DateRange {
                    since: Some(d(2026, 2, 6)),
                    until: Some(d(2026, 2, 6)),
                },
            ],
            d(2026, 2, 6),
        )
        .expect("resolve ranges");

        let filter = discovery_filter(&ranges);

        assert_eq!(filter.since, None);
        assert_eq!(filter.until, None);
        assert!(!filter.has_timestamp_range());
        assert!(ranges[0].filter.has_timestamp_range());
    }

    #[test]
    fn cli_config_fills_multi_summary_defaults() {
        let config = Config {
            offline: true,
            strict_pricing: true,
            timezone: Some("Asia/Shanghai".to_string()),
            currency: Some("CNY".to_string()),
            ..Config::default()
        };

        let options = apply_cli_config_multi(
            MultiSummaryOptions {
                source: UsageSource::Codex,
                ranges: vec![UsageRange::Today, UsageRange::ThisWeek],
                ..MultiSummaryOptions::default()
            },
            &config,
        );

        assert_eq!(options.source, UsageSource::Codex);
        assert_eq!(
            options.ranges,
            vec![UsageRange::Today, UsageRange::ThisWeek]
        );
        assert!(options.offline);
        assert!(options.strict_pricing);
        assert_eq!(options.timezone.as_deref(), Some("Asia/Shanghai"));
        assert_eq!(options.currency.as_deref(), Some("CNY"));
    }
}
