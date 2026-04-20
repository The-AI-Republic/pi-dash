use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct RegisterRequest {
    pub runner_name: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterResponse {
    pub runner_id: Uuid,
    pub runner_secret: String,
    /// Public REST API token (`X-Api-Key`) for `/api/v1/`. Optional so a
    /// daemon built against the new server can also enroll against an older
    /// server that hasn't shipped the dual-credential change yet.
    #[serde(default)]
    pub api_token: Option<String>,
    pub heartbeat_interval_secs: u64,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize)]
struct RegisterEnvelope<'a> {
    #[serde(flatten)]
    req: &'a RegisterRequest,
    token: &'a str,
}

pub async fn register(
    cloud_url: &str,
    token: &str,
    req: &RegisterRequest,
) -> Result<RegisterResponse> {
    let url = format!(
        "{}/api/v1/runner/register/",
        cloud_url.trim_end_matches('/')
    );
    let client = http_client()?;
    let resp = client
        .post(&url)
        .json(&RegisterEnvelope { req, token })
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("registration failed: HTTP {status}: {body}");
    }
    let out = resp.json::<RegisterResponse>().await?;
    Ok(out)
}

pub async fn rotate(
    cloud_url: &str,
    runner_id: &Uuid,
    runner_secret: &str,
) -> Result<RegisterResponse> {
    let url = format!(
        "{}/api/v1/runner/{}/rotate/",
        cloud_url.trim_end_matches('/'),
        runner_id
    );
    let client = http_client()?;
    let resp = client
        .post(&url)
        .bearer_auth(runner_secret)
        .json(&serde_json::json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("rotate failed: HTTP {status}: {body}");
    }
    Ok(resp.json::<RegisterResponse>().await?)
}

pub async fn deregister(
    cloud_url: &str,
    runner_id: &Uuid,
    runner_secret: &str,
    removal_token: Option<&str>,
) -> Result<()> {
    let url = format!(
        "{}/api/v1/runner/{}/deregister/",
        cloud_url.trim_end_matches('/'),
        runner_id
    );
    let client = http_client()?;
    let mut req = client.post(&url).bearer_auth(runner_secret);
    if let Some(tok) = removal_token {
        req = req.json(&serde_json::json!({ "removal_token": tok }));
    } else {
        req = req.json(&serde_json::json!({}));
    }
    let resp = req.send().await.with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("deregister failed: HTTP {status}: {body}");
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
        .build()?)
}
