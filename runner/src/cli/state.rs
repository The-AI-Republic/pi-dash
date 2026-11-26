// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash state …` subcommands.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::resolve::{looks_like_uuid, resolve_issue};

/// Environment variable the daemon sets with the run's project
/// identifier (NEWPIDASH-320 §6.3). `state list` falls back to it when
/// neither `--project`, a positional argument, nor
/// `PIDASH_ISSUE_IDENTIFIER` names the project.
pub const PROJECT_ENV: &str = "PIDASH_PROJECT";

/// Environment variable the daemon sets with the run's issue identifier.
pub const ISSUE_ENV: &str = "PIDASH_ISSUE_IDENTIFIER";

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
        /// Project UUID or identifier (e.g. `ENG`). Wins over the
        /// positional argument; both default from `PIDASH_ISSUE_IDENTIFIER`,
        /// then `PIDASH_PROJECT`.
        #[arg(long, value_name = "PROJ-or-UUID")]
        project: Option<String>,
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
        StateCommand::List {
            project_or_issue,
            project,
        } => cmd_list(&client, &project_or_issue, &project).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// What to list states for: an explicit `--project` (a project UUID or
/// identifier, passed straight through — project routes accept both),
/// or an issue context (positional, `PIDASH_ISSUE_IDENTIFIER`, then
/// `PIDASH_PROJECT`) resolved to its project.
#[derive(Debug)]
enum StateListTarget {
    Project(String),
    Issue(String),
}

async fn cmd_list(
    client: &ApiClient,
    project_or_issue: &Option<String>,
    project: &Option<String>,
) -> Result<(), CliError> {
    let current_issue = std::env::var(ISSUE_ENV).ok();
    let current_project = std::env::var(PROJECT_ENV).ok();
    let target = state_list_target(
        project.as_deref(),
        project_or_issue.as_deref(),
        current_issue.as_deref(),
        current_project.as_deref(),
    )?;
    let project_id = match target {
        StateListTarget::Project(project) => project,
        StateListTarget::Issue(arg) => {
            if looks_like_uuid(&arg) {
                arg
            } else {
                resolve_issue(client, &arg).await?.project_id
            }
        }
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

fn state_list_target(
    project: Option<&str>,
    explicit: Option<&str>,
    current_issue: Option<&str>,
    current_project: Option<&str>,
) -> Result<StateListTarget, CliError> {
    if let Some(project) = project {
        return Ok(StateListTarget::Project(project.to_string()));
    }
    // A bare UUID stays ambiguous by design: it may be a project UUID or
    // an issue UUID, and both resolve to the same project below.
    if let Some(arg) = explicit.or(current_issue) {
        return Ok(StateListTarget::Issue(arg.to_string()));
    }
    current_project
        .map(|project| StateListTarget::Project(project.to_string()))
        .ok_or_else(|| {
            CliError::new(
                EXIT_INVALID,
                "state list requires a project UUID, issue identifier, PIDASH_ISSUE_IDENTIFIER, or PIDASH_PROJECT",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{StateListTarget, state_list_target};

    fn issue(target: &StateListTarget) -> &str {
        match target {
            StateListTarget::Issue(arg) => arg,
            StateListTarget::Project(_) => panic!("expected issue target"),
        }
    }

    fn project(target: &StateListTarget) -> &str {
        match target {
            StateListTarget::Project(arg) => arg,
            StateListTarget::Issue(_) => panic!("expected project target"),
        }
    }

    #[test]
    fn state_list_project_flag_wins() {
        let target = state_list_target(Some("ENG"), Some("ENG-42"), Some("ENG-7"), Some("ENG"))
            .expect("target");
        assert_eq!(project(&target), "ENG");
    }

    #[test]
    fn state_list_uses_explicit_arg_first() {
        let target =
            state_list_target(None, Some("ENG-42"), Some("ENG-7"), Some("ENG")).expect("target");
        assert_eq!(issue(&target), "ENG-42");
    }

    #[test]
    fn state_list_falls_back_to_current_issue_env() {
        let target = state_list_target(None, None, Some("ENG-7"), Some("ENG")).expect("target");
        assert_eq!(issue(&target), "ENG-7");
    }

    #[test]
    fn state_list_falls_back_to_current_project_env() {
        let target = state_list_target(None, None, None, Some("ENG")).expect("target");
        assert_eq!(project(&target), "ENG");
    }

    #[test]
    fn state_list_requires_context() {
        let err = state_list_target(None, None, None, None).expect_err("missing target");
        assert_eq!(
            err.message,
            "state list requires a project UUID, issue identifier, PIDASH_ISSUE_IDENTIFIER, or PIDASH_PROJECT"
        );
    }
}
