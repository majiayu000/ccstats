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
