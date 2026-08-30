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
    /// Attach a GitHub pull request or GitLab merge request to a work item so
    /// Pi Dash links it and tracks its status. Idempotent; one issue may have
    /// many code reviews, but one code review attaches to exactly one issue.
    AttachReview(AttachReviewArgs),
    /// Attach a GitHub pull request to a work item. Alias of `attach-review`
    /// kept for existing scripts.
    AttachPr(AttachReviewArgs),
    /// Re-grant a fresh ticking budget to an issue whose agent-ticker budget
    /// is exhausted, so the periodic agent runs resume. No-op (reports
    /// `granted: false`) unless the issue is currently in a ticking state
    /// (In Progress / In Review / In Test) AND its current budget is used up.
    ReTick {
        /// Project-scoped identifier, e.g. `ENG-42`.
        identifier: String,
    },
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

    /// Parent issue — project-scoped identifier (`PROJ-123`) or a raw UUID.
    /// Attaches the new work item as a sub-issue of the given parent.
    #[arg(long)]
    pub parent: Option<String>,
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

    /// New parent issue — project-scoped identifier (`PROJ-123`) or a raw
    /// UUID. Mutually exclusive with `--clear-parent`.
    #[arg(long, conflicts_with = "clear_parent")]
    pub parent: Option<String>,

    /// Detach the current parent, making this a top-level issue (sends
    /// `parent: null`). Mutually exclusive with `--parent`.
    #[arg(long)]
    pub clear_parent: bool,
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
pub struct AttachReviewArgs {
    /// Project-scoped identifier, e.g. `ENG-42`.
    pub identifier: String,

    /// GitHub pull request or GitLab merge request URL.
    #[arg(long)]
    pub url: String,
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
        IssueCommand::AttachReview(a) => cmd_attach_review(&client, a).await,
        IssueCommand::AttachPr(a) => cmd_attach_review(&client, a).await,
        IssueCommand::ReTick { identifier } => cmd_re_tick(&client, &identifier).await,
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

    // Resolve the network-dependent fields first, then hand the already-resolved
    // values to the pure body builder so the URL/body contract is unit-testable.
    let state_uuid = match args.state.as_deref() {
        Some(state) if looks_like_uuid(state) => Some(state.to_string()),
        Some(state) => Some(resolve_state_name(client, &project_ref, state).await?),
        None => None,
    };
    let parent_uuid = match args.parent.as_deref() {
        Some(parent) => Some(resolve_parent_id(client, parent).await?),
        None => None,
    };

    let body = build_create_body(
        args.title,
        args.description.as_deref(),
        args.priority,
        state_uuid,
        parent_uuid,
    );

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

/// Assemble the `work-items` POST body from already-resolved values. Kept pure
/// (no network) so the field contract — including the rich-text `description_html`
/// key and the `parent` FK the MCP path also sends — is unit-testable.
fn build_create_body(
    title: String,
    description: Option<&str>,
    priority: Option<String>,
    state_uuid: Option<String>,
    parent_uuid: Option<String>,
) -> Map<String, Value> {
    let mut body: Map<String, Value> = Map::new();
    body.insert("name".into(), Value::String(title));
    if let Some(desc) = description {
        // Issue descriptions are stored as rich text: the API's serializer is a
        // ModelSerializer over the `Issue` model, whose only description column
        // is `description_html` (it has no plain `description` field). Sending a
        // bare `description` key was silently dropped, so CLI-created issues had
        // an empty body. Convert the plain-text/markdown input to minimal HTML
        // and send it under the key the server actually persists.
        body.insert(
            "description_html".into(),
            Value::String(description_to_html(desc)),
        );
    }
    if let Some(prio) = priority {
        body.insert("priority".into(), Value::String(prio));
    }
    if let Some(uuid) = state_uuid {
        body.insert("state".into(), Value::String(uuid));
    }
    if let Some(uuid) = parent_uuid {
        body.insert("parent".into(), Value::String(uuid));
    }
    body
}

/// Resolve a `--parent` value to an issue UUID. A raw UUID is accepted as-is
/// (mirroring `--state`); otherwise the `PROJ-123` identifier is resolved via
/// the by-identifier GET. Empty input is a clean pre-API error; a well-formed
/// but nonexistent identifier surfaces as the GET's not-found error, and the
/// server owns cross-workspace/cross-project validation of the final `parent`.
async fn resolve_parent_id(client: &ApiClient, parent: &str) -> Result<String, CliError> {
    let trimmed = parent.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--parent must not be empty"));
    }
    if looks_like_uuid(trimmed) {
        Ok(trimmed.to_string())
    } else {
        Ok(resolve_issue(client, trimmed).await?.id)
    }
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

/// Convert the CLI's plain-text/markdown `--description` into the minimal HTML
/// the web API stores in `description_html`.
///
/// Mirrors the server-side renderer
/// (`apps/api/pi_dash/assistant/runtime/markdown.py::markdown_to_html`) so a
/// description filed via the CLI reads identically to one filed by the in-app AI
/// assistant: a blank line starts a new paragraph, a single newline becomes a
/// `<br/>`, and empty input yields the model's default empty body. The server
/// re-sanitizes the result via `validate_html_content`, so this only needs to be
/// correct, not defensive.
fn description_to_html(body: &str) -> String {
    let paragraphs: Vec<String> = body
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", html_escape(p).replace('\n', "<br/>")))
        .collect();
    if paragraphs.is_empty() {
        return "<p></p>".to_string();
    }
    paragraphs.join("")
}

/// Escape the five HTML-significant characters, matching Python's
/// `html.escape(s, quote=True)` used by the server renderer.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// The `parent`-field mutation requested by a `pidash issue patch`.
///
/// `clap`'s `conflicts_with` guarantees `--parent` and `--clear-parent` are
/// never both present, so these three variants are exhaustive.
enum ParentPatch {
    /// `--parent` given; the resolved parent issue UUID.
    Set(String),
    /// `--clear-parent` given; detach the parent by sending `parent: null`.
    Clear,
    /// Neither flag given; leave the parent untouched.
    Unchanged,
}

async fn cmd_patch(client: &ApiClient, args: PatchArgs) -> Result<(), CliError> {
    // Resolve issue first — we always need project_id for the mutating PATCH URL.
    let issue = resolve_issue(client, &args.identifier).await?;

    let state_uuid = match args.state.as_deref() {
        Some(state) if looks_like_uuid(state) => Some(state.to_string()),
        Some(state) => Some(resolve_state_name(client, &issue.project_id, state).await?),
        None => None,
    };

    let parent = if args.clear_parent {
        ParentPatch::Clear
    } else if let Some(ref parent) = args.parent {
        ParentPatch::Set(resolve_parent_id(client, parent).await?)
    } else {
        ParentPatch::Unchanged
    };

    let body = build_patch_body(
        args.title.as_deref(),
        args.description.as_deref(),
        args.priority.as_deref(),
        state_uuid,
        parent,
    )?;

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

/// Assemble the `work-items` PATCH body from already-resolved values, enforcing
/// the "at least one mutation" guard. Kept pure (no network) so the body
/// contract — including `parent: null` for `--clear-parent` and the fact that a
/// parent mutation satisfies the guard — is unit-testable.
fn build_patch_body(
    title: Option<&str>,
    description: Option<&str>,
    priority: Option<&str>,
    state_uuid: Option<String>,
    parent: ParentPatch,
) -> Result<Map<String, Value>, CliError> {
    let mut body: Map<String, Value> = Map::new();

    if let Some(title) = title {
        body.insert("name".into(), Value::String(title.to_string()));
    }
    if let Some(desc) = description {
        // Same rich-text contract as `build_create_body`: the server stores the
        // body in `description_html`, and the PATCH view keys the
        // description-version bookkeeping off `request.data.get("description_html")`.
        // Convert the plain-text/markdown input and send it under that key; the
        // model re-derives `description_stripped` and the serializer re-sanitizes
        // the HTML on save.
        body.insert(
            "description_html".into(),
            Value::String(description_to_html(desc)),
        );
    }
    if let Some(prio) = priority {
        body.insert("priority".into(), Value::String(prio.to_string()));
    }
    if let Some(uuid) = state_uuid {
        body.insert("state".into(), Value::String(uuid));
    }
    match parent {
        ParentPatch::Set(uuid) => {
            body.insert("parent".into(), Value::String(uuid));
        }
        ParentPatch::Clear => {
            body.insert("parent".into(), Value::Null);
        }
        ParentPatch::Unchanged => {}
    }

    if body.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "at least one of --state/--title/--description/--priority/--parent/--clear-parent is required",
        ));
    }

    Ok(body)
}

async fn cmd_attach_review(client: &ApiClient, args: AttachReviewArgs) -> Result<(), CliError> {
    let url = args.url.trim();
    if url.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--url must not be empty"));
    }

    // Resolve issue first — we need project_id for the work-item-scoped URL.
    let issue = resolve_issue(client, &args.identifier).await?;

    let mut body: Map<String, Value> = Map::new();
    body.insert("url".into(), Value::String(url.to_string()));

    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/code-reviews/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.post(&path, &Value::Object(body)).await?;
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

async fn cmd_re_tick(client: &ApiClient, identifier: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/re-tick/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    // The server applies the guardrails (ticking state + exhausted budget)
    // and reports `granted: false` with a reason when nothing changed, so a
    // no-op is a normal 200 rather than an error.
    let resp = client.post(&path, &serde_json::json!({})).await?;
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
    fn description_to_html_empty_yields_empty_paragraph() {
        // Matches the model's default empty body so `--description ""` clears it.
        assert_eq!(description_to_html(""), "<p></p>");
        assert_eq!(description_to_html("   \n  "), "<p></p>");
    }

    #[test]
    fn description_to_html_single_paragraph() {
        assert_eq!(description_to_html("hello world"), "<p>hello world</p>");
    }

    #[test]
    fn description_to_html_blank_line_splits_paragraphs() {
        assert_eq!(
            description_to_html("first para\n\nsecond para"),
            "<p>first para</p><p>second para</p>"
        );
    }

    #[test]
    fn description_to_html_single_newline_becomes_br() {
        assert_eq!(
            description_to_html("line one\nline two"),
            "<p>line one<br/>line two</p>"
        );
    }

    #[test]
    fn description_to_html_escapes_html_significant_chars() {
        // Without escaping, `<`/`&` would corrupt the stored HTML (or be stripped
        // by the server sanitizer), which is exactly how descriptions went missing.
        assert_eq!(
            description_to_html("a < b && c > d \"q\" 'x'"),
            "<p>a &lt; b &amp;&amp; c &gt; d &quot;q&quot; &#x27;x&#x27;</p>"
        );
    }

    #[test]
    fn description_to_html_collapses_extra_blank_lines_and_trims() {
        assert_eq!(description_to_html("  a  \n\n\n  b  "), "<p>a</p><p>b</p>");
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

    // --- parent support --------------------------------------------------

    #[test]
    fn build_create_body_includes_resolved_parent() {
        let body = build_create_body(
            "Sub-task".to_string(),
            None,
            None,
            None,
            Some("11111111-2222-3333-4444-555555555555".to_string()),
        );
        assert_eq!(
            body.get("parent").and_then(Value::as_str),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(body.get("name").and_then(Value::as_str), Some("Sub-task"));
    }

    #[test]
    fn build_create_body_omits_parent_when_absent() {
        let body = build_create_body("Top-level".to_string(), None, None, None, None);
        assert!(!body.contains_key("parent"));
    }

    #[test]
    fn build_patch_body_sets_parent_uuid() {
        let body = build_patch_body(
            None,
            None,
            None,
            None,
            ParentPatch::Set("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string()),
        )
        .expect("parent is a valid mutation");
        assert_eq!(
            body.get("parent").and_then(Value::as_str),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
    }

    #[test]
    fn build_patch_body_clear_parent_sends_null() {
        let body = build_patch_body(None, None, None, None, ParentPatch::Clear)
            .expect("clear-parent is a valid mutation");
        // `--clear-parent` must emit an explicit JSON null (detach), not omit
        // the key — omitting it would leave the parent unchanged server-side.
        assert_eq!(body.get("parent"), Some(&Value::Null));
    }

    #[test]
    fn build_patch_body_parent_alone_satisfies_guard() {
        // A parent mutation with no other flag must not trip the
        // "at least one of ..." guard.
        assert!(
            build_patch_body(None, None, None, None, ParentPatch::Set("x".to_string())).is_ok()
        );
        assert!(build_patch_body(None, None, None, None, ParentPatch::Clear).is_ok());
    }

    #[test]
    fn build_patch_body_empty_is_rejected() {
        let err = build_patch_body(None, None, None, None, ParentPatch::Unchanged)
            .expect_err("no mutations must be rejected");
        assert_eq!(err.exit_code, EXIT_INVALID);
        // The guard message advertises the parent flags so agents can discover them.
        assert!(err.message.contains("--parent"));
        assert!(err.message.contains("--clear-parent"));
    }

    /// Parse `pidash issue patch` args in isolation. `PatchArgs` derives
    /// `Args`, not `Parser`, so wrap it in a throwaway `Parser` to exercise the
    /// `conflicts_with` relationship the way clap enforces it at runtime.
    #[derive(Debug, clap::Parser)]
    struct PatchArgsHarness {
        #[command(flatten)]
        args: PatchArgs,
    }

    #[test]
    fn patch_parent_and_clear_parent_conflict() {
        use clap::Parser;
        let err = PatchArgsHarness::try_parse_from([
            "patch",
            "PROJ-1",
            "--parent",
            "PROJ-2",
            "--clear-parent",
        ])
        .expect_err("--parent and --clear-parent are mutually exclusive");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn patch_parent_alone_parses() {
        use clap::Parser;
        let parsed =
            PatchArgsHarness::try_parse_from(["patch", "PROJ-1", "--parent", "PROJ-2"]).unwrap();
        assert_eq!(parsed.args.parent.as_deref(), Some("PROJ-2"));
        assert!(!parsed.args.clear_parent);
    }

    #[test]
    fn patch_clear_parent_alone_parses() {
        use clap::Parser;
        let parsed =
            PatchArgsHarness::try_parse_from(["patch", "PROJ-1", "--clear-parent"]).unwrap();
        assert!(parsed.args.clear_parent);
        assert!(parsed.args.parent.is_none());
    }
}
