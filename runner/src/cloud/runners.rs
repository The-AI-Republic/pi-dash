//! Runner CRUD over the connection-bearer auth.
//!
//! All routes here authenticate with
//! ``Authorization: Bearer <connection_secret>`` + ``X-Connection-Id`` and
//! are scoped to one connection.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RunnerCrudError {
    /// The cloud rejected the request because ``runner_name`` already
    /// exists under this connection's pod. Auto-name retry path.
    #[error("runner name already taken")]
    NameTaken,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterRunnerRequest {
    pub runner_id: Uuid,
    pub name: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pod: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRunnerResponse {
    pub runner_id: Uuid,
    pub pod_id: Uuid,
    pub project_identifier: String,
}

/// ``POST /api/v1/runner/connections/<id>/runners/`` — register a runner.
pub async fn register_runner(
    cloud_url: &str,
    connection_id: &Uuid,
    connection_secret: &str,
    req: &RegisterRunnerRequest,
) -> std::result::Result<RegisterRunnerResponse, RunnerCrudError> {
    let url = format!(
        "{}/api/v1/runner/connections/{}/runners/",
        cloud_url.trim_end_matches('/'),
        connection_id
    );
    let resp = http_client()
        .map_err(RunnerCrudError::Other)?
        .post(&url)
        .bearer_auth(connection_secret)
        .header("X-Connection-Id", connection_id.to_string())
        .json(req)
        .send()
        .await
        .with_context(|| format!("POST {url}"))
        .map_err(RunnerCrudError::Other)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(classify(status, &body));
    }
    resp.json::<RegisterRunnerResponse>()
        .await
        .context("parsing register-runner response")
        .map_err(RunnerCrudError::Other)
}

/// ``DELETE /api/v1/runner/connections/<id>/runners/<rid>/``.
pub async fn delete_runner(
    cloud_url: &str,
    connection_id: &Uuid,
    connection_secret: &str,
    runner_id: &Uuid,
) -> Result<()> {
    let url = format!(
        "{}/api/v1/runner/connections/{}/runners/{}/",
        cloud_url.trim_end_matches('/'),
        connection_id,
        runner_id
    );
    let resp = http_client()?
        .delete(&url)
        .bearer_auth(connection_secret)
        .header("X-Connection-Id", connection_id.to_string())
        .send()
        .await
        .with_context(|| format!("DELETE {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("delete-runner failed: HTTP {status}: {body}");
    }
    Ok(())
}

fn classify(status: reqwest::StatusCode, body: &str) -> RunnerCrudError {
    if status == reqwest::StatusCode::CONFLICT
        && let Ok(parsed) = serde_json::from_str::<ErrorBody>(body)
        && parsed.error.as_deref() == Some("runner_name_taken")
    {
        return RunnerCrudError::NameTaken;
    }
    RunnerCrudError::Other(anyhow::anyhow!(
        "register-runner failed: HTTP {status}: {body}"
    ))
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
    fn classify_409_runner_name_taken_maps_to_name_taken() {
        let err = classify(
            reqwest::StatusCode::CONFLICT,
            r#"{"error": "runner_name_taken"}"#,
        );
        assert!(matches!(err, RunnerCrudError::NameTaken));
    }

    #[test]
    fn classify_other_409_maps_to_other() {
        let err = classify(
            reqwest::StatusCode::CONFLICT,
            r#"{"error": "connection at capacity"}"#,
        );
        assert!(matches!(err, RunnerCrudError::Other(_)));
    }
}
