// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash issue …` subcommands.
//!
//! Thin wrappers around the `/api/v1/` REST surface. JSON on stdout, JSON on
//! stderr for errors, exit codes per `api_client::EXIT_*`.

use clap::{Args, Subcommand};
use serde_json::{Map, Value};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

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
    /// Create a new work item under a project. `--project` is required — the CLI
    /// is machine-global and intentionally has no default project, so the caller
    /// must always name one (slug like `ENG` or a project UUID).
    Create(CreateArgs),
    /// Update fields on a work item. Pass only the fields you want to change.
    Patch(PatchArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Title (required).
    #[arg(long)]
    pub title: String,

    /// Description (plain text or markdown).
    #[arg(long)]
    pub description: Option<String>,

    /// Priority: `none|low|medium|high|urgent`.
    #[arg(long)]
    pub priority: Option<String>,

    /// Initial state — exact state name (case-insensitive) or a state UUID.
    #[arg(long)]
    pub state: Option<String>,
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

pub async fn run(args: IssueArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        IssueCommand::Get { identifier } => cmd_get(&client, &identifier).await,
        IssueCommand::Create(a) => cmd_create(&client, a).await,
        IssueCommand::Patch(p) => cmd_patch(&client, p).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_get(client: &ApiClient, identifier: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    println!(
        "{}",
        serde_json::to_string(&issue.raw).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_create(client: &ApiClient, args: CreateArgs) -> Result<(), CliError> {
    if args.title.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--title must not be empty"));
    }

    // Pass `--project` straight through. The backend accepts either a UUID or
    // a workspace-scoped slug in the URL path, so the CLI no longer pre-resolves
    // it. The same value seeds the state-name resolution URL below.
    let project_ref = args.project.as_str();

    let mut body: Map<String, Value> = Map::new();
    body.insert("name".into(), Value::String(args.title));
    if let Some(desc) = args.description {
        body.insert("description".into(), Value::String(desc));
    }
    if let Some(prio) = args.priority {
        body.insert("priority".into(), Value::String(prio));
    }
    if let Some(state) = args.state {
        let uuid = if looks_like_uuid(&state) {
            state
        } else {
            resolve_state_name(client, project_ref, &state).await?
        };
        body.insert("state".into(), Value::String(uuid));
    }

    let path = format!(
        "workspaces/{}/projects/{}/work-items/",
        client.env.workspace_slug, project_ref
    );
    let resp = client.post(&path, &Value::Object(body)).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
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
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}
