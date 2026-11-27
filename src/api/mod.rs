pub mod graph;
pub mod spo;
pub mod spo_copy;
pub mod spo_folders;
pub mod spo_views;

use anyhow::{anyhow, Result};
use reqwest::{RequestBuilder, Response, StatusCode};
use std::time::Duration;
use url::Url;

/// Validate that an OData `@odata.nextLink` URL uses HTTPS and points to the same host and port as `expected_origin`.
pub fn validate_next_link(expected_origin: &str, next_link: &str) -> Result<()> {
    let expected = Url::parse(expected_origin)
        .map_err(|e| anyhow!("Origen esperado inválido '{}': {}", expected_origin, e))?;
    let next = Url::parse(next_link)
        .map_err(|e| anyhow!("URL de paginación nextLink inválida '{}': {}", next_link, e))?;
    if next.scheme() != "https" {
        return Err(anyhow!("nextLink debe usar HTTPS: '{}'", next_link));
    }
    let exp_host = expected
        .host_str()
        .ok_or_else(|| anyhow!("Origen esperado sin host: '{}'", expected_origin))?;
    let next_host = next
        .host_str()
        .ok_or_else(|| anyhow!("nextLink sin host: '{}'", next_link))?;
    if !next_host.eq_ignore_ascii_case(exp_host)
        || next.port_or_known_default() != expected.port_or_known_default()
    {
        return Err(anyhow!(
            "nextLink apunta a un host distinto ('{}' != '{}')",
            next_host,
            exp_host
        ));
    }
    Ok(())
}

/// Validate that `id` is a valid GUID (32 hex digits or 8-4-4-4-12 hyphenated, optional `{}`) to prevent OData injection.
pub fn validate_guid(id: &str) -> Result<&str> {
    let trimmed = id.trim().trim_start_matches('{').trim_end_matches('}');
    let valid_hyphenated = trimmed.len() == 36
        && trimmed.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    let valid_compact = trimmed.len() == 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid_hyphenated && !valid_compact {
        return Err(anyhow!("GUID inválido: '{}'", id));
    }
    Ok(trimmed)
}

/// Reject relative folder paths containing traversal (`..`) segments or NUL bytes.
pub fn validate_relative_folder_path(path: &str) -> Result<()> {
    if path.contains('\0') {
        return Err(anyhow!("Ruta de carpeta inválida (contiene byte NUL)"));
    }
    for seg in path.split(['/', '\\']) {
        if seg == ".." {
            return Err(anyhow!(
                "Ruta de carpeta inválida (no se permite '..'): '{}'",
                path
            ));
        }
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_next_link() {
        assert!(validate_next_link(
            "https://graph.microsoft.com/v1.0/groups",
            "https://graph.microsoft.com/v1.0/groups?$skiptoken=abc"
        )
        .is_ok());
        assert!(validate_next_link(
            "https://graph.microsoft.com/v1.0/groups",
            "https://evil.example.com/v1.0/groups"
        )
        .is_err());
        assert!(validate_next_link(
            "https://graph.microsoft.com/v1.0/groups",
            "http://graph.microsoft.com/v1.0/groups"
        )
        .is_err());
    }

    #[test]
    fn test_validate_guid() {
        assert!(validate_guid("12345678-1234-1234-1234-1234567890ab").is_ok());
        assert!(validate_guid("{12345678-1234-1234-1234-1234567890AB}").is_ok());
        assert!(validate_guid("not-a-guid' or 1 eq 1").is_err());
    }

    #[test]
    fn test_validate_relative_folder_path() {
        assert!(validate_relative_folder_path("Area1/Sub").is_ok());
        assert!(validate_relative_folder_path("../secret").is_err());
        assert!(validate_relative_folder_path("Area1/../secret").is_err());
        assert!(validate_relative_folder_path(r"Area1\..\secret").is_err());
        assert!(validate_relative_folder_path("Area1/sub\0").is_err());
    }
}
