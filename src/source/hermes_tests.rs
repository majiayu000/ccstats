use super::*;

#[test]
fn session_residual_rejects_reasoning_component_mismatch() {
    let row = HermesRow {
        session_id: "session".to_string(),
        model: "gpt-5".to_string(),
        provider: "openai".to_string(),
        base_url: String::new(),
        billing_mode: String::new(),
        task: "<session-residual>".to_string(),
        call_count: 1,
        input: 0,
        output: 10,
        cache_read: 0,
        cache_write: 0,
        reasoning: 0,
        estimated_cost: 0.0,
        actual_cost: 0.0,
        cost_status: None,
        timestamp_seconds: 1.0,
        project_path: None,
    };
    let subtotal = UsageTotals {
        output: 10,
        reasoning: 5,
        ..UsageTotals::default()
    };

    assert!(matches!(
        residual_from_session(row, Some(&subtotal)),
        Err("session aggregate is below detail subtotal")
    ));
}
