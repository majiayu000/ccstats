use chrono::NaiveDate;

use crate::consts::DATE_FORMAT;
use crate::error::AppError;

pub(crate) fn parse_date(s: &str) -> Result<NaiveDate, AppError> {
    // Try YYYYMMDD
    if s.len() == 8
        && let Ok(d) = NaiveDate::parse_from_str(s, "%Y%m%d")
    {
        return Ok(d);
    }
    // Try YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(s, DATE_FORMAT) {
        return Ok(d);
    }
    Err(AppError::InvalidDate {
        input: s.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yyyymmdd_format() {
        let d = parse_date("20260212").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 2, 12).unwrap());
    }

    #[test]
    fn parse_yyyy_mm_dd_format() {
        let d = parse_date("2026-02-12").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 2, 12).unwrap());
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
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
    }

    #[test]
    fn parse_leap_day_invalid_year() {
        assert!(parse_date("2025-02-29").is_err());
    }

    #[test]
    fn error_contains_original_input() {
        let err = parse_date("garbage").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("garbage"));
    }
}
