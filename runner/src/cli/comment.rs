// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash comment …` subcommands.

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::api_client::{ApiClient, CliEnv, CliError, report_error};

use super::resolve::resolve_issue;

#[derive(Debug, Args)]
pub struct CommentArgs {
    #[command(subcommand)]
    pub command: CommentCommand,
}

#[derive(Debug, Subcommand)]
pub enum CommentCommand {
    /// List comments on a work item.
    List {
        /// Work item identifier, e.g. `ENG-42`.
        identifier: String,
    },
    /// Post a new comment on a work item.
    Add {
        /// Work item identifier, e.g. `ENG-42`.
        identifier: String,
        /// Comment body (plain text or markdown).
        #[arg(long)]
        body: String,
    },
    /// Edit an existing comment owned by this user. Requires the issue
    /// identifier because the REST URL is project-scoped.
    Update {
        /// Work item identifier the comment lives on, e.g. `ENG-42`.
        identifier: String,
        /// Comment UUID.
        comment_id: String,
        /// New comment body.
        #[arg(long)]
        body: String,
    },
}

pub async fn run(args: CommentArgs) -> i32 {
    let env = match CliEnv::from_env() {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(1, format!("{e}"))),
    };

    let result = match args.command {
        CommentCommand::List { identifier } => cmd_list(&client, &identifier).await,
        CommentCommand::Add { identifier, body } => cmd_add(&client, &identifier, &body).await,
        CommentCommand::Update {
            identifier,
            comment_id,
            body,
        } => cmd_update(&client, &identifier, &comment_id, &body).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, identifier: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.get(&path).await?;
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    Ok(())
}

async fn cmd_add(client: &ApiClient, identifier: &str, body: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.post(&path, &json!({"comment_html": body})).await?;
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    Ok(())
}

async fn cmd_update(
    client: &ApiClient,
    identifier: &str,
    comment_id: &str,
    body: &str,
) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/{}/",
        client.env.workspace_slug, issue.project_id, issue.id, comment_id
    );
    let mut payload: Map<String, Value> = Map::new();
    payload.insert("comment_html".into(), Value::String(body.to_string()));
    let resp = client.patch(&path, &Value::Object(payload)).await?;
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    Ok(())
}
