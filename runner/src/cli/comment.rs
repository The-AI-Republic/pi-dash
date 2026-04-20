// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash comment …` subcommands.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_UNKNOWN, report_error};

use super::resolve::resolve_issue;

#[derive(Debug, Args)]
pub struct CommentArgs {
    #[command(subcommand)]
    pub command: CommentCommand,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct CommentBodyArgs {
    /// Comment body (plain text or markdown).
    #[arg(long)]
    body: Option<String>,
    /// Path to a file containing the comment body.
    #[arg(long = "body-file", value_name = "PATH")]
    body_file: Option<PathBuf>,
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
        #[command(flatten)]
        comment_body: CommentBodyArgs,
    },
    /// Edit an existing comment owned by this user. Requires the issue
    /// identifier because the REST URL is project-scoped.
    Update {
        /// Work item identifier the comment lives on, e.g. `ENG-42`.
        identifier: String,
        /// Comment UUID.
        comment_id: String,
        #[command(flatten)]
        comment_body: CommentBodyArgs,
    },
}

pub async fn run(args: CommentArgs) -> i32 {
    let env = match CliEnv::from_env() {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        CommentCommand::List { identifier } => cmd_list(&client, &identifier).await,
        CommentCommand::Add {
            identifier,
            comment_body,
        } => cmd_add(&client, &identifier, comment_body).await,
        CommentCommand::Update {
            identifier,
            comment_id,
            comment_body,
        } => cmd_update(&client, &identifier, &comment_id, comment_body).await,
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
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_add(
    client: &ApiClient,
    identifier: &str,
    comment_body: CommentBodyArgs,
) -> Result<(), CliError> {
    let body = load_comment_body(comment_body)?;
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.post(&path, &json!({"comment_html": body})).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_update(
    client: &ApiClient,
    identifier: &str,
    comment_id: &str,
    comment_body: CommentBodyArgs,
) -> Result<(), CliError> {
    let body = load_comment_body(comment_body)?;
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/{}/",
        client.env.workspace_slug, issue.project_id, issue.id, comment_id
    );
    let mut payload: Map<String, Value> = Map::new();
    payload.insert("comment_html".into(), Value::String(body));
    let resp = client.patch(&path, &Value::Object(payload)).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

fn load_comment_body(args: CommentBodyArgs) -> Result<String, CliError> {
    match (args.body, args.body_file) {
        (Some(body), None) => Ok(body),
        (None, Some(path)) => std::fs::read_to_string(&path).map_err(|e| {
            CliError::new(
                EXIT_UNKNOWN,
                format!(
                    "failed reading comment body file {}: {e}",
                    display_path(&path)
                ),
            )
        }),
        _ => unreachable!("clap enforces exactly one comment body source"),
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{CommentBodyArgs, load_comment_body};

    #[test]
    fn load_comment_body_prefers_inline_body() {
        let body = load_comment_body(CommentBodyArgs {
            body: Some("hello".into()),
            body_file: None,
        })
        .expect("inline body");
        assert_eq!(body, "hello");
    }

    #[test]
    fn load_comment_body_reads_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("comment.md");
        std::fs::write(&path, "from file\n").expect("write file");

        let body = load_comment_body(CommentBodyArgs {
            body: None,
            body_file: Some(path),
        })
        .expect("file body");
        assert_eq!(body, "from file\n");
    }
}
