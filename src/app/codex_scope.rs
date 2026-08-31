use std::fmt::Write as _;

use crate::source::CodexScope;

pub(crate) fn annotate_json(json: &str, scope: Option<CodexScope>) -> String {
    let Some(scope) = scope else {
        return json.to_string();
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };

    match &mut value {
        serde_json::Value::Array(rows) => {
            for row in rows {
                if let serde_json::Value::Object(object) = row {
                    object.insert(
                        "codex_scope".to_string(),
                        serde_json::Value::String(scope.as_str().to_string()),
                    );
                }
            }
        }
        serde_json::Value::Object(object) => {
            object.insert(
                "codex_scope".to_string(),
                serde_json::Value::String(scope.as_str().to_string()),
            );
        }
        _ => {}
    }

    serde_json::to_string(&value).unwrap_or_else(|_| json.to_string())
}

pub(crate) fn annotate_csv(mut csv: String, scope: Option<CodexScope>) -> String {
    if let Some(scope) = scope {
        if !csv.ends_with('\n') {
            csv.push('\n');
        }
        let _ = writeln!(csv, "# codex_scope,{}", scope.as_str());
    }
    csv
}
