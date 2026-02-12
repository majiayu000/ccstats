use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AppError {
    #[error("Invalid date \"{input}\" (expected YYYYMMDD or YYYY-MM-DD)")]
    InvalidDate { input: String },

    #[error("Invalid timezone: {input}")]
    InvalidTimezone { input: String },

    #[error("Unsupported locale: {input}")]
    UnsupportedLocale { input: String },

    #[error("{0}")]
    Jq(#[from] JqError),
}

#[derive(Debug, Error)]
pub(crate) enum JqError {
    #[error("jq not found. Please install jq to use --jq option.")]
    NotFound,

    #[error("Failed to run jq: {0}")]
    Spawn(std::io::Error),

    #[error("Failed to write to jq stdin: {0}")]
    Stdin(std::io::Error),

    #[error("Failed to wait for jq: {0}")]
    Wait(std::io::Error),

    #[error("Invalid UTF-8 from jq: {0}")]
    Utf8(std::string::FromUtf8Error),

    #[error("jq error: {0}")]
    Filter(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_display_date() {
        let e = AppError::InvalidDate {
            input: "abc".to_string(),
        };
        assert_eq!(
            e.to_string(),
            r#"Invalid date "abc" (expected YYYYMMDD or YYYY-MM-DD)"#
        );
    }

    #[test]
    fn app_error_display_timezone() {
        let e = AppError::InvalidTimezone {
            input: "Mars/Olympus".to_string(),
        };
        assert_eq!(e.to_string(), "Invalid timezone: Mars/Olympus");
    }

    #[test]
    fn app_error_display_locale() {
        let e = AppError::UnsupportedLocale {
            input: "xx".to_string(),
        };
        assert_eq!(e.to_string(), "Unsupported locale: xx");
    }

    #[test]
    fn jq_error_not_found() {
        assert_eq!(
            JqError::NotFound.to_string(),
            "jq not found. Please install jq to use --jq option."
        );
    }

    #[test]
    fn jq_error_filter() {
        let e = JqError::Filter("parse error".to_string());
        assert_eq!(e.to_string(), "jq error: parse error");
    }

    #[test]
    fn app_error_from_jq_error() {
        let jq = JqError::Filter("bad filter".to_string());
        let app: AppError = jq.into();
        assert_eq!(app.to_string(), "jq error: bad filter");
    }
}
