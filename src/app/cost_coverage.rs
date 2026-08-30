use std::fmt::Write as _;

use crate::source::CostCoverage;

pub(crate) fn annotate_json(json: &str, coverage: Option<CostCoverage>) -> String {
    let Some(coverage) = coverage else {
        return json.to_string();
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    let metadata = serde_json::json!({
        "total_tokens": coverage.total_tokens,
        "priced_tokens": coverage.priced_tokens,
        "percent": coverage.percent(),
        "complete": !coverage.is_partial(),
        "cost_is_lower_bound": coverage.is_partial(),
    });
    if let serde_json::Value::Array(rows) = &mut value {
        for row in rows {
            if let serde_json::Value::Object(object) = row {
                object.insert("api_equivalent_cost_coverage".to_string(), metadata.clone());
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| json.to_string())
}

pub(crate) fn annotate_csv(mut csv: String, coverage: Option<CostCoverage>) -> String {
    if let Some(coverage) = coverage {
        if !csv.ends_with('\n') {
            csv.push('\n');
        }
        let _ = writeln!(
            csv,
            "# api_equivalent_cost_coverage,{},{},{:.2},{},{}",
            coverage.total_tokens,
            coverage.priced_tokens,
            coverage.percent(),
            !coverage.is_partial(),
            coverage.is_partial()
        );
    }
    csv
}

pub(crate) fn print_note(coverage: Option<CostCoverage>) {
    let Some(coverage) = coverage else {
        return;
    };
    if coverage.is_partial() {
        println!(
            "\n  API-equivalent cost coverage: {} / {} tokens ({:.2}%); displayed cost is a lower bound.",
            coverage.priced_tokens,
            coverage.total_tokens,
            coverage.percent()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_marks_partial_api_cost_as_lower_bound() {
        let output = annotate_json(
            r#"[{"date":"2026-08-24"}]"#,
            Some(CostCoverage {
                total_tokens: 200,
                priced_tokens: 100,
            }),
        );
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");
        let coverage = &value[0]["api_equivalent_cost_coverage"];
        assert_eq!(coverage["total_tokens"], 200);
        assert_eq!(coverage["priced_tokens"], 100);
        assert_eq!(coverage["percent"], 50.0);
        assert_eq!(coverage["complete"], false);
        assert_eq!(coverage["cost_is_lower_bound"], true);
    }
}
