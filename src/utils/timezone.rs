use chrono::offset::Offset;
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, TimeZone, Timelike, Utc};
use tz::TimeZoneRef;
use tz::datetime::FoundDateTimeKind;

use crate::error::AppError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Timezone {
    Local,
    Named(TimeZoneRef<'static>),
}

impl Timezone {
    pub(crate) const UTC: Self = Self::Named(tzdb::time_zone::UTC);

    pub(crate) fn parse(value: Option<&str>) -> Result<Self, AppError> {
        let Some(raw) = value else {
            return Ok(Timezone::Local);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("local") {
            return Ok(Timezone::Local);
        }
        if trimmed.eq_ignore_ascii_case("utc") || trimmed.eq_ignore_ascii_case("z") {
            return Ok(Self::UTC);
        }
        tzdb::tz_by_name(trimmed)
            .map(Timezone::Named)
            .ok_or_else(|| AppError::InvalidTimezone {
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
                let offset = tz
                    .find_local_time_type(utc.timestamp())
                    .expect("bundled IANA zone covers the supported timestamp range")
                    .ut_offset();
                utc.with_timezone(&FixedOffset::east_opt(offset).expect("valid IANA UTC offset"))
            }
        }
    }

    pub(crate) fn date_start_utc_millis(self, date: NaiveDate) -> Option<i64> {
        self.local_datetime_utc_millis(date.and_hms_opt(0, 0, 0)?)
    }

    pub(crate) fn local_datetime_utc_millis(self, local: chrono::NaiveDateTime) -> Option<i64> {
        match self {
            Timezone::Local => Local
                .from_local_datetime(&local)
                .earliest()
                .map(|value| value.timestamp_millis()),
            Timezone::Named(tz) => {
                let candidates = tz::DateTime::find(
                    local.year(),
                    local.month() as u8,
                    local.day() as u8,
                    local.hour() as u8,
                    local.minute() as u8,
                    local.second() as u8,
                    0,
                    tz,
                )
                .ok()?;
                // Keep the earliest valid occurrence of a repeated wall time,
                // and reject times skipped by a forward transition.
                let seconds =
                    candidates
                        .into_inner()
                        .into_iter()
                        .find_map(|candidate| match candidate {
                            FoundDateTimeKind::Normal(datetime) => Some(datetime.unix_time()),
                            FoundDateTimeKind::Skipped { .. } => None,
                        })?;
                Some(DateTime::from_timestamp(seconds, local.nanosecond())?.timestamp_millis())
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
        for alias in ["utc", "UTC", "z", "Z"] {
            let tz = Timezone::parse(Some(alias)).unwrap();
            assert!(matches!(tz, Timezone::Named(zone) if zone == tzdb::time_zone::UTC));
        }
    }

    #[test]
    fn parse_named_timezone() {
        let tz = Timezone::parse(Some("America/New_York")).unwrap();
        assert!(matches!(
            tz,
            Timezone::Named(tzdb::time_zone::america::NEW_YORK)
        ));
    }

    #[test]
    fn parse_asia_timezone() {
        let tz = Timezone::parse(Some("Asia/Shanghai")).unwrap();
        assert!(matches!(
            tz,
            Timezone::Named(tzdb::time_zone::asia::SHANGHAI)
        ));
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
        let tz = Timezone::parse(Some("  UTC  ")).unwrap();
        assert!(matches!(tz, Timezone::Named(zone) if zone == tzdb::time_zone::UTC));
    }

    #[test]
    fn to_fixed_offset_utc_preserves_time() {
        let utc = "2026-02-12T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let tz = Timezone::UTC;
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

    #[test]
    fn named_wall_times_reject_dst_gaps_and_choose_the_earlier_fold() {
        let new_york = Timezone::parse(Some("America/New_York")).unwrap();
        let missing = "2026-03-08T02:30:00.125".parse().unwrap();
        let repeated = "2026-11-01T01:30:00.125".parse().unwrap();
        let earlier = "2026-11-01T05:30:00.125Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(new_york.local_datetime_utc_millis(missing), None);
        assert_eq!(
            new_york.local_datetime_utc_millis(repeated),
            Some(earlier.timestamp_millis())
        );

        let apia = Timezone::parse(Some("Pacific/Apia")).unwrap();
        assert_eq!(
            apia.date_start_utc_millis("2011-12-30".parse().unwrap()),
            None
        );
    }

    #[test]
    fn bundled_zones_cover_the_existing_timestamp_range() {
        for &name in tzdb::TZ_NAMES {
            let zone = tzdb::tz_by_name(name).unwrap();
            for utc in [DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MAX_UTC] {
                let offset = zone
                    .find_local_time_type(utc.timestamp())
                    .unwrap_or_else(|error| panic!("{name} cannot resolve {utc}: {error}"))
                    .ut_offset();
                assert!(FixedOffset::east_opt(offset).is_some(), "{name}: {offset}");
            }
        }
    }
}
