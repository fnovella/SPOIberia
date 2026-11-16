pub mod graph;
pub mod spo;
pub mod spo_copy;
pub mod spo_folders;

use anyhow::{anyhow, Result};
use reqwest::{RequestBuilder, Response, StatusCode};
use std::time::Duration;

/// Execute a request builder closure with automatic retry on 429 Too Many Requests / 503 Service Unavailable.
pub async fn send_with_retry<F>(mut build_req: F) -> Result<Response>
where
    F: FnMut() -> RequestBuilder,
{
    let mut attempts = 0usize;
    loop {
        attempts += 1;
        let resp = build_req().send().await?;
        let status = resp.status();
        if (status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE)
            && attempts <= 4
        {
            let wait_secs = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(2u64.pow(attempts as u32));
            tokio::time::sleep(Duration::from_secs(wait_secs.min(30))).await;
            continue;
        }
        return Ok(resp);
    }
}

pub async fn check_json_response<T: serde::de::DeserializeOwned>(
    resp: Response,
    context_msg: &str,
) -> Result<T> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{} (HTTP {}): {}", context_msg, status, body));
    }
    let text = resp.text().await?;
    serde_json::from_str::<T>(&text)
        .map_err(|e| anyhow!("{}: error parseando JSON ({}): {}", context_msg, e, text))
}
