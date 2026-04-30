use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Body of ``POST /api/v1/runner/connections/enroll/``.
#[derive(Debug, Clone, Serialize)]
pub struct EnrollmentRequest {
    pub token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnrollmentResponse {
    pub connection_id: Uuid,
    pub connection_secret: String,
    pub workspace_slug: String,
    pub heartbeat_interval_secs: u64,
    pub protocol_version: u32,
}

/// Exchange a one-time enrollment token for a long-lived connection
/// secret. Called once during ``pidash connect``.
pub async fn enroll(cloud_url: &str, req: &EnrollmentRequest) -> Result<EnrollmentResponse> {
    let url = format!(
        "{}/api/v1/runner/connections/enroll/",
        cloud_url.trim_end_matches('/')
    );
    let resp = http_client()?
        .post(&url)
        .json(req)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("enrollment failed: HTTP {status}: {}", truncate_body(&body));
    }
    resp.json::<EnrollmentResponse>()
        .await
        .context("parsing enroll response")
}

const MAX_ERROR_BODY_CHARS: usize = 256;

fn truncate_body(body: &str) -> String {
    let s = body.trim();
    if s.chars().count() <= MAX_ERROR_BODY_CHARS {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_ERROR_BODY_CHARS).collect();
    format!("{head}…(truncated)")
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enroll_response_parses_full_body() {
        let body = r#"{
            "connection_id": "00000000-0000-0000-0000-000000000001",
            "connection_secret": "apd_cs_x",
            "workspace_slug": "acme",
            "heartbeat_interval_secs": 25,
            "protocol_version": 3
        }"#;
        let resp: EnrollmentResponse = serde_json::from_str(body).unwrap();
        assert_eq!(resp.workspace_slug, "acme");
        assert_eq!(resp.protocol_version, 3);
    }

    #[test]
    fn truncate_body_preserves_short_strings() {
        assert_eq!(truncate_body("  invalid token  "), "invalid token");
    }
}
