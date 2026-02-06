use std::collections::HashMap;
use std::time::Duration;

const LITELLM_PRICING_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_RETRIES: usize = 3;
const RETRY_BACKOFF_MS: u64 = 250;

pub(super) fn fetch_litellm_raw() -> Option<HashMap<String, serde_json::Value>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into();

    for attempt in 0..FETCH_RETRIES {
        if let Ok(response) = agent.get(LITELLM_PRICING_URL).call() {
            let mut body = response.into_body();
            if let Ok(parsed) = serde_json::from_reader(body.as_reader()) {
                return Some(parsed);
            }
        }

        if attempt + 1 < FETCH_RETRIES {
            std::thread::sleep(Duration::from_millis(
                RETRY_BACKOFF_MS * (attempt as u64 + 1),
            ));
        }
    }

    None
}
