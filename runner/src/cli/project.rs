// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash project …` subcommands.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::issue::build_query_string;
use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_NOT_FOUND, EXIT_UNKNOWN, report_error};

#[derive(Debug, Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// List projects in the active workspace. Prints JSON.
    List,
    /// List a project's runners from the cloud (status, heartbeat, pod).
    /// Prints JSON. For the runners configured on this machine, use
    /// `pidash runner list`.
    Runners(RunnersArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RunnersArgs {
    /// Project identifier (e.g. ENG), slug, or UUID.
    pub project: String,
    /// Narrow to a single pod (UUID).
    #[arg(long)]
    pub pod: Option<String>,
    /// Include desktop-bundled runners, which are hidden by default.
    #[arg(long)]
    pub include_bundled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRow {
    pub id: String,
    pub identifier: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_default: bool,
}

pub async fn run(args: ProjectArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        ProjectCommand::List => cmd_list(&client).await,
        ProjectCommand::Runners(runners_args) => cmd_runners(&client, runners_args).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn list_projects(client: &ApiClient) -> Result<Vec<ProjectRow>, CliError> {
    let path = format!("workspaces/{}/projects/", client.env.workspace_slug);
    let resp = client.get(&path).await?;
    project_rows_from_value(resp)
}

pub async fn resolve_project(
    client: &ApiClient,
    project_ref: &str,
) -> Result<ProjectRow, CliError> {
    let needle = project_ref.trim();
    let needle_upper = needle.to_uppercase();
    let projects = list_projects(client).await?;
    projects
        .into_iter()
        .find(|p| p.id == needle || p.identifier.to_uppercase() == needle_upper)
        .ok_or_else(|| CliError::new(EXIT_NOT_FOUND, format!("project {needle:?} not found")))
}

pub async fn default_project(client: &ApiClient) -> Result<Option<ProjectRow>, CliError> {
    Ok(list_projects(client)
        .await?
        .into_iter()
        .find(|p| p.is_default))
}

/// `GET workspaces/<slug>/projects/<uuid>/runners/` — the cloud's view of a
/// project's runners, sharing visibility rules with the web AI Workers panel.
/// The project reference is resolved to a UUID client-side (same three-way
/// id/identifier match as the other project subcommands), so an unknown
/// project fails with `EXIT_NOT_FOUND` before any runners call.
pub async fn list_project_runners(
    client: &ApiClient,
    args: &RunnersArgs,
) -> Result<Value, CliError> {
    let project = resolve_project(client, &args.project).await?;
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(pod) = &args.pod {
        params.push(("pod", pod.clone()));
    }
    if args.include_bundled {
        params.push(("include_bundled", "true".to_string()));
    }
    let path = format!(
        "workspaces/{}/projects/{}/runners/{}",
        client.env.workspace_slug,
        project.id,
        build_query_string(&params),
    );
    client.get(&path).await
}

async fn cmd_runners(client: &ApiClient, args: RunnersArgs) -> Result<(), CliError> {
    let runners = list_project_runners(client, &args).await?;
    println!(
        "{}",
        serde_json::to_string(&runners).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_list(client: &ApiClient) -> Result<(), CliError> {
    let projects = list_projects(client).await?;
    println!(
        "{}",
        serde_json::to_string(&projects).expect("serialize JSON value")
    );
    Ok(())
}

fn project_rows_from_value(resp: Value) -> Result<Vec<ProjectRow>, CliError> {
    if let Some(results) = resp.get("results") {
        return serde_json::from_value(results.clone())
            .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project list JSON: {e}")));
    }
    serde_json::from_value(resp)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project list JSON: {e}")))
}
