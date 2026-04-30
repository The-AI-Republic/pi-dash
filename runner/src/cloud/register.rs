use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Typed failure modes for the register endpoint so `configure` can distinguish
/// a name collision (retryable when auto-generated, loud-fail when user-supplied)
/// from a generic network / auth / cap error.
#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    /// Cloud rejected the request because `runner_name` already exists in the
    /// target workspace. Triggered by `UNIQUE(workspace_id, name)` on the cloud
    /// side and surfaced as `409 {"error": "runner_name_taken"}`.
    #[error("runner name already taken in this workspace")]
    NameTaken,
    /// Anything else — transport failure, HTTP non-200 other than the specific
    /// 409 shape above, malformed response body, etc. Keeps the anyhow context
    /// chain intact for debugging.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterRequest {
    pub runner_name: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    pub protocol_version: u32,
    /// Project identifier the runner is registering against. Required
    /// post-refactor — the runner is bound to one project for its
    /// lifetime. See
    /// `.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md` §7.
    pub project: String,
    /// Optional pod name within the project. Defaults to the project's
    /// auto-created default pod when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterResponse {
    pub runner_id: Uuid,
    pub runner_secret: String,
    /// Workspace slug the runner was bound to at enrollment. Persisted in
    /// `Config.runner.workspace_slug` so CRUD subcommands can scope their
    /// REST calls without the user passing `--workspace`. `Option` purely
    /// for forward-compat with an older server that hasn't shipped the
    /// field yet — on success the server always populates it.
    #[serde(default)]
    pub workspace_slug: Option<String>,
    /// Project identifier the cloud assigned. Echoed back so the daemon
    /// can stamp it into `[[runner]]` without re-parsing the request.
    #[serde(default)]
    pub project_identifier: Option<String>,
    /// Pod id the runner was placed in. Optional for forward-compat with
    /// older servers; when present, persisted to `RunnerConfig.pod_id`.
    #[serde(default)]
    pub pod_id: Option<Uuid>,
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
) -> std::result::Result<RegisterResponse, RegisterError> {
    let url = format!(
        "{}/api/v1/runner/register/",
        cloud_url.trim_end_matches('/')
    );
    let client = http_client().map_err(RegisterError::Other)?;
    let resp = client
        .post(&url)
        .json(&RegisterEnvelope { req, token })
        .send()
        .await
        .with_context(|| format!("POST {url}"))
        .map_err(RegisterError::Other)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(classify_register_error(status, &body));
    }
    let out = resp
        .json::<RegisterResponse>()
        .await
        .context("parsing register response")
        .map_err(RegisterError::Other)?;
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

// Cap the server body we surface in error strings. A misbehaving (or
// hostile) cloud could echo the registration token back in an error body;
// the TUI renders this verbatim into `form.error`, so we truncate
// aggressively. 256 chars is enough to read a typical "unauthorized" /
// "invalid token" message without leaking a whole token or PII-laden JSON.
const MAX_ERROR_BODY_CHARS: usize = 256;

fn truncate_body(body: &str) -> String {
    let s = body.trim();
    if s.chars().count() <= MAX_ERROR_BODY_CHARS {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_ERROR_BODY_CHARS).collect();
    format!("{head}…(truncated)")
}

/// Translate an HTTP error response into a `RegisterError`. Extracted so
/// `configure`'s retry decision can be unit-tested without standing up a
/// fake HTTP server. The cloud-side contract is:
///
/// - `409 {"error": "runner_name_taken"}` → `NameTaken` (retryable for an
///   auto-generated name, loud error for a user-supplied one).
/// - Anything else non-2xx → `Other` with the status + body captured
///   (truncated to `MAX_ERROR_BODY_CHARS` to avoid leaking echoed tokens).
fn classify_register_error(status: reqwest::StatusCode, body: &str) -> RegisterError {
    if status == reqwest::StatusCode::CONFLICT
        && let Ok(parsed) = serde_json::from_str::<ErrorBody>(body)
        && parsed.error.as_deref() == Some("runner_name_taken")
    {
        return RegisterError::NameTaken;
    }
    RegisterError::Other(anyhow::anyhow!(
        "registration failed: HTTP {status}: {body}",
        body = truncate_body(body),
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
    fn register_response_without_workspace_slug_deserializes_to_none() {
        // Pins the forward-compat contract for `#[serde(default)]` on
        // `workspace_slug`: a response body from a server that pre-dates this
        // field must still parse, with the field defaulting to `None`. Without
        // this guard the runner would hard-fail against an older server.
        let body = r#"{
            "runner_id": "00000000-0000-0000-0000-000000000001",
            "runner_secret": "apd_rs_x",
            "heartbeat_interval_secs": 25,
            "protocol_version": 1
        }"#;
        let resp: RegisterResponse = serde_json::from_str(body).unwrap();
        assert!(resp.workspace_slug.is_none());
    }

    #[test]
    fn register_response_with_workspace_slug_deserializes() {
        let body = r#"{
            "runner_id": "00000000-0000-0000-0000-000000000001",
            "runner_secret": "apd_rs_x",
            "workspace_slug": "acme",
            "heartbeat_interval_secs": 25,
            "protocol_version": 1
        }"#;
        let resp: RegisterResponse = serde_json::from_str(body).unwrap();
        assert_eq!(resp.workspace_slug.as_deref(), Some("acme"));
    }

    #[test]
    fn classify_409_runner_name_taken_maps_to_name_taken() {
        // Pin the cloud-side contract: configure uses this to decide whether
        // to retry an auto-generated name or error out a user-supplied one.
        let err = classify_register_error(
            reqwest::StatusCode::CONFLICT,
            r#"{"error": "runner_name_taken"}"#,
        );
        assert!(
            matches!(err, RegisterError::NameTaken),
            "expected NameTaken, got {err:?}",
        );
    }

    #[test]
    fn classify_409_runner_cap_reached_maps_to_other() {
        // The same endpoint also returns 409 for the per-user cap. That one
        // must NOT trigger the auto-name retry — it's a hard error.
        let err = classify_register_error(
            reqwest::StatusCode::CONFLICT,
            r#"{"error": "runner cap reached (5)"}"#,
        );
        assert!(
            matches!(err, RegisterError::Other(_)),
            "expected Other, got {err:?}",
        );
    }

    #[test]
    fn classify_non_409_status_maps_to_other() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let err = classify_register_error(status, "some body");
            assert!(
                matches!(err, RegisterError::Other(_)),
                "{status} should map to Other, got {err:?}",
            );
        }
    }

    #[test]
    fn classify_truncates_large_bodies() {
        // A misbehaving cloud that echoes a token-length string back in the
        // error body must not land on screen verbatim. We cap at
        // MAX_ERROR_BODY_CHARS; the tail is replaced by a marker.
        let big = "x".repeat(MAX_ERROR_BODY_CHARS * 4);
        let err = classify_register_error(reqwest::StatusCode::UNAUTHORIZED, &big).to_string();
        assert!(
            err.contains("(truncated)"),
            "expected truncation marker, got: {err}"
        );
        assert!(
            err.len() < big.len(),
            "error string {} should be shorter than input {}",
            err.len(),
            big.len()
        );
    }

    #[test]
    fn truncate_body_preserves_short_strings() {
        assert_eq!(truncate_body("  invalid token  "), "invalid token");
    }
}
