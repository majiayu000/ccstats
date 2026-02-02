use chrono::NaiveDate;

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    // Try YYYYMMDD
    if s.len() == 8 {
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y%m%d") {
            return Some(d);
        }
    }
    // Try YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d);
    }
    None
}
