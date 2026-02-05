use chrono::NaiveDate;

pub(crate) fn parse_date(s: &str) -> Result<NaiveDate, String> {
    // Try YYYYMMDD
    if s.len() == 8 {
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y%m%d") {
            return Ok(d);
        }
    }
    // Try YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d);
    }
    Err(format!(
        "Invalid date \"{}\" (expected YYYYMMDD or YYYY-MM-DD)",
        s
    ))
}
