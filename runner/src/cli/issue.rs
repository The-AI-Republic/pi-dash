// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash issue …` subcommands.
//!
//! Thin wrappers around the `/api/v1/` REST surface. JSON on stdout, JSON on
//! stderr for errors, exit codes per `api_client::EXIT_*`.

use clap::{Args, Subcommand};
use serde_json::{Map, Value};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, report_error};

use super::resolve::{looks_like_uuid, resolve_issue, resolve_state_name};

#[derive(Debug, Args)]
pub struct IssueArgs {
    #[command(subcommand)]
    pub command: IssueCommand,
}

#[derive(Debug, Subcommand)]
pub enum IssueCommand {
    /// Fetch a work item by `PROJ-123` identifier. Prints the full payload as JSON.
    Get {
        /// Project-scoped identifier, e.g. `ENG-42`.
        identifier: String,
    },
    /// Update fields on a work item. Pass only the fields you want to change.
    Patch(PatchArgs),
}

#[derive(Debug, Args)]
pub struct PatchArgs {
    /// Project-scoped identifier, e.g. `ENG-42`.
    pub identifier: String,

    /// Target state — either the exact state name (case-insensitive) or a state UUID.
    #[arg(long)]
    pub state: Option<String>,

    /// New title.
    #[arg(long)]
    pub title: Option<String>,

    /// New description (plain text or markdown).
    #[arg(long)]
    pub description: Option<String>,

    /// Priority: `none|low|medium|high|urgent`.
    #[arg(long)]
    pub priority: Option<String>,
}

pub async fn run(args: IssueArgs) -> i32 {
    let env = match CliEnv::from_env() {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(1, format!("{e}"))),
    };

    let result = match args.command {
        IssueCommand::Get { identifier } => cmd_get(&client, &identifier).await,
        IssueCommand::Patch(p) => cmd_patch(&client, p).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_get(client: &ApiClient, identifier: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    println!("{}", serde_json::to_string(&issue.raw).unwrap_or_default());
    Ok(())
}

async fn cmd_patch(client: &ApiClient, args: PatchArgs) -> Result<(), CliError> {
    let mut body: Map<String, Value> = Map::new();

    if let Some(ref title) = args.title {
        body.insert("name".into(), Value::String(title.clone()));
    }
    if let Some(ref desc) = args.description {
        // The Issue serializer exposes `description` as the plain text variant;
        // richer formats (description_html / description_json) go through the
        // web editor path, not the CLI.
        body.insert("description".into(), Value::String(desc.clone()));
    }
    if let Some(ref prio) = args.priority {
        body.insert("priority".into(), Value::String(prio.clone()));
    }

    // Resolve issue first — we always need project_id for the mutating PATCH URL.
    let issue = resolve_issue(client, &args.identifier).await?;

    if let Some(ref state) = args.state {
        let uuid = if looks_like_uuid(state) {
            state.clone()
        } else {
            resolve_state_name(client, &issue.project_id, state).await?
        };
        body.insert("state".into(), Value::String(uuid));
    }

    if body.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "at least one of --state/--title/--description/--priority is required",
        ));
    }

    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.patch(&path, &Value::Object(body)).await?;
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    Ok(())
}
