// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash __desktop` — verbs the Pi Dash desktop app calls to share this
//! host's sign-in.
//!
//! The desktop app drives the user-installed `pidash` with the default
//! config paths. When its own window is signed out but this host is signed
//! in, it asks for a one-time redeem URL (`__desktop handoff`) and opens it in
//! its webview, which signs the window in as the same user.
//!
//! Output is JSON on stdout; failures are `{"error": "<code>", "message":
//! "..."}` with a non-zero exit, so the app never has to parse prose.
//!
//! Hidden because it is plumbing for the app, not a user-facing verb.

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use serde::Deserialize;
use std::time::Duration;

use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct DesktopArgs {
    #[command(subcommand)]
    pub command: DesktopCommand,
}

#[derive(Debug, Subcommand)]
pub enum DesktopCommand {
    /// Exchange `[cli].handoff_key` for a one-time URL that signs the desktop
    /// webview in as this host's user. Prints `{"redeem_url": "..."}`.
    Handoff,
}

pub async fn run(args: DesktopArgs, paths: &Paths) -> Result<()> {
    match args.command {
        DesktopCommand::Handoff => {
            match handoff(paths).await {
                Ok(redeem_url) => {
                    println!("{}", serde_json::json!({ "redeem_url": redeem_url }));
                }
                Err(e) => {
                    println!(
                        "{}",
                        serde_json::json!({ "error": e.code, "message": e.message })
                    );
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}

#[derive(Debug)]
struct HandoffError {
    code: &'static str,
    message: String,
}

impl HandoffError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct HandoffResponse {
    redeem_url: String,
}

async fn handoff(paths: &Paths) -> std::result::Result<String, HandoffError> {
    let cfg = file::load_config_opt(paths)
        .map_err(|e| HandoffError::new("handoff_failed", format!("{e:#}")))?;
    let (cloud_url, cli) = match cfg {
        Some(cfg) => (cfg.daemon.cloud_url, cfg.cli.unwrap_or_default()),
        None => {
            return Err(HandoffError::new(
                "not_signed_in",
                "this host is not signed in to Pi Dash",
            ));
        }
    };
    if cloud_url.is_empty() || cli.token.as_deref().is_none_or(str::is_empty) {
        return Err(HandoffError::new(
            "not_signed_in",
            "this host is not signed in to Pi Dash",
        ));
    }
    let key = cli.handoff_key.filter(|k| !k.is_empty()).ok_or_else(|| {
        HandoffError::new(
            "no_handoff_key",
            "this host's sign-in has no desktop handoff key; sign in again with `pidash auth login`",
        )
    })?;
    request_redeem_url(
        &cloud_url,
        &key,
        crate::util::machine_fingerprint::machine_fingerprint().as_deref(),
    )
    .await
}

/// `POST /api/v1/auth/desktop-handoff/` with the handoff key as a bearer.
async fn request_redeem_url(
    cloud_url: &str,
    handoff_key: &str,
    machine_fingerprint: Option<&str>,
) -> std::result::Result<String, HandoffError> {
    let failed = |message: String| HandoffError::new("handoff_failed", message);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
        .build()
        .map_err(|e| failed(format!("building HTTP client: {e}")))?;
    let url = format!(
        "{}/api/v1/auth/desktop-handoff/",
        cloud_url.trim_end_matches('/')
    );
    let mut body = serde_json::json!({});
    if let Some(fp) = machine_fingerprint {
        body["machine_fingerprint"] = serde_json::Value::String(fp.to_string());
    }
    let resp = client
        .post(&url)
        .bearer_auth(handoff_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| failed(format!("POST {url}: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet: String = text.chars().take(256).collect();
        return Err(failed(format!(
            "desktop handoff failed: HTTP {status}: {snippet}"
        )));
    }
    let parsed: HandoffResponse = serde_json::from_str(&text)
        .map_err(|e| failed(format!("parsing desktop handoff response: {e}")))?;
    if parsed.redeem_url.is_empty() {
        return Err(failed("desktop handoff response had no redeem_url".into()));
    }
    Ok(parsed.redeem_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{CliSection, Config, DaemonConfig};
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn write(paths: &Paths, cli: Option<CliSection>) {
        file::write_config(
            paths,
            &Config {
                version: 2,
                daemon: DaemonConfig {
                    cloud_url: "https://cloud.example".into(),
                    ..DaemonConfig::default()
                },
                runners: vec![],
                cli,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn no_config_is_not_signed_in() {
        let tmp = tempdir().unwrap();
        let err = handoff(&paths_for(tmp.path())).await.unwrap_err();
        assert_eq!(err.code, "not_signed_in");
    }

    #[tokio::test]
    async fn signed_out_config_is_not_signed_in() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write(
            &paths,
            Some(CliSection {
                handoff_key: Some("dhk_x".into()),
                ..Default::default()
            }),
        );
        assert_eq!(handoff(&paths).await.unwrap_err().code, "not_signed_in");
    }

    #[tokio::test]
    async fn signed_in_without_key_reports_no_handoff_key() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write(
            &paths,
            Some(CliSection {
                token: Some("mt_x".into()),
                ..Default::default()
            }),
        );
        assert_eq!(handoff(&paths).await.unwrap_err().code, "no_handoff_key");
    }
}
