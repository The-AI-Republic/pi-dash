// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash config …` subcommands.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_UNKNOWN, report_error};
use crate::cli::runner_ops;

use super::project::resolve_project;

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Set local CLI defaults.
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct SetArgs {
    #[command(subcommand)]
    pub command: SetCommand,
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// Set the local default project used by non-interactive CLI calls.
    #[command(name = "default-project")]
    DefaultProject {
        /// Project identifier or UUID.
        project: String,
    },
}

pub async fn run(args: ConfigArgs, paths: &crate::util::paths::Paths) -> anyhow::Result<()> {
    let result = match args.command {
        ConfigCommand::Set(s) => match s.command {
            SetCommand::DefaultProject { project } => set_default_project(paths, &project).await,
        },
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let code = report_error(&e);
            std::process::exit(code);
        }
    }
}

async fn set_default_project(
    paths: &crate::util::paths::Paths,
    project_ref: &str,
) -> Result<(), CliError> {
    let env = CliEnv::resolve(paths)?;
    let client = ApiClient::new(env).map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{e}")))?;
    let project = resolve_project(&client, project_ref).await?;
    let path = format!(
        "workspaces/{}/projects/{}/",
        client.env.workspace_slug, project.id
    );
    let body = serde_json::json!({ "is_default": true });
    client.patch(&path, &body).await?;
    runner_ops::write_cli_default_project(paths, &project.identifier)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("writing default project: {e}")))?;
    println!(
        "Set default project to {} ({}).",
        project.name, project.identifier
    );
    Ok(())
}
