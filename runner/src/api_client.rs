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

/// Runtime configuration sourced from the environment.
///
/// The agent receives these from the runner-injected env (PR 2). For
/// out-of-band operator use, the same vars can be exported in the shell.
#[derive(Debug, Clone)]
pub struct CliEnv {
    pub api_url: String,
    pub workspace_slug: String,
    pub token: String,
}

impl CliEnv {
    pub fn from_env() -> Result<Self, CliError> {
        let api_url = std::env::var("PIDASH_API_URL")
            .map_err(|_| CliError::new(EXIT_INVALID, "PIDASH_API_URL is not set"))?;
        let workspace_slug = std::env::var("PIDASH_WORKSPACE_SLUG").map_err(|_| {
            CliError::new(EXIT_INVALID, "PIDASH_WORKSPACE_SLUG is not set")
        })?;
        let token = std::env::var("PIDASH_TOKEN")
            .map_err(|_| CliError::new(EXIT_INVALID, "PIDASH_TOKEN is not set"))?;
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
        let text = resp.text().await.unwrap_or_default();
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
        500..=599 => EXIT_SERVER,
        _ => EXIT_UNKNOWN,
    };
    let msg = match status.as_u16() {
        400 | 409 | 422 => "invalid request",
        401 | 403 => "auth failed",
        404 => "not found",
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
