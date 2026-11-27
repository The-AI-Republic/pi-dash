// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash state …` subcommands.

use clap::{Args, Subcommand};

use serde_json::Value;

use crate::api_client::{
    ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_UNKNOWN, report_error,
};

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
    /// List states for the current issue's project. Optionally pass a
    /// project identifier (e.g. `ENG`), a project UUID, or an issue
    /// identifier (e.g. `ENG-42`) to override the current issue context
    /// from `PIDASH_ISSUE_IDENTIFIER`.
    List {
        /// Optional project identifier, project UUID, or work-item
        /// identifier (`ENG`, `<uuid>`, `ENG-42`).
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
    let resp = list_states(
        client,
        project_or_issue.as_deref(),
        project.as_deref(),
        current_issue.as_deref(),
        current_project.as_deref(),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// Fetch the states of the project named by `--project`, the positional
/// argument, or the run context (see `state_list_target`). A project the
/// server does not know (404) is reported by name.
pub async fn list_states(
    client: &ApiClient,
    project_or_issue: Option<&str>,
    project: Option<&str>,
    current_issue: Option<&str>,
    current_project: Option<&str>,
) -> Result<Value, CliError> {
    let target = state_list_target(project, project_or_issue, current_issue, current_project)?;
    let (project_id, named_project) = match target {
        StateListTarget::Project(project) => (project.clone(), Some(project)),
        // A UUID in `PIDASH_ISSUE_IDENTIFIER` has always been used as the
        // project UUID directly; keep that.
        StateListTarget::Issue(arg) if looks_like_uuid(&arg) => (arg, None),
        StateListTarget::Issue(arg) => (resolve_issue(client, &arg).await?.project_id, None),
    };
    let path = format!(
        "workspaces/{}/projects/{}/states/",
        client.env.workspace_slug, project_id
    );
    client.get(&path).await.map_err(|e| match named_project {
        Some(name) if e.exit_code == EXIT_NOT_FOUND => {
            let mut err = CliError::new(EXIT_NOT_FOUND, format!("project '{name}' not found"));
            err.detail = e.detail;
            err.status = e.status;
            err
        }
        _ => e,
    })
}

/// `<PROJ>-<number>`, e.g. `ENG-42`: the shape of a work-item identifier.
/// Anything else that is not a UUID is taken as a project identifier.
fn looks_like_issue_identifier(s: &str) -> bool {
    s.rsplit_once('-').is_some_and(|(prefix, num)| {
        !prefix.is_empty() && !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit())
    })
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
    // The positional argument is a project (identifier or UUID) unless it
    // has the `<PROJ>-<number>` shape of a work-item identifier.
    if let Some(arg) = explicit.map(str::trim).filter(|a| !a.is_empty()) {
        if looks_like_uuid(arg) || !looks_like_issue_identifier(arg) {
            return Ok(StateListTarget::Project(arg.to_string()));
        }
        return Ok(StateListTarget::Issue(arg.to_string()));
    }
    if let Some(arg) = current_issue {
        return Ok(StateListTarget::Issue(arg.to_string()));
    }
    current_project
        .map(|project| StateListTarget::Project(project.to_string()))
        .ok_or_else(|| {
            CliError::new(
                EXIT_INVALID,
                "state list requires a project identifier or UUID, issue identifier, PIDASH_ISSUE_IDENTIFIER, or PIDASH_PROJECT",
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
    fn state_list_positional_project_forms_are_projects() {
        for arg in ["OPENHUB", "e509f5c9-c6fd-4b1a-a89c-8cb863610500", "MY-PROJ"] {
            let target = state_list_target(None, Some(arg), Some("ENG-7"), None).expect("target");
            assert_eq!(project(&target), arg);
        }
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
            "state list requires a project identifier or UUID, issue identifier, PIDASH_ISSUE_IDENTIFIER, or PIDASH_PROJECT"
        );
    }
}
