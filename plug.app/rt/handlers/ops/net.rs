use std::time::Duration;
use ureq;
use serde_json::Value;

/// default timeout for net request
const DEFAULT_TIMEOUT: u64 = 30;
const USER_AGENT: &str = "plug-runtime/1.0.0";

/// execute post request with json body
pub fn http_post_json(url: &str, json_payload: &str) -> Result<String, String> {
    // check input json format validity
    let body: Value = serde_json::from_str(json_payload)
        .map_err(|e| format!("Invalid JSON input: {}", e))?;

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
        .build();

    let response = agent.post(url)
        .set("User-Agent", USER_AGENT)
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("POST failed: {}", e))?;

    response.into_string()
        .map_err(|e| format!("Failed to read response body: {}", e))
}
