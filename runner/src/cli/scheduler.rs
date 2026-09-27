// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash scheduler …` subcommands — read-only visibility into project
//! schedulers (PDASHOSS01-225).
//!
//! Schedulers are workspace-level definitions installed onto projects; a
//! project install carries the cadence and fires agent runs. This surface
//! is strictly read-only: list the schedulers a workspace or project has,
//! read one definition (prompt included), and audit its recent runs.
//! Creating, editing, enabling or triggering schedulers is out of scope.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_UNKNOWN, report_error};

use super::issue::{build_query_string, resolve_project_ref};

#[derive(Debug, Args)]
pub struct SchedulerArgs {
    #[command(subcommand)]
    pub command: SchedulerCommand,
}

#[derive(Debug, Subcommand)]
pub enum SchedulerCommand {
    /// List schedulers. With `--project`, the schedulers installed on that
    /// project (each with its cadence, next and last occurrence); without,
    /// every scheduler definition in the workspace.
    List {
        /// Project identifier (slug like `ENG`) or project UUID.
        #[arg(long)]
        project: Option<String>,
    },
    /// Read one scheduler's full definition — prompt, enabled flags, and its
    /// install on the project (cadence, outcome mode, pod, last error).
    Get {
        /// Scheduler UUID (from `pidash scheduler list`).
        scheduler_id: String,
        /// Project identifier or UUID. Defaults to the current run's project.
        #[arg(long)]
        project: Option<String>,
    },
    /// List recent agent runs fired by a scheduler on a project, newest
    /// first: status, timing, and the issues each run wrote to.
    Runs {
        /// Scheduler UUID (from `pidash scheduler list`).
        scheduler_id: String,
        /// Maximum number of runs to return (server default 30, max 100).
        #[arg(long)]
        limit: Option<u32>,
        /// Project identifier or UUID. Defaults to the current run's project.
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn run(args: SchedulerArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        SchedulerCommand::List { project } => cmd_list(&client, project.as_deref()).await,
        SchedulerCommand::Get {
            scheduler_id,
            project,
        } => cmd_get(&client, paths, &scheduler_id, project.as_deref()).await,
        SchedulerCommand::Runs {
            scheduler_id,
            limit,
            project,
        } => cmd_runs(&client, paths, &scheduler_id, limit, project.as_deref()).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, project: Option<&str>) -> Result<(), CliError> {
    // Bare `scheduler list` is workspace-wide by design: unlike labels or
    // states, scheduler definitions live on the workspace, so the no-flag
    // form should not silently narrow to a default project.
    let path = match project.map(str::trim).filter(|p| !p.is_empty()) {
        Some(project) => format!(
            "workspaces/{}/projects/{}/schedulers/",
            client.env.workspace_slug, project
        ),
        None => format!("workspaces/{}/schedulers/", client.env.workspace_slug),
    };
    print_json(client.get(&path).await?)
}

async fn cmd_get(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    scheduler_id: &str,
    project: Option<&str>,
) -> Result<(), CliError> {
    let project = resolve_project_ref(client, paths, project).await?;
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/",
        client.env.workspace_slug,
        project,
        scheduler_id.trim()
    );
    print_json(client.get(&path).await?)
}

async fn cmd_runs(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    scheduler_id: &str,
    limit: Option<u32>,
    project: Option<&str>,
) -> Result<(), CliError> {
    let project = resolve_project_ref(client, paths, project).await?;
    let query = runs_query(limit);
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/runs/{query}",
        client.env.workspace_slug,
        project,
        scheduler_id.trim()
    );
    print_json(client.get(&path).await?)
}

/// `--limit` maps onto the server's standard `per_page` pagination knob.
fn runs_query(limit: Option<u32>) -> String {
    match limit {
        Some(limit) => build_query_string(&[("per_page", limit.to_string())]),
        None => String::new(),
    }
}

fn print_json(value: serde_json::Value) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string(&value).expect("serialize JSON value")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::runs_query;

    #[test]
    fn runs_query_maps_limit_to_per_page() {
        assert_eq!(runs_query(Some(25)), "?per_page=25");
    }

    #[test]
    fn runs_query_empty_without_limit() {
        assert_eq!(runs_query(None), "");
    }
}
