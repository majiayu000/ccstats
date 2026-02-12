use comfy_table::{Attribute, Cell, CellAlignment, Color, Table, TableComponent};

use crate::error::AppError;

#[derive(Debug, Clone, Copy)]
pub(crate) struct NumberFormat {
    group_sep: char,
    decimal_sep: char,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            group_sep: ',',
            decimal_sep: '.',
        }
    }
}

impl NumberFormat {
    pub(crate) fn from_locale(locale: Option<&str>) -> Result<Self, AppError> {
        let Some(raw) = locale else {
            return Ok(NumberFormat::default());
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(NumberFormat::default());
        }
        let base = trimmed
            .split(['-', '_'])
            .next()
            .unwrap_or(trimmed)
            .to_ascii_lowercase();

        let format = match base.as_str() {
            "de" => NumberFormat {
                group_sep: '.',
                decimal_sep: ',',
            },
            "fr" | "ru" => NumberFormat {
                group_sep: ' ',
                decimal_sep: ',',
            },
            "en" | "zh" => NumberFormat::default(),
            _ => {
                return Err(AppError::UnsupportedLocale {
                    input: trimmed.to_string(),
                });
            }
        };

        Ok(format)
    }
}

pub(super) fn format_number(n: i64, format: NumberFormat) -> String {
    let (sign, digits) = if n < 0 {
        ("-", (-n).to_string())
    } else {
        ("", n.to_string())
    };
    let mut result = String::new();
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(format.group_sep);
        }
        result.push(c);
    }
    let formatted: String = result.chars().rev().collect();
    format!("{}{}", sign, formatted)
}

/// Format number in compact form (K, M, B suffixes)
pub(super) fn format_compact(n: i64, format: NumberFormat) -> String {
    let (sign, value) = if n < 0 { ("-", -n) } else { ("", n) };
    let (scaled, suffix) = if value >= 1_000_000_000 {
        (value as f64 / 1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "M")
    } else if value >= 1_000 {
        (value as f64 / 1_000.0, "K")
    } else {
        return format!("{}{}", sign, value);
    };
    let mut s = format!("{:.1}", scaled);
    if format.decimal_sep != '.' {
        s = s.replace('.', &format.decimal_sep.to_string());
    }
    format!("{}{}{}", sign, s, suffix)
}

pub(super) fn format_cost(cost: f64) -> String {
    if cost.is_nan() {
        "N/A".to_string()
    } else {
        format!("${:.2}", cost)
    }
}

pub(super) fn cost_json_value(cost: f64) -> serde_json::Value {
    if cost.is_nan() {
        serde_json::Value::Null
    } else {
        serde_json::json!(cost)
    }
}

pub(super) fn styled_cell(text: &str, color: Option<Color>, bold: bool) -> Cell {
    let mut cell = Cell::new(text);
    if let Some(c) = color {
        cell = cell.fg(c);
    }
    if bold {
        cell = cell.add_attribute(Attribute::Bold);
    }
    cell
}

pub(super) fn header_cell(text: &str, use_color: bool) -> Cell {
    let mut cell = Cell::new(text).add_attribute(Attribute::Bold);
    if use_color {
        cell = cell.fg(Color::Cyan);
    }
    cell
}

/// Replace the double-line header separator (╞═╪═╡) with single-line (├─┼─┤)
pub(super) fn normalize_header_separator(table: &mut Table) {
    table.set_style(TableComponent::HeaderLines, '─');
    table.set_style(TableComponent::LeftHeaderIntersection, '├');
    table.set_style(TableComponent::MiddleHeaderIntersections, '┼');
    table.set_style(TableComponent::RightHeaderIntersection, '┤');
}

pub(super) fn right_cell(text: &str, color: Option<Color>, bold: bool) -> Cell {
    let mut cell = Cell::new(text).set_alignment(CellAlignment::Right);
    if let Some(c) = color {
        cell = cell.fg(c);
    }
    if bold {
        cell = cell.add_attribute(Attribute::Bold);
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::{NumberFormat, format_compact, format_cost, format_number};

    #[test]
    fn format_number_with_commas() {
        let fmt = NumberFormat::default();
        assert_eq!(format_number(0, fmt), "0");
        assert_eq!(format_number(999, fmt), "999");
        assert_eq!(format_number(1000, fmt), "1,000");
        assert_eq!(format_number(1234567, fmt), "1,234,567");
    }

    #[test]
    fn format_compact_units() {
        let fmt = NumberFormat::default();
        assert_eq!(format_compact(0, fmt), "0");
        assert_eq!(format_compact(999, fmt), "999");
        assert_eq!(format_compact(1_000, fmt), "1.0K");
        assert_eq!(format_compact(1_500, fmt), "1.5K");
        assert_eq!(format_compact(1_000_000, fmt), "1.0M");
        assert_eq!(format_compact(2_500_000, fmt), "2.5M");
        assert_eq!(format_compact(1_000_000_000, fmt), "1.0B");
    }

    #[test]
    fn format_cost_handles_nan() {
        assert_eq!(format_cost(f64::NAN), "N/A");
        assert_eq!(format_cost(1.234), "$1.23");
    }
}
