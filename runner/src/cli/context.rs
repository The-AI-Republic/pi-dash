// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash context …` subcommands.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::project::{ProjectRow, resolve_project};

#[derive(Debug, Args)]
pub struct ContextArgs {
    #[command(subcommand)]
    pub command: ContextCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Write .pidash/context.md for this workspace.
    Init(InitArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Local workspace directory. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

pub async fn run(args: ContextArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        ContextCommand::Init(a) => cmd_init(&client, a).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn write_context_for_project(
    paths: &crate::util::paths::Paths,
    workspace_dir: &Path,
    project_ref: &str,
) -> Result<PathBuf, CliError> {
    let env = CliEnv::resolve(paths)?;
    let client = ApiClient::new(env).map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{e}")))?;
    let project = resolve_project(&client, project_ref).await?;
    write_context_file(workspace_dir, &client.env.workspace_slug, &project)
}

async fn cmd_init(client: &ApiClient, args: InitArgs) -> Result<(), CliError> {
    if args.project.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    let workspace = match args.workspace {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| {
            CliError::new(EXIT_UNKNOWN, format!("resolving current directory: {e}"))
        })?,
    };
    let project = resolve_project(client, &args.project).await?;
    let path = write_context_file(&workspace, &client.env.workspace_slug, &project)?;
    println!("{}", serde_json::json!({"path": path, "project": project}));
    Ok(())
}

fn write_context_file(
    workspace_dir: &Path,
    workspace_slug: &str,
    project: &ProjectRow,
) -> Result<PathBuf, CliError> {
    let pidash_dir = workspace_dir.join(".pidash");
    fs::create_dir_all(&pidash_dir)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("creating {:?}: {e}", pidash_dir)))?;
    let path = pidash_dir.join("context.md");
    let body = render_context(workspace_slug, project);
    fs::write(&path, body)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("writing {:?}: {e}", path)))?;
    Ok(path)
}

fn render_context(workspace_slug: &str, project: &ProjectRow) -> String {
    format!(
        "---\nworkspace_slug: {}\ndefault_project_id: {}\nprojects:\n  - id: {}\n    identifier: {}\n    name: {}\n    description: {}\n    is_default: {}\n---\n\n# Pi Dash Workspace Context\n\nThis workspace is linked to the Pi Dash projects above.\n",
        yaml_string(workspace_slug),
        yaml_string(&project.id),
        yaml_string(&project.id),
        yaml_string(&project.identifier),
        yaml_string(&project.name),
        yaml_string(&project.description),
        project.is_default,
    )
}

fn yaml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
