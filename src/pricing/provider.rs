use std::collections::HashMap;

pub(super) fn fetch_litellm_raw() -> Option<HashMap<String, serde_json::Value>> {
    let url = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
    let response = ureq::get(url).call().ok()?;
    let mut body = response.into_body();
    serde_json::from_reader(body.as_reader()).ok()
}
