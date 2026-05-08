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

/// ``DELETE /api/v1/runners/<runner_id>/?purge_local=true|false``.
///
/// Cloud-side runner deletion via the X-Api-Key (MachineToken) auth
/// surface — the route the local CLI uses now that the connection-
/// scoped variant is gone. ``purge_local=true`` emits a
/// ``remove_runner`` control frame so the daemon cascades the
/// teardown to local config + data dir; ``false`` emits the legacy
/// ``revoke`` and leaves the local install in place.
pub async fn delete_runner(
    cloud_url: &str,
    api_token: &str,
    runner_id: &Uuid,
    purge_local: bool,
) -> Result<()> {
    let url = format!(
        "{}/api/v1/runners/{}/?purge_local={}",
        cloud_url.trim_end_matches('/'),
        runner_id,
        if purge_local { "true" } else { "false" },
    );
    let resp = http_client()?
        .delete(&url)
        .header("X-Api-Key", api_token)
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

/// Probe the cloud's `/api/v1/runner/health/` endpoint with a short
/// timeout. Used by `pidash runner remove` to decide whether to take
/// the cascade path (B1) or the local-only path (B2) when the user
/// hasn't passed `--local-only`.
///
/// "Reachable" means the cloud answered with *any* HTTP response —
/// 2xx, 4xx, or 5xx. The auth-walled / disabled / mis-deployed cases
/// (401/403/404/5xx) all imply the cloud is up, just unhappy with this
/// particular request, and the actual delete endpoint may behave
/// differently. Only network errors (DNS, refused, TLS, timeout)
/// drive us into OfflineFallback. This is the conservative posture:
/// when in doubt, attempt the cloud delete and let its error path
/// surface the real failure to the operator.
pub async fn probe_cloud_reachable(cloud_url: &str) -> bool {
    let url = format!("{}/api/v1/runner/health/", cloud_url.trim_end_matches('/'),);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    // Any HTTP response — including 4xx / 5xx — proves the cloud is
    // reachable. Only transport-layer errors (timeout, refused, DNS,
    // TLS) mean "offline".
    client.get(&url).send().await.is_ok()
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
