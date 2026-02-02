use chrono::{DateTime, Local, Utc};
use chrono_tz::Tz;
use num_format::{Locale, ToFormattedString};

/// Format a number with locale-aware thousand separators
pub fn format_number_locale(n: i64, locale: Option<&str>) -> String {
    let loc = match locale {
        Some("zh") | Some("zh_CN") => Locale::zh,
        Some("de") | Some("de_DE") => Locale::de,
        Some("fr") | Some("fr_FR") => Locale::fr,
        Some("ja") | Some("ja_JP") => Locale::ja,
        Some("ko") | Some("ko_KR") => Locale::ko,
        Some("es") | Some("es_ES") => Locale::es,
        Some("it") | Some("it_IT") => Locale::it,
        Some("pt") | Some("pt_BR") => Locale::pt,
        Some("ru") | Some("ru_RU") => Locale::ru,
        _ => Locale::en, // Default to English
    };
    n.to_formatted_string(&loc)
}

/// Parse timezone string to Tz, returns None if invalid
pub fn parse_timezone(tz_str: &str) -> Option<Tz> {
    tz_str.parse::<Tz>().ok()
}

/// Convert UTC datetime to specified timezone
pub fn to_timezone(dt: DateTime<Utc>, tz: Option<&str>) -> String {
    match tz.and_then(parse_timezone) {
        Some(tz) => dt.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
        None => {
            let local: DateTime<Local> = dt.into();
            local.format("%Y-%m-%d %H:%M").to_string()
        }
    }
}

/// Format date with timezone
pub fn format_date_tz(date_str: &str, tz: Option<&str>) -> String {
    // For simple date strings like "2026-02-02", just return as-is
    // Timezone mainly affects datetime display, not pure dates
    if tz.is_some() && tz != Some("local") {
        format!("{} ({})", date_str, tz.unwrap_or("local"))
    } else {
        date_str.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number_locale() {
        assert_eq!(format_number_locale(1234567, Some("en")), "1,234,567");
        assert_eq!(format_number_locale(1234567, Some("de")), "1.234.567");
        assert_eq!(format_number_locale(1234567, None), "1,234,567");
    }

    #[test]
    fn test_parse_timezone() {
        assert!(parse_timezone("Asia/Shanghai").is_some());
        assert!(parse_timezone("UTC").is_some());
        assert!(parse_timezone("America/New_York").is_some());
        assert!(parse_timezone("invalid").is_none());
    }
}
