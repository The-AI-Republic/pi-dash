// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash comment …` subcommands.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Map, Value};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

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

#[derive(Debug, Args)]
pub struct CommentSpeakerArgs {
    /// Mark the comment as spoken by an AI agent with this display name.
    #[arg(long = "as-agent", value_name = "NAME")]
    as_agent: Option<String>,
    /// Pi Dash agent run UUID that produced this comment.
    #[arg(long = "agent-run-id", value_name = "UUID")]
    agent_run_id: Option<String>,
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
        #[command(flatten)]
        speaker: CommentSpeakerArgs,
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

pub async fn run(args: CommentArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
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
            speaker,
        } => cmd_add(&client, &identifier, comment_body, speaker).await,
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
    speaker: CommentSpeakerArgs,
) -> Result<(), CliError> {
    let body = load_comment_body(comment_body)?;
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let mut payload: Map<String, Value> = Map::new();
    payload.insert("comment_html".into(), Value::String(body));
    add_speaker_metadata(&mut payload, speaker)?;
    let resp = client.post(&path, &Value::Object(payload)).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

fn add_speaker_metadata(
    payload: &mut Map<String, Value>,
    speaker: CommentSpeakerArgs,
) -> Result<(), CliError> {
    let as_agent = speaker.as_agent.and_then(non_empty);
    let agent_run_id = speaker.agent_run_id.and_then(non_empty);
    match (as_agent, agent_run_id) {
        (Some(label), agent_run_id) => {
            payload.insert("speaker_type".into(), Value::String("agent".into()));
            payload.insert("speaker_label".into(), Value::String(label));
            if let Some(run_id) = agent_run_id {
                payload.insert("speaker_agent_run_id".into(), Value::String(run_id));
            }
        }
        (None, Some(_)) => {
            return Err(CliError::new(
                EXIT_INVALID,
                "--agent-run-id requires --as-agent so Pi Dash can mark the comment speaker",
            ));
        }
        (None, None) => {}
    }
    Ok(())
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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
    use serde_json::{Map, Value};

    use super::{CommentBodyArgs, CommentSpeakerArgs, add_speaker_metadata, load_comment_body};

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

    #[test]
    fn add_speaker_metadata_marks_agent_comment() {
        let mut payload = Map::new();
        add_speaker_metadata(
            &mut payload,
            CommentSpeakerArgs {
                as_agent: Some("Codex".into()),
                agent_run_id: Some("11111111-1111-1111-1111-111111111111".into()),
            },
        )
        .expect("speaker metadata");

        assert_eq!(
            payload.get("speaker_type"),
            Some(&Value::String("agent".into()))
        );
        assert_eq!(
            payload.get("speaker_label"),
            Some(&Value::String("Codex".into()))
        );
        assert_eq!(
            payload.get("speaker_agent_run_id"),
            Some(&Value::String(
                "11111111-1111-1111-1111-111111111111".into()
            ))
        );
    }

    #[test]
    fn add_speaker_metadata_rejects_agent_run_without_agent_label() {
        let mut payload = Map::new();
        let err = add_speaker_metadata(
            &mut payload,
            CommentSpeakerArgs {
                as_agent: None,
                agent_run_id: Some("11111111-1111-1111-1111-111111111111".into()),
            },
        )
        .expect_err("run id without agent label should fail");

        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
        assert!(err.message.contains("--agent-run-id requires --as-agent"));
        assert!(payload.is_empty());
    }
}
