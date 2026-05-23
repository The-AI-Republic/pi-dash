// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash project …` subcommands.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
