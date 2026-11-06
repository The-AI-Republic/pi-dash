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
    /// Set the folder new built-in engine runners work under
    /// (`[engine] task_root`). Each project gets
    /// `<root>/<workspace>/<project>/`. Projects already set up keep their
    /// folder; nothing is moved. `default` returns to the default root.
    #[command(name = "engine-task-root")]
    EngineTaskRoot {
        /// Absolute path (a relative one is taken from the current
        /// directory), or `default`.
        path: String,
    },
}

pub async fn run(args: ConfigArgs, paths: &crate::util::paths::Paths) -> anyhow::Result<()> {
    let result = match args.command {
        ConfigCommand::Set(s) => match s.command {
            SetCommand::DefaultProject { project } => set_default_project(paths, &project).await,
            SetCommand::EngineTaskRoot { path } => {
                return set_engine_task_root(paths, &path).await;
            }
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

/// `pidash config set engine-task-root <path>`: validated by the running
/// daemon when one answers (design §10), else here with the same rules.
async fn set_engine_task_root(paths: &crate::util::paths::Paths, raw: &str) -> anyhow::Result<()> {
    use crate::ipc::protocol::{Request, Response};
    use anyhow::Context;
    let task_root = if raw.trim() == "default" {
        None
    } else {
        let p = std::path::PathBuf::from(raw.trim());
        Some(if p.is_absolute() {
            p
        } else {
            std::env::current_dir()
                .context("reading the current directory")?
                .join(p)
        })
    };
    let settings = match crate::ipc::client::Client::connect(paths.ipc_socket_path()).await {
        Ok(mut client) => match client
            .call(Request::EngineSettingsSet {
                task_root: task_root.clone(),
            })
            .await?
        {
            Response::EngineSettings(s) => s,
            Response::Error(e) => anyhow::bail!("{}", e.message),
            other => anyhow::bail!("unexpected daemon response: {other:?}"),
        },
        Err(_) => crate::daemon::engine_ops::set_task_root(paths, task_root)?,
    };
    println!(
        "Engine task root: {}{}",
        settings.task_root.display(),
        if settings.task_root_is_default {
            " (default)"
        } else {
            ""
        }
    );
    for w in &settings.warnings {
        println!("warning: {w}");
    }
    println!("Projects already set up keep their folder; new ones use this root.");
    Ok(())
}
