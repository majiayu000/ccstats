use chrono::offset::Offset;
use chrono::{DateTime, FixedOffset, Local, Utc};
use chrono_tz::Tz;
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Timezone {
    Local,
    Named(Tz),
}

impl Timezone {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
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
            .map_err(|_| format!("Invalid timezone: {}", trimmed))
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
