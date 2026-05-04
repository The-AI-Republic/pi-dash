// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! HTTP client for the `pidash` CRUD subcommands.
//!
//! Distinct from `cloud::register` (one-shot enrollment) and `cloud::ws`
//! (long-lived WebSocket). This module only implements the thin layer the
//! CLI needs to talk to the `/api/v1/` REST surface with an `X-Api-Key`
//! header and uniform error → exit-code mapping.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::Value;

/// CLI exit codes documented in the implementation plan (Open design question 1).
pub const EXIT_INVALID: i32 = 2;
pub const EXIT_AUTH: i32 = 3;
pub const EXIT_NOT_FOUND: i32 = 4;
pub const EXIT_SERVER: i32 = 5;
pub const EXIT_THROTTLED: i32 = 6;
pub const EXIT_UNKNOWN: i32 = 1;

/// Error type that remembers a suggested process exit code.
///
/// `anyhow::Error` alone would lose the code distinction; we need it
/// to translate HTTP status into `std::process::ExitCode`.
#[derive(Debug)]
pub struct CliError {
    pub exit_code: i32,
    pub message: String,
    /// Optional upstream body (already trimmed) — surfaced in the stderr JSON
    /// so agents can inspect the server's error detail without a second call.
    pub detail: Option<String>,
}

impl CliError {
    pub fn new(exit_code: i32, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

/// Runtime configuration the CRUD subcommands need to talk to the cloud.
///
/// **Resolution order** (see `CliEnv::resolve`):
///   1. The runner config file at `~/.config/pidash/config.toml`.
///      `api_url` ← `[daemon].cloud_url`,
///      `workspace_slug` ← primary `[[runner]].workspace_slug`,
///      `token` ← `[cli].token`.
///   2. If the file is missing, OR any required field is absent from
///      it, fall through to the corresponding env var
///      (`PIDASH_API_URL` / `PIDASH_WORKSPACE_SLUG` / `PIDASH_TOKEN`).
///
/// This means: an enrolled host doesn't need any env vars — the CLI
/// reads everything from the same config the daemon uses. Out-of-band
/// operator use (CI scripts, ad-hoc invocations) can still rely on env
/// vars. The agent path Just Works because the agent inherits the
/// runner's $HOME, so the CLI finds the config without anyone wiring
/// per-spawn env vars.
#[derive(Debug, Clone)]
pub struct CliEnv {
    pub api_url: String,
    pub workspace_slug: String,
    pub token: String,
}

impl CliEnv {
    /// Env-only loader, retained as a fallback for callers without
    /// a `Paths` handle and as the leaf step of `resolve`.
    pub fn from_env() -> Result<Self, CliError> {
        let api_url = std::env::var("PIDASH_API_URL")
            .map_err(|_| CliError::new(EXIT_INVALID, "PIDASH_API_URL is not set"))?;
        let workspace_slug = std::env::var("PIDASH_WORKSPACE_SLUG")
            .map_err(|_| CliError::new(EXIT_INVALID, "PIDASH_WORKSPACE_SLUG is not set"))?;
        let token = std::env::var("PIDASH_TOKEN")
            .map_err(|_| CliError::new(EXIT_INVALID, "PIDASH_TOKEN is not set"))?;
        Ok(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            workspace_slug,
            token,
        })
    }

    /// Preferred entrypoint for CLI subcommands invoked from a path
    /// where `Paths` is in scope. Reads the runner config file and
    /// falls back to env vars per-field for anything missing.
    ///
    /// Returns `CliError(EXIT_INVALID)` if NEITHER source has a
    /// required value, with a message naming the specific field so
    /// the user knows what to add.
    pub fn resolve(paths: &crate::util::paths::Paths) -> Result<Self, CliError> {
        let cfg = crate::config::file::load_config_opt(paths).ok().flatten();
        let from_cfg_url = cfg
            .as_ref()
            .map(|c| c.daemon.cloud_url.trim().to_string())
            .filter(|s| !s.is_empty());
        let from_cfg_workspace = cfg.as_ref().and_then(|c| {
            c.runners
                .first()
                .and_then(|r| r.workspace_slug.as_deref())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        });
        let from_cfg_token = cfg.as_ref().and_then(|c| {
            c.cli
                .as_ref()
                .and_then(|s| s.token.as_deref())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        });

        let api_url = from_cfg_url
            .or_else(|| std::env::var("PIDASH_API_URL").ok())
            .ok_or_else(|| {
                CliError::new(
                    EXIT_INVALID,
                    "no cloud URL configured: set [daemon].cloud_url in \
                     pidash config or export PIDASH_API_URL",
                )
            })?;
        let workspace_slug = from_cfg_workspace
            .or_else(|| std::env::var("PIDASH_WORKSPACE_SLUG").ok())
            .ok_or_else(|| {
                CliError::new(
                    EXIT_INVALID,
                    "no workspace slug configured: set [[runner]].workspace_slug \
                     in pidash config or export PIDASH_WORKSPACE_SLUG",
                )
            })?;
        let token = from_cfg_token
            .or_else(|| std::env::var("PIDASH_TOKEN").ok())
            .ok_or_else(|| {
                CliError::new(
                    EXIT_INVALID,
                    "no auth token configured: set [cli].token in pidash \
                     config or export PIDASH_TOKEN",
                )
            })?;

        Ok(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            workspace_slug,
            token,
        })
    }

    /// Build a full URL under `/api/v1/` from a path fragment that must not
    /// start with a leading slash (e.g. `workspaces/acme/users/me/`).
    pub fn v1(&self, tail: &str) -> String {
        format!("{}/api/v1/{}", self.api_url, tail.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;
    use crate::config::file::write_config;
    use crate::config::schema::{
        AgentSection, ApprovalPolicySection, ClaudeCodeSection, CliSection, CodexSection,
        Config, DaemonConfig, RunnerConfig, WorkspaceSection,
    };
    use crate::util::paths::Paths;
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn make_config(token: &str) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://cloud.example/".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners: vec![RunnerConfig {
                name: "r1".into(),
                runner_id: uuid::Uuid::new_v4(),
                workspace_slug: Some("acme".into()),
                project_slug: Some("WEB".into()),
                pod_id: None,
                workspace: WorkspaceSection {
                    working_dir: std::path::PathBuf::from("/tmp/wd"),
                },
                agent: AgentSection::default(),
                codex: CodexSection::default(),
                claude_code: ClaudeCodeSection::default(),
                approval_policy: ApprovalPolicySection::default(),
            }],
            cli: Some(CliSection {
                token: Some(token.into()),
            }),
        }
    }

    #[test]
    fn resolve_reads_url_workspace_token_from_config_file() {
        // The new behaviour: with a populated config file on disk, the
        // CLI reads URL, workspace, and token from the same file the
        // daemon uses. This is the happy-path that fixes the runner's
        // "PIDASH_API_URL is not set" bug — no env vars needed.
        //
        // Resolver tries config FIRST, so this assertion holds
        // regardless of whether the test process has PIDASH_*  exported
        // (the env path is never reached when config has all three).
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_config(&paths, &make_config("tok-from-cfg")).unwrap();

        let env = CliEnv::resolve(&paths).expect("resolve should succeed");
        // Trailing slash in the config is stripped by the resolver so
        // `v1()` builds clean URLs.
        assert_eq!(env.api_url, "https://cloud.example");
        assert_eq!(env.workspace_slug, "acme");
        assert_eq!(env.token, "tok-from-cfg");
    }
}

/// Thin HTTP client scoped to a single CLI invocation.
pub struct ApiClient {
    pub env: CliEnv,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(env: CliEnv) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("pidash-cli/{}", crate::RUNNER_VERSION))
            .build()
            .context("build http client")?;
        Ok(Self { env, http })
    }

    pub async fn get(&self, path: &str) -> Result<Value, CliError> {
        self.request(Method::GET, path, None::<&()>).await
    }

    pub async fn patch<B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Value, CliError> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn post<B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Value, CliError> {
        self.request(Method::POST, path, Some(body)).await
    }

    async fn request<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<Value, CliError> {
        let url = self.env.v1(path);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .header("X-Api-Key", &self.env.token)
            .header("Accept", "application/json");
        if let Some(payload) = body {
            req = req.json(payload);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{method} {url}: {e}")))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| {
            CliError::new(
                EXIT_UNKNOWN,
                format!("{method} {url}: failed reading response body: {e}"),
            )
        })?;
        if status.is_success() {
            if text.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| {
                CliError::new(EXIT_SERVER, format!("invalid JSON from {url}: {e}"))
                    .with_detail(text)
            });
        }
        Err(map_error_status(status, text))
    }
}

fn map_error_status(status: StatusCode, body: String) -> CliError {
    let exit_code = match status.as_u16() {
        400 | 409 | 422 => EXIT_INVALID,
        401 | 403 => EXIT_AUTH,
        404 => EXIT_NOT_FOUND,
        429 => EXIT_THROTTLED,
        500..=599 => EXIT_SERVER,
        _ => EXIT_UNKNOWN,
    };
    let msg = match status.as_u16() {
        400 | 409 | 422 => "invalid request",
        401 | 403 => "auth failed",
        404 => "not found",
        429 => "throttled",
        500..=599 => "server error",
        _ => "request failed",
    };
    CliError::new(exit_code, format!("HTTP {status}: {msg}")).with_detail(body)
}

/// Emit a JSON error payload to stderr and return the recommended exit code.
///
/// Used by every CLI subcommand to keep the stderr contract uniform:
/// `{"error": "<short>"}` or `{"error": "<short>", "detail": "<upstream body>"}`.
pub fn report_error(err: &CliError) -> i32 {
    let payload = match &err.detail {
        Some(d) => serde_json::json!({"error": err.message, "detail": d}),
        None => serde_json::json!({"error": err.message}),
    };
    eprintln!("{payload}");
    err.exit_code
}
