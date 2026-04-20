// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash state …` subcommands.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, report_error};

use super::resolve::{looks_like_uuid, resolve_issue};

#[derive(Debug, Args)]
pub struct StateArgs {
    #[command(subcommand)]
    pub command: StateCommand,
}

#[derive(Debug, Subcommand)]
pub enum StateCommand {
    /// List states for a project. Pass either a project UUID or an issue
    /// identifier (e.g. `ENG-42`); in the latter case the project is
    /// resolved from the issue.
    List {
        /// Project UUID or work-item identifier.
        project_or_issue: String,
    },
}

pub async fn run(args: StateArgs) -> i32 {
    let env = match CliEnv::from_env() {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(1, format!("{e}"))),
    };

    let result = match args.command {
        StateCommand::List { project_or_issue } => cmd_list(&client, &project_or_issue).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, arg: &str) -> Result<(), CliError> {
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
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    Ok(())
}
