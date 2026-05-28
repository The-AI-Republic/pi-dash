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
use crate::cli::runner_ops;

use super::project;
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
    /// Create a new work item under a project. If `--project` is omitted,
    /// the CLI uses PIDASH_PROJECT_ID, local config default_project, or
    /// the workspace default project from Pi Dash cloud.
    Create(CreateArgs),
    /// List work items in a project. Returns the server's paginated envelope
    /// (`{count, next_cursor, prev_cursor, results: [...]}`) — pass `--cursor`
    /// from a prior page to walk pages.
    List(ListArgs),
    /// Update fields on a work item. Pass only the fields you want to change.
    Patch(PatchArgs),
    /// Move a work item into another project in the same workspace.
    Move(MoveArgs),
    /// Full-text search across work item titles, descriptions, and
    /// comments. Returns matches with snippet, state, project, timestamps,
    /// and a relevance rank. Use it to recover historical context before
    /// starting similar work.
    Search(SearchArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: Option<String>,

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

#[derive(Debug, Args)]
pub struct MoveArgs {
    /// Project-scoped identifier, e.g. `ENG-42`.
    pub identifier: String,

    /// Target project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search pattern. Supports websearch syntax: quoted phrases, `OR`,
    /// `-exclude`. Stem-aware (e.g. `color` finds `colors`, `colored`).
    pub query: String,

    /// Scope to a single project (slug like `ENG` or project UUID).
    /// Omit to search the whole workspace.
    #[arg(long)]
    pub project: Option<String>,

    /// Filter by status: `open` (in progress), `closed` (completed or
    /// cancelled), or `all` (default).
    #[arg(long, default_value = "all")]
    pub status: String,

    /// Lower bound on `updated_at`, ISO 8601 (e.g. `2025-01-01T00:00:00Z`).
    #[arg(long)]
    pub since: Option<String>,

    /// Max results to return. Server default is 10, hard cap is 50 —
    /// tuned for agent context windows.
    #[arg(long)]
    pub limit: Option<u32>,

    /// Sort order: `rank` (relevance, default), `-created`
    /// (newest first), `-updated` (most-recently-updated first). The
    /// server rejects other values with a 400.
    #[arg(long)]
    pub sort: Option<String>,
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
        IssueCommand::Create(a) => cmd_create(&client, paths, a).await,
        IssueCommand::List(a) => cmd_list(&client, a).await,
        IssueCommand::Patch(p) => cmd_patch(&client, p).await,
        IssueCommand::Move(m) => cmd_move(&client, m).await,
        IssueCommand::Search(s) => cmd_search(&client, s).await,
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

async fn cmd_create(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    args: CreateArgs,
) -> Result<(), CliError> {
    if args.title.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--title must not be empty"));
    }

    let project_ref = resolve_create_project(client, paths, args.project.as_deref()).await?;

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
            resolve_state_name(client, &project_ref, &state).await?
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

async fn resolve_create_project(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    explicit: Option<&str>,
) -> Result<String, CliError> {
    if let Some(project) = explicit.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(project.to_string());
    }
    if let Ok(project) = std::env::var("PIDASH_PROJECT_ID") {
        let trimmed = project.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(project) = runner_ops::load_cli_default_project(paths)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("loading default project: {e}")))?
        && !project.trim().is_empty()
    {
        return Ok(project);
    }
    if let Some(default_project) = project::default_project(client).await? {
        return Ok(default_project.identifier);
    }
    Err(CliError::new(
        EXIT_INVALID,
        "--project is required because no default project is configured",
    ))
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

/// Build the ordered (key, value) param list for `pidash issue search`.
///
/// Pulled out of `cmd_search` so the URL contract is testable without a
/// network round-trip — the order matters because `build_query_string`
/// preserves it, and the agent prompt fragment documents specific
/// flag names.
///
/// Returns `Err(CliError)` if the trimmed query is empty (the only
/// pre-flight validation; `status` / `sort` / `since` validation is the
/// server's job, so the same string gets a 400 response that the agent
/// can parse).
fn build_search_params(args: &SearchArgs) -> Result<Vec<(&'static str, String)>, CliError> {
    let q = args.query.trim();
    if q.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "search query must not be empty"));
    }

    let mut params: Vec<(&'static str, String)> = vec![("q", q.to_string())];
    if let Some(p) = args.project.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        params.push(("project", p.to_string()));
    }
    // Send `status` only when it differs from the server default to keep
    // the URL terse on the common path.
    if args.status != "all" {
        params.push(("status", args.status.clone()));
    }
    if let Some(since) = args.since.as_ref() {
        params.push(("since", since.clone()));
    }
    if let Some(limit) = args.limit {
        params.push(("limit", limit.to_string()));
    }
    if let Some(sort) = args.sort.as_ref() {
        params.push(("sort", sort.clone()));
    }
    Ok(params)
}

async fn cmd_search(client: &ApiClient, args: SearchArgs) -> Result<(), CliError> {
    let params = build_search_params(&args)?;
    let query = build_query_string(&params);
    let path = format!(
        "workspaces/{}/work-items/search/advanced/{query}",
        client.env.workspace_slug
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_move(client: &ApiClient, args: MoveArgs) -> Result<(), CliError> {
    if args.project.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    let issue = resolve_issue(client, &args.identifier).await?;
    let body = serde_json::json!({ "project": args.project });
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/move/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.post(&path, &body).await?;
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
        assert_eq!(percent_encode_value("a=b&c+d e"), "a%3Db%26c%2Bd%20e");
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

    fn search_args(query: &str) -> SearchArgs {
        SearchArgs {
            query: query.to_string(),
            project: None,
            status: "all".to_string(),
            since: None,
            limit: None,
            sort: None,
        }
    }

    #[test]
    fn build_search_params_rejects_empty_query() {
        let args = search_args("");
        let err = build_search_params(&args).expect_err("empty query must be invalid");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("search query"));
    }

    #[test]
    fn build_search_params_rejects_whitespace_query() {
        let args = search_args("   ");
        assert!(build_search_params(&args).is_err());
    }

    #[test]
    fn build_search_params_minimum_only_carries_q() {
        let args = search_args("hello world");
        let params = build_search_params(&args).expect("valid args");
        // Common-path URLs stay terse — `status=all` is the server
        // default and is intentionally omitted.
        assert_eq!(params, vec![("q", "hello world".to_string())]);
    }

    #[test]
    fn build_search_params_carries_all_flags_in_documented_order() {
        // Order matters: the URL contract is documented (q first, then
        // project, status, since, limit, sort) and tests should pin it
        // so a future re-shuffle here is caught before it lands.
        let args = SearchArgs {
            query: "cache".to_string(),
            project: Some("ENG".to_string()),
            status: "closed".to_string(),
            since: Some("2025-01-01T00:00:00Z".to_string()),
            limit: Some(5),
            sort: Some("-created".to_string()),
        };
        let params = build_search_params(&args).expect("valid args");
        assert_eq!(
            params,
            vec![
                ("q", "cache".to_string()),
                ("project", "ENG".to_string()),
                ("status", "closed".to_string()),
                ("since", "2025-01-01T00:00:00Z".to_string()),
                ("limit", "5".to_string()),
                ("sort", "-created".to_string()),
            ]
        );
    }

    #[test]
    fn build_search_params_url_targets_advanced_endpoint() {
        // End-to-end string check: the resulting query string lands on
        // the documented advanced-search path with the documented param
        // names. Catches any future rename of `q` → `query` etc. that
        // would silently break the prompt-fragment contract.
        let args = SearchArgs {
            query: "x".to_string(),
            project: None,
            status: "open".to_string(),
            since: None,
            limit: Some(10),
            sort: None,
        };
        let params = build_search_params(&args).expect("valid args");
        let query_string = build_query_string(&params);
        assert_eq!(query_string, "?q=x&status=open&limit=10");
    }

    #[test]
    fn build_search_params_omits_empty_project() {
        // `--project ""` (or trailing whitespace) must not survive into
        // the URL — the server treats missing project as workspace-wide
        // and an empty-string param would 400.
        let args = SearchArgs {
            query: "x".to_string(),
            project: Some("   ".to_string()),
            status: "all".to_string(),
            since: None,
            limit: None,
            sort: None,
        };
        let params = build_search_params(&args).expect("valid args");
        assert!(params.iter().all(|(k, _)| *k != "project"));
    }
}
