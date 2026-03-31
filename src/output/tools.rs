//! Output formatters for tool usage statistics

use comfy_table::CellAlignment;

use crate::core::ToolSummary;

use super::format::{create_styled_table, header_cell, right_cell};

/// Print tool usage as a table
pub(crate) fn print_tools_table(summary: &ToolSummary, use_color: bool) {
    if summary.tools.is_empty() {
        println!("No tool usage data found.");
        return;
    }

    let mut table = create_styled_table();
    table.set_header(vec![
        header_cell("Tool", use_color),
        header_cell("Calls", use_color),
        header_cell("%", use_color),
    ]);

    for tool in &summary.tools {
        let pct = if summary.total > 0 {
            (tool.calls as f64 / summary.total as f64) * 100.0
        } else {
            0.0
        };
        table.add_row(vec![
            comfy_table::Cell::new(&tool.name),
            right_cell(&format_calls(tool.calls), None, false),
            right_cell(&format!("{pct:.1}%"), None, false),
        ]);
    }

    // Total row
    table.add_row(vec![
        comfy_table::Cell::new("Total")
            .add_attribute(comfy_table::Attribute::Bold)
            .set_alignment(CellAlignment::Left),
        right_cell(&format_calls(summary.total), None, true),
        right_cell("100.0%", None, true),
    ]);

    println!("{table}");
}

/// Output tool usage as JSON
pub(crate) fn output_tools_json(summary: &ToolSummary) -> String {
    let tools: Vec<serde_json::Value> = summary
        .tools
        .iter()
        .map(|t| {
            let pct = if summary.total > 0 {
                (t.calls as f64 / summary.total as f64) * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "name": t.name,
                "calls": t.calls,
                "percentage": (pct * 10.0).round() / 10.0,
            })
        })
        .collect();

    serde_json::json!({
        "tools": tools,
        "total": summary.total,
    })
    .to_string()
}

/// Output tool usage as CSV
pub(crate) fn output_tools_csv(summary: &ToolSummary) -> String {
    let mut out = String::from("tool,calls,percentage\n");
    for tool in &summary.tools {
        let pct = if summary.total > 0 {
            (tool.calls as f64 / summary.total as f64) * 100.0
        } else {
            0.0
        };
        out.push_str(&format!("{},{},{:.1}\n", tool.name, tool.calls, pct));
    }
    out.push_str(&format!("Total,{},100.0\n", summary.total));
    out
}

fn format_calls(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ToolStats, ToolSummary};

    fn sample_summary() -> ToolSummary {
        ToolSummary {
            tools: vec![
                ToolStats {
                    name: "Read".to_string(),
                    calls: 100,
                },
                ToolStats {
                    name: "Bash".to_string(),
                    calls: 50,
                },
                ToolStats {
                    name: "Edit".to_string(),
                    calls: 25,
                },
            ],
            total: 175,
        }
    }

    #[test]
    fn json_output_structure() {
        let json = output_tools_json(&sample_summary());
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["total"], 175);
        let tools = val["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "Read");
        assert_eq!(tools[0]["calls"], 100);
    }

    #[test]
    fn csv_output_format() {
        let csv = output_tools_csv(&sample_summary());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "tool,calls,percentage");
        assert!(lines[1].starts_with("Read,100,"));
        assert!(lines[4].starts_with("Total,175,"));
    }

    #[test]
    fn format_calls_with_commas() {
        assert_eq!(format_calls(0), "0");
        assert_eq!(format_calls(999), "999");
        assert_eq!(format_calls(1000), "1,000");
        assert_eq!(format_calls(1_234_567), "1,234,567");
    }

    #[test]
    fn empty_summary() {
        let summary = ToolSummary::default();
        let json = output_tools_json(&summary);
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["total"], 0);
        assert!(val["tools"].as_array().unwrap().is_empty());
    }
}
