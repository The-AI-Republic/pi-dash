// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash state …` subcommands.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::resolve::{looks_like_uuid, resolve_issue};

#[derive(Debug, Args)]
pub struct StateArgs {
    #[command(subcommand)]
    pub command: StateCommand,
}

#[derive(Debug, Subcommand)]
pub enum StateCommand {
    /// List states for the current issue's project. Optionally pass either a
    /// project UUID or an issue identifier (e.g. `ENG-42`) to override the
    /// current issue context from `PIDASH_ISSUE_IDENTIFIER`.
    List {
        /// Optional project UUID or work-item identifier.
        project_or_issue: Option<String>,
    },
}

pub async fn run(args: StateArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        StateCommand::List { project_or_issue } => cmd_list(&client, &project_or_issue).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, project_or_issue: &Option<String>) -> Result<(), CliError> {
    let current_issue = std::env::var("PIDASH_ISSUE_IDENTIFIER").ok();
    let arg = state_list_target(project_or_issue.as_deref(), current_issue.as_deref())?;
    let project_id = if looks_like_uuid(arg) {
        arg.to_string()
    } else {
        resolve_issue(client, arg).await?.project_id
    };
    let path = format!(
        "workspaces/{}/projects/{}/states/",
        client.env.workspace_slug, project_id
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

fn state_list_target<'a>(
    explicit: Option<&'a str>,
    current_issue: Option<&'a str>,
) -> Result<&'a str, CliError> {
    explicit.or(current_issue).ok_or_else(|| {
        CliError::new(
            EXIT_INVALID,
            "state list requires a project UUID, issue identifier, or PIDASH_ISSUE_IDENTIFIER",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::state_list_target;

    #[test]
    fn state_list_uses_explicit_arg_first() {
        let target = state_list_target(Some("ENG-42"), Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-42");
    }

    #[test]
    fn state_list_falls_back_to_current_issue_env() {
        let target = state_list_target(None, Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-7");
    }

    #[test]
    fn state_list_requires_context() {
        let err = state_list_target(None, None).expect_err("missing target");
        assert_eq!(
            err.message,
            "state list requires a project UUID, issue identifier, or PIDASH_ISSUE_IDENTIFIER"
        );
    }
}
