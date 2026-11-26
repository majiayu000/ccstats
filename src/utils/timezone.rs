use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, Offset, TimeZone};
use jiff::{Timestamp, Zoned};
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

    /// The instant at this zone's offset at that moment, as a fixed-offset time.
    pub(crate) fn to_fixed_offset(self, utc: Timestamp) -> Zoned {
        let offset = match self {
            Timezone::Local => TimeZone::system().to_offset(utc),
            Timezone::Named(tz) => {
                let seconds = tz
                    .find_local_time_type(utc.as_second())
                    .expect("bundled IANA zone covers the supported timestamp range")
                    .ut_offset();
                Offset::from_seconds(seconds).expect("valid IANA UTC offset")
            }
        };
        utc.to_zoned(TimeZone::fixed(offset))
    }

    pub(crate) fn date_start_utc_millis(self, date: Date) -> Option<i64> {
        self.local_datetime_utc_millis(date.to_datetime(jiff::civil::Time::midnight()))
    }

    pub(crate) fn local_datetime_utc_millis(self, local: DateTime) -> Option<i64> {
        match self {
            // Keep the earliest valid occurrence of a repeated wall time,
            // and reject times skipped by a forward transition.
            Timezone::Local => {
                let offset = match TimeZone::system().to_ambiguous_timestamp(local).offset() {
                    AmbiguousOffset::Unambiguous { offset } => offset,
                    AmbiguousOffset::Fold { before, .. } => before,
                    AmbiguousOffset::Gap { .. } => return None,
                };
                Some(offset.to_timestamp(local).ok()?.as_millisecond())
            }
            Timezone::Named(tz) => {
                let candidates = tz::DateTime::find(
                    i32::from(local.year()),
                    local.month() as u8,
                    local.day() as u8,
                    local.hour() as u8,
                    local.minute() as u8,
                    local.second() as u8,
                    0,
                    tz,
                )
                .ok()?;
                let seconds =
                    candidates
                        .into_inner()
                        .into_iter()
                        .find_map(|candidate| match candidate {
                            FoundDateTimeKind::Normal(datetime) => Some(datetime.unix_time()),
                            FoundDateTimeKind::Skipped { .. } => None,
                        })?;
                Some(
                    Timestamp::new(seconds, local.subsec_nanosecond())
                        .ok()?
                        .as_millisecond(),
                )
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
        let utc = "2026-02-12T10:00:00Z".parse::<Timestamp>().unwrap();
        let tz = Timezone::UTC;
        let fixed = tz.to_fixed_offset(utc);
        assert_eq!(fixed.offset().seconds(), 0);
        assert_eq!(fixed.strftime("%H:%M").to_string(), "10:00");
    }

    #[test]
    fn to_fixed_offset_named_shifts_time() {
        let utc = "2026-06-15T12:00:00Z".parse::<Timestamp>().unwrap();
        let tz = Timezone::parse(Some("America/New_York")).unwrap();
        let fixed = tz.to_fixed_offset(utc);
        // EDT is UTC-4 in June
        assert_eq!(fixed.offset().seconds(), -4 * 3600);
        assert_eq!(fixed.strftime("%H:%M").to_string(), "08:00");
    }

    #[test]
    fn named_wall_times_reject_dst_gaps_and_choose_the_earlier_fold() {
        let new_york = Timezone::parse(Some("America/New_York")).unwrap();
        let missing = "2026-03-08T02:30:00.125".parse::<DateTime>().unwrap();
        let repeated = "2026-11-01T01:30:00.125".parse::<DateTime>().unwrap();
        let earlier = "2026-11-01T05:30:00.125Z".parse::<Timestamp>().unwrap();
        assert_eq!(new_york.local_datetime_utc_millis(missing), None);
        assert_eq!(
            new_york.local_datetime_utc_millis(repeated),
            Some(earlier.as_millisecond())
        );

        let apia = Timezone::parse(Some("Pacific/Apia")).unwrap();
        assert_eq!(
            apia.date_start_utc_millis("2011-12-30".parse::<Date>().unwrap()),
            None
        );
    }

    #[test]
    fn bundled_zones_cover_the_existing_timestamp_range() {
        for &name in tzdb::TZ_NAMES {
            let zone = tzdb::tz_by_name(name).unwrap();
            for utc in [Timestamp::MIN, Timestamp::MAX] {
                let offset = zone
                    .find_local_time_type(utc.as_second())
                    .unwrap_or_else(|error| panic!("{name} cannot resolve {utc}: {error}"))
                    .ut_offset();
                assert!(Offset::from_seconds(offset).is_ok(), "{name}: {offset}");
            }
        }
    }
}
