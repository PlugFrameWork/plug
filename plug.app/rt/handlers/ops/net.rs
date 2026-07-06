use std::time::Duration;
use ureq;
use serde_json::Value;

/// default timeout for net reqests
const DEFAULT_TIMEOUT: u64 = 30;
const USER_AGENT: &str = "plug-runtime/1.0.0";

/// execute get reqest and return raw string
pub fn http_get(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
        .build();

    let response = agent.get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("GET failed: {}", e))?;

    response.into_string()
        .map_err(|e| format!("Failed to read response body: {}", e))
}

/// execute post reqest with json body
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

/// check if internet is online
pub fn is_online() -> bool {
    // quick check using google dns
    ureq::get("http://clients3.google.com/generate_204")
        .timeout(Duration::from_secs(3))
        .call()
        .is_ok()
}
