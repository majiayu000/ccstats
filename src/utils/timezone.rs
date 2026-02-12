use chrono::offset::Offset;
use chrono::{DateTime, FixedOffset, Local, Utc};
use chrono_tz::Tz;
use std::str::FromStr;

use crate::error::AppError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Timezone {
    Local,
    Named(Tz),
}

impl Timezone {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, AppError> {
        let Some(raw) = value else {
            return Ok(Timezone::Local);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("local") {
            return Ok(Timezone::Local);
        }
        if trimmed.eq_ignore_ascii_case("utc") || trimmed.eq_ignore_ascii_case("z") {
            return Ok(Timezone::Named(chrono_tz::UTC));
        }
        Tz::from_str(trimmed)
            .map(Timezone::Named)
            .map_err(|_| AppError::InvalidTimezone {
                input: trimmed.to_string(),
            })
    }

    pub(crate) fn to_fixed_offset(self, utc: DateTime<Utc>) -> DateTime<FixedOffset> {
        match self {
            Timezone::Local => {
                let local = utc.with_timezone(&Local);
                let offset = local.offset().fix();
                local.with_timezone(&offset)
            }
            Timezone::Named(tz) => {
                let local = utc.with_timezone(&tz);
                let offset = local.offset().fix();
                local.with_timezone(&offset)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_none_returns_local() {
        assert!(matches!(Timezone::parse(None).unwrap(), Timezone::Local));
    }

    #[test]
    fn parse_empty_returns_local() {
        assert!(matches!(
            Timezone::parse(Some("")).unwrap(),
            Timezone::Local
        ));
    }

    #[test]
    fn parse_local_string_returns_local() {
        assert!(matches!(
            Timezone::parse(Some("local")).unwrap(),
            Timezone::Local
        ));
        assert!(matches!(
            Timezone::parse(Some("LOCAL")).unwrap(),
            Timezone::Local
        ));
        assert!(matches!(
            Timezone::parse(Some("Local")).unwrap(),
            Timezone::Local
        ));
    }

    #[test]
    fn parse_utc_variants() {
        let tz = Timezone::parse(Some("utc")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::UTC)));

        let tz = Timezone::parse(Some("UTC")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::UTC)));

        let tz = Timezone::parse(Some("z")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::UTC)));

        let tz = Timezone::parse(Some("Z")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::UTC)));
    }

    #[test]
    fn parse_named_timezone() {
        let tz = Timezone::parse(Some("America/New_York")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::America::New_York)));
    }

    #[test]
    fn parse_asia_timezone() {
        let tz = Timezone::parse(Some("Asia/Shanghai")).unwrap();
        assert!(matches!(tz, Timezone::Named(chrono_tz::Asia::Shanghai)));
    }

    #[test]
    fn parse_invalid_timezone_returns_error() {
        let err = Timezone::parse(Some("Mars/Olympus")).unwrap_err();
        assert!(err.to_string().contains("Mars/Olympus"));
    }

    #[test]
    fn parse_whitespace_trimmed() {
        assert!(matches!(
            Timezone::parse(Some("  local  ")).unwrap(),
            Timezone::Local
        ));
        assert!(matches!(
            Timezone::parse(Some("  UTC  ")).unwrap(),
            Timezone::Named(chrono_tz::UTC)
        ));
    }

    #[test]
    fn to_fixed_offset_utc_preserves_time() {
        let utc = "2026-02-12T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let tz = Timezone::Named(chrono_tz::UTC);
        let fixed = tz.to_fixed_offset(utc);
        assert_eq!(fixed.offset().local_minus_utc(), 0);
        assert_eq!(fixed.format("%H:%M").to_string(), "10:00");
    }

    #[test]
    fn to_fixed_offset_named_shifts_time() {
        let utc = "2026-06-15T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let tz = Timezone::parse(Some("America/New_York")).unwrap();
        let fixed = tz.to_fixed_offset(utc);
        // EDT is UTC-4 in June
        assert_eq!(fixed.offset().local_minus_utc(), -4 * 3600);
        assert_eq!(fixed.format("%H:%M").to_string(), "08:00");
    }
}
