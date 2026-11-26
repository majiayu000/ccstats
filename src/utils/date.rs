use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::consts::DATE_FORMAT;
use crate::error::AppError;

pub(crate) fn parse_date(s: &str) -> Result<Date, AppError> {
    // Try YYYYMMDD
    if s.len() == 8
        && let Ok(d) = Date::strptime("%Y%m%d", s)
    {
        return Ok(d);
    }
    // Try YYYY-MM-DD
    if let Ok(d) = Date::strptime(DATE_FORMAT, s) {
        return Ok(d);
    }
    Err(AppError::InvalidDate {
        input: s.to_string(),
    })
}

/// RFC 3339 in UTC as ccstats has always written it: `+00:00`, and a fraction of
/// 3, 6 or 9 digits only when the time has one.
pub(crate) fn rfc3339(at: Timestamp) -> String {
    let civil = at.to_zoned(TimeZone::UTC).datetime();
    let nanos = civil.subsec_nanosecond();
    let fraction = match nanos {
        0 => String::new(),
        n if n % 1_000_000 == 0 => format!(".{:03}", n / 1_000_000),
        n if n % 1_000 == 0 => format!(".{:06}", n / 1_000),
        n => format!(".{n:09}"),
    };
    format!("{}{fraction}+00:00", civil.strftime("%Y-%m-%dT%H:%M:%S"))
}

/// The Unix millisecond containing the instant, including before the epoch.
pub(crate) fn epoch_millis(at: Timestamp) -> i64 {
    at.as_nanosecond().div_euclid(1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yyyymmdd_format() {
        let d = parse_date("20260212").unwrap();
        assert_eq!(d, jiff::civil::date(2026, 2, 12));
    }

    #[test]
    fn parse_yyyy_mm_dd_format() {
        let d = parse_date("2026-02-12").unwrap();
        assert_eq!(d, jiff::civil::date(2026, 2, 12));
    }

    #[test]
    fn parse_invalid_date_returns_error() {
        assert!(parse_date("not-a-date").is_err());
    }

    #[test]
    fn parse_empty_string_returns_error() {
        assert!(parse_date("").is_err());
    }

    #[test]
    fn parse_partial_date_returns_error() {
        assert!(parse_date("2026-02").is_err());
    }

    #[test]
    fn parse_8_chars_non_date_returns_error() {
        // 8 chars but not a valid date
        assert!(parse_date("abcdefgh").is_err());
    }

    #[test]
    fn parse_invalid_month_returns_error() {
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("20261301").is_err());
    }

    #[test]
    fn parse_leap_day_valid() {
        let d = parse_date("2024-02-29").unwrap();
        assert_eq!(d, jiff::civil::date(2024, 2, 29));
    }

    #[test]
    fn parse_leap_day_invalid_year() {
        assert!(parse_date("2025-02-29").is_err());
    }

    #[test]
    fn rfc3339_keeps_the_previous_utc_form() {
        let at = |s: &str| s.parse::<Timestamp>().unwrap();
        assert_eq!(
            rfc3339(at("2026-02-12T10:00:00Z")),
            "2026-02-12T10:00:00+00:00"
        );
        assert_eq!(
            rfc3339(at("2026-02-12T10:00:00.12Z")),
            "2026-02-12T10:00:00.120+00:00"
        );
        assert_eq!(
            rfc3339(at("2026-02-12T10:00:00.1234Z")),
            "2026-02-12T10:00:00.123400+00:00"
        );
        assert_eq!(
            rfc3339(at("2026-02-12T10:00:00.000000001Z")),
            "2026-02-12T10:00:00.000000001+00:00"
        );
    }

    #[test]
    fn error_contains_original_input() {
        let err = parse_date("garbage").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("garbage"));
    }
}
