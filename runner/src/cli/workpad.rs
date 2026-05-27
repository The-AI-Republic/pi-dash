// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash workpad …` subcommands.
//!
//! The workpad is the coding agent's durable per-issue scratchpad. It used
//! to live in a dedicated `## Agent Workpad` IssueComment; with the comment
//! thread now reserved for human ↔ agent conversation, the workpad is a
//! plain markdown field on the issue itself and these commands are the
//! agent's read/write channel.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::json;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::resolve::resolve_issue;

#[derive(Debug, Args)]
pub struct WorkpadArgs {
    #[command(subcommand)]
    pub command: WorkpadCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkpadCommand {
    /// Fetch the agent workpad for a work item.
    ///
    /// Defaults the work-item identifier to `PIDASH_ISSUE_IDENTIFIER` when
    /// omitted, so the agent can call `pidash workpad get` with no args.
    Get {
        /// Work item identifier, e.g. `ENG-42`. Defaults to
        /// `PIDASH_ISSUE_IDENTIFIER`.
        identifier: Option<String>,
    },
    /// Overwrite the agent workpad. An empty file clears it.
    Update {
        /// Work item identifier, e.g. `ENG-42`. Defaults to
        /// `PIDASH_ISSUE_IDENTIFIER`.
        identifier: Option<String>,
        /// Path to a file containing the workpad body (markdown).
        #[arg(long = "body-file", value_name = "PATH")]
        body_file: PathBuf,
    },
}

pub async fn run(args: WorkpadArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        WorkpadCommand::Get { identifier } => cmd_get(&client, identifier).await,
        WorkpadCommand::Update {
            identifier,
            body_file,
        } => cmd_update(&client, identifier, body_file).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_get(client: &ApiClient, identifier: Option<String>) -> Result<(), CliError> {
    let current_issue = std::env::var("PIDASH_ISSUE_IDENTIFIER").ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_update(
    client: &ApiClient,
    identifier: Option<String>,
    body_file: PathBuf,
) -> Result<(), CliError> {
    let body = load_workpad_body(&body_file)?;
    let current_issue = std::env::var("PIDASH_ISSUE_IDENTIFIER").ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.patch(&path, &json!({ "body": body })).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

fn resolve_identifier<'a>(
    explicit: Option<&'a str>,
    current_issue: Option<&'a str>,
) -> Result<&'a str, CliError> {
    explicit.or(current_issue).ok_or_else(|| {
        CliError::new(
            EXIT_INVALID,
            "workpad requires an issue identifier or PIDASH_ISSUE_IDENTIFIER",
        )
    })
}

fn load_workpad_body(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| {
        CliError::new(
            EXIT_UNKNOWN,
            format!("failed reading workpad body file {}: {e}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{load_workpad_body, resolve_identifier};

    #[test]
    fn resolve_prefers_explicit_arg() {
        let target = resolve_identifier(Some("ENG-42"), Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-42");
    }

    #[test]
    fn resolve_falls_back_to_env() {
        let target = resolve_identifier(None, Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-7");
    }

    #[test]
    fn resolve_requires_context() {
        let err = resolve_identifier(None, None).expect_err("missing target");
        assert_eq!(
            err.message,
            "workpad requires an issue identifier or PIDASH_ISSUE_IDENTIFIER"
        );
    }

    #[test]
    fn load_workpad_body_reads_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pad.md");
        std::fs::write(&path, "## Agent Workpad\n\nphase: implementing\n").expect("write file");

        let body = load_workpad_body(&path).expect("body");
        assert!(body.contains("phase: implementing"));
    }
}
