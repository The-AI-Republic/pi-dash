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
    /// List work items in a project. Returns the server's paginated envelope
    /// (`{count, next_cursor, prev_cursor, results: [...]}`) — pass `--cursor`
    /// from a prior page to walk pages.
    List(ListArgs),
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

    /// Order-by field, e.g. `-created_at` (default), `priority`, `state__name`.
    #[arg(long)]
    pub order_by: Option<String>,
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
        IssueCommand::List(a) => cmd_list(&client, a).await,
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
    if args.project.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
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

async fn cmd_list(client: &ApiClient, args: ListArgs) -> Result<(), CliError> {
    if args.project.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    let project_ref = args.project.as_str();

    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(c) = args.cursor.as_ref() {
        params.push(("cursor", c.clone()));
    }
    if let Some(n) = args.per_page {
        params.push(("per_page", n.to_string()));
    }
    if let Some(o) = args.order_by.as_ref() {
        params.push(("order_by", o.clone()));
    }
    let query = build_query_string(&params);

    let path = format!(
        "workspaces/{}/projects/{}/work-items/{query}",
        client.env.workspace_slug, project_ref
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// Build a query-string suffix (`?k=v&...`) with percent-encoded values.
/// Returns an empty string when there are no params.
fn build_query_string(params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", percent_encode_value(v)))
        .collect();
    format!("?{}", pairs.join("&"))
}

/// Percent-encode bytes outside the unreserved set (RFC 3986 §2.3) for use in
/// query-string values. Inline so we don't add a dep just for this one site.
fn percent_encode_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_value_passes_unreserved() {
        assert_eq!(percent_encode_value("ENG-42_v1.0~beta"), "ENG-42_v1.0~beta");
    }

    #[test]
    fn percent_encode_value_encodes_query_separators() {
        // `=`, `&`, `+`, ` ` would otherwise corrupt the query-string parse.
        assert_eq!(
            percent_encode_value("a=b&c+d e"),
            "a%3Db%26c%2Bd%20e"
        );
    }

    #[test]
    fn percent_encode_value_encodes_multibyte_utf8() {
        // π = 0xCF 0x80
        assert_eq!(percent_encode_value("π"), "%CF%80");
    }

    #[test]
    fn build_query_string_empty_yields_empty() {
        assert_eq!(build_query_string(&[]), "");
    }

    #[test]
    fn build_query_string_joins_and_encodes() {
        let params = vec![
            ("cursor", "abc=def&ghi".to_string()),
            ("per_page", "50".to_string()),
        ];
        assert_eq!(
            build_query_string(&params),
            "?cursor=abc%3Ddef%26ghi&per_page=50"
        );
    }
}
