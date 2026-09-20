// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash page …` subcommands.
//!
//! Project pages are a project-scoped wiki: decisions, conventions, stage
//! plans. These commands are read-only wrappers around the `/api/v1/` page
//! endpoints, so an agent can look up shared knowledge while it works.
//!
//! JSON on stdout, JSON on stderr for errors, exit codes per
//! `api_client::EXIT_*` — same contract as `issue` and `workpad`.

use clap::{Args, Subcommand};
use serde_json::Value;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_SERVER, EXIT_UNKNOWN, report_error};

use super::issue::build_query_string;
use super::resolve::looks_like_uuid;

#[derive(Debug, Args)]
pub struct PageArgs {
    #[command(subcommand)]
    pub command: PageCommand,
}

#[derive(Debug, Subcommand)]
pub enum PageCommand {
    /// List the pages of a project. Returns the server's paginated envelope
    /// (`{count, next_cursor, prev_cursor, results: [...]}`) carrying page
    /// metadata only — read a single page to get its body.
    List(ListArgs),
    /// Fetch one page, including `description_markdown`.
    Get(GetArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Pagination cursor returned from a prior page (the `next_cursor` field).
    #[arg(long)]
    pub cursor: Option<String>,

    /// Items per page. Server-side default applies if omitted.
    #[arg(long)]
    pub per_page: Option<u32>,

    /// Also list archived pages. Excluded by default.
    #[arg(long)]
    pub include_archived: bool,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Page UUID, as listed by `pidash page list`.
    pub page_id: String,

    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Print only the page body as markdown, so it can be redirected to a
    /// file. Without it the whole JSON envelope is printed.
    #[arg(long)]
    pub body_only: bool,
}

pub async fn run(args: PageArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        PageCommand::List(args) => cmd_list(&client, args).await,
        PageCommand::Get(args) => cmd_get(&client, args).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn cmd_list(client: &ApiClient, args: ListArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let query = build_query_string(&list_params(&args));
    // Project-scoped REST routes accept either a UUID or the workspace-scoped
    // slug in the path, so `--project` goes through unresolved (see
    // `cli::resolve`).
    let path = format!(
        "workspaces/{}/projects/{project_ref}/pages/{query}",
        client.env.workspace_slug
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

pub async fn cmd_get(client: &ApiClient, args: GetArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let page_id = validated_page_id(&args.page_id)?;
    let path = format!(
        "workspaces/{}/projects/{project_ref}/pages/{page_id}/",
        client.env.workspace_slug
    );
    let resp = client.get(&path).await?;
    println!("{}", render_get(&resp, args.body_only)?);
    Ok(())
}

/// What `pidash page get` writes to stdout: the JSON envelope by default, or
/// the bare markdown body under `--body-only` so an agent can redirect it
/// straight into a file.
fn render_get(resp: &Value, body_only: bool) -> Result<String, CliError> {
    if body_only {
        return Ok(markdown_body(resp)?.to_string());
    }
    Ok(serde_json::to_string(resp).expect("serialize JSON value"))
}

/// Collect the list query parameters in a stable order so the resulting
/// query string is predictable (and assertable in tests) without a
/// network round-trip.
fn list_params(args: &ListArgs) -> Vec<(&'static str, String)> {
    let mut params: Vec<(&'static str, String)> = Vec::new();
    if let Some(c) = args.cursor.as_ref() {
        params.push(("cursor", c.clone()));
    }
    if let Some(n) = args.per_page {
        params.push(("per_page", n.to_string()));
    }
    if args.include_archived {
        params.push(("include_archived", "true".to_string()));
    }
    params
}

fn validated_project(project: &str) -> Result<&str, CliError> {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    Ok(trimmed)
}

/// The page route binds `<uuid:page_id>`, so a non-UUID would 404 with a
/// routing error rather than a useful message. Reject it locally instead.
fn validated_page_id(page_id: &str) -> Result<&str, CliError> {
    let trimmed = page_id.trim();
    if !looks_like_uuid(trimmed) {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("page id must be a UUID, got {trimmed:?}"),
        ));
    }
    Ok(trimmed)
}

/// Pull `description_markdown` out of a page payload for `--body-only`.
///
/// A page with an empty body is legitimate and prints nothing; a payload
/// missing the field entirely means the server is older than this CLI, which
/// is worth an error rather than silent emptiness.
fn markdown_body(resp: &Value) -> Result<&str, CliError> {
    resp.get("description_markdown")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CliError::new(
                EXIT_SERVER,
                "response missing 'description_markdown'; the Pi Dash server may predate page markdown",
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        GetArgs, ListArgs, list_params, markdown_body, render_get, validated_page_id,
        validated_project,
    };
    use crate::api_client::{EXIT_INVALID, EXIT_SERVER};
    use crate::cli::issue::build_query_string;

    const PAGE_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn list_args() -> ListArgs {
        ListArgs {
            project: "ENG".to_string(),
            cursor: None,
            per_page: None,
            include_archived: false,
        }
    }

    fn get_args() -> GetArgs {
        GetArgs {
            page_id: PAGE_UUID.to_string(),
            project: "ENG".to_string(),
            body_only: false,
        }
    }

    #[test]
    fn list_without_options_sends_no_query_string() {
        assert_eq!(build_query_string(&list_params(&list_args())), "");
    }

    #[test]
    fn list_include_archived_sets_the_flag() {
        let args = ListArgs {
            include_archived: true,
            ..list_args()
        };

        assert_eq!(
            build_query_string(&list_params(&args)),
            "?include_archived=true"
        );
    }

    #[test]
    fn list_default_omits_include_archived_so_the_server_default_applies() {
        let params = list_params(&list_args());

        assert!(!params.iter().any(|(k, _)| *k == "include_archived"));
    }

    #[test]
    fn list_pagination_params_are_encoded() {
        let args = ListArgs {
            cursor: Some("20:1:0".to_string()),
            per_page: Some(50),
            include_archived: true,
            ..list_args()
        };

        assert_eq!(
            build_query_string(&list_params(&args)),
            "?cursor=20%3A1%3A0&per_page=50&include_archived=true"
        );
    }

    #[test]
    fn empty_project_is_rejected() {
        let err = validated_project("   ").expect_err("empty project must be invalid");

        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn project_is_passed_through_untouched() {
        assert_eq!(validated_project(" ENG ").expect("project"), "ENG");
    }

    #[test]
    fn page_id_must_be_a_uuid() {
        let err = validated_page_id("release-checklist").expect_err("slug must be invalid");

        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("must be a UUID"));
    }

    #[test]
    fn page_id_accepts_a_uuid() {
        assert_eq!(validated_page_id(PAGE_UUID).expect("page id"), PAGE_UUID);
    }

    #[test]
    fn body_only_extracts_the_markdown() {
        let resp = json!({"id": PAGE_UUID, "description_markdown": "# Title\n\nbody"});

        assert_eq!(markdown_body(&resp).expect("body"), "# Title\n\nbody");
    }

    #[test]
    fn body_only_accepts_an_empty_body() {
        let resp = json!({"id": PAGE_UUID, "description_markdown": ""});

        assert_eq!(markdown_body(&resp).expect("body"), "");
    }

    #[test]
    fn body_only_errors_when_the_server_omits_the_field() {
        let resp = json!({"id": PAGE_UUID, "description_html": "<p>x</p>"});

        let err = markdown_body(&resp).expect_err("missing field must error");

        assert_eq!(err.exit_code, EXIT_SERVER);
        assert!(err.message.contains("description_markdown"));
    }

    #[test]
    fn get_defaults_to_the_full_envelope() {
        let args = get_args();
        let resp = json!({"id": PAGE_UUID, "description_markdown": "# Title"});

        assert!(!args.body_only);
        assert_eq!(
            render_get(&resp, args.body_only).expect("rendered"),
            serde_json::to_string(&resp).expect("json")
        );
    }

    #[test]
    fn body_only_prints_just_the_markdown() {
        let resp = json!({"id": PAGE_UUID, "name": "Conventions", "description_markdown": "# Title\n\nbody"});

        assert_eq!(render_get(&resp, true).expect("rendered"), "# Title\n\nbody");
    }
}
