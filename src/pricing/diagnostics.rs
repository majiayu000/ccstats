use super::db::{PricingDb, ResolvedPricing};
use super::resolver::resolve_pricing_known_with_key;
use super::source::PricingSource;

impl PricingDb {
    pub(crate) fn pricing_diagnostics(&self) -> Vec<String> {
        let resolved = self.resolved.borrow();
        let mut lines: Vec<_> = resolved
            .iter()
            .map(|(model, resolution)| match resolution {
                ResolvedPricing::Known { source, .. } => {
                    let matched_key = if *source == PricingSource::Fallback {
                        Some("built-in fallback".to_string())
                    } else {
                        resolve_pricing_known_with_key(model, &self.models)
                            .map(|matched| matched.matched_key)
                    };
                    diagnostic_line(model, matched_key.as_deref(), *source)
                }
                ResolvedPricing::Unknown => diagnostic_line(model, None, PricingSource::Unknown),
            })
            .collect();
        lines.sort_unstable();
        lines
    }
}

fn diagnostic_line(model: &str, matched_key: Option<&str>, source: PricingSource) -> String {
    format!(
        "Pricing: {model} -> {} ({})",
        matched_key.unwrap_or("no match"),
        source.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::ModelPricing;

    #[test]
    fn diagnostic_line_distinguishes_catalog_fallback_and_unknown() {
        assert_eq!(
            diagnostic_line("glm-5.2", Some("fireworks_ai/glm-5p2"), PricingSource::Live),
            "Pricing: glm-5.2 -> fireworks_ai/glm-5p2 (live)"
        );
        assert_eq!(
            diagnostic_line(
                "gpt-4o-mini",
                Some("built-in fallback"),
                PricingSource::Fallback,
            ),
            "Pricing: gpt-4o-mini -> built-in fallback (fallback)"
        );
        assert_eq!(
            diagnostic_line("qwen-unknown", None, PricingSource::Unknown),
            "Pricing: qwen-unknown -> no match (unknown)"
        );
    }

    #[test]
    fn pricing_db_diagnostics_are_sorted_and_include_unknown_models() {
        let mut db = PricingDb::default();
        db.source = PricingSource::Cache;
        db.insert_model_for_tests("claude-sonnet-4".to_string(), ModelPricing::default());

        assert!(db.get_pricing("sonnet-4").is_some());
        assert!(db.get_pricing("qwen-unknown").is_none());
        assert_eq!(
            db.pricing_diagnostics(),
            vec![
                "Pricing: qwen-unknown -> no match (unknown)",
                "Pricing: sonnet-4 -> claude-sonnet-4 (cache)",
            ]
        );
    }
}
