//! Local tool executors: the 17 `assistant/tools/*.py` tools as
//! `ToolStore` operations (PIDASHCONV-1113).
//!
//! Each executor mirrors its Python tool line for line: scope checks
//! first (member project, scoped issue, write gate), then validation,
//! then the store calls, then the shaped result. Every `ModelRetry`
//! the tools raise becomes [`ToolFailure::Retry`] with the identical
//! message; arg validation failures become
//! [`ToolFailure::InvalidArgs`] with the identical `ErrorDetails` list;
//! everything that escapes a Python tool (missing user/thread rows,
//! malformed project/state ids, DB and queue failures) becomes
//! [`ToolFailure::Internal`], which fails the turn rather than
//! retrying. PIDASHCONV-863 maps these onto `ToolReturnPart` /
//! `RetryPromptPart` / turn failure.
//!
//! Pure shaping, validation and SQL fragments come from the services
//! ports (`tools_issues`, `tools_projects`, `tools_comments`,
//! `tools_runs`, `tools_github`, `tools_results`, `tools_scoping`,
//! `markdown`); the executors add the control flow and the store calls.
//! [`ToolStore`](super::tools_local_store::ToolStore) is the data seam.

use serde_json::{json, Value};
use uuid::Uuid;

use super::tool_args;
use super::tool_schemas;
use super::tools_local_store::{IssueRow, PrFetch, ToolEnv, ToolStore};
use pidash_services::assistant::agent::AssistantDeps;
use pidash_services::assistant::tools_issues::ToolError;
use pidash_services::assistant::tools_scoping::ToolScopeError;

// ---------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------

/// How a tool call ended. The 863 loop renders `Ok` as a `ToolReturnPart`
/// (content = the value as-is), `Retry`/`InvalidArgs` as a
/// `RetryPromptPart` (content = the message / details list), and
/// `Internal` as a turn failure.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolFailure {
    /// A `ModelRetry` message, byte-identical to Python's.
    Retry { message: String },
    /// Pydantic arg-validation details (`list[ErrorDetails]`).
    InvalidArgs { details: Value },
    /// Anything escaping the Python tool: fails the turn, never retries.
    Internal { message: String },
}

impl ToolFailure {
    pub fn retry(message: impl Into<String>) -> Self {
        ToolFailure::Retry {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        ToolFailure::Internal {
            message: message.into(),
        }
    }
}

impl From<ToolError> for ToolFailure {
    fn from(error: ToolError) -> Self {
        ToolFailure::retry(error.message())
    }
}

impl From<ToolScopeError> for ToolFailure {
    fn from(error: ToolScopeError) -> Self {
        debug_assert!(error.is_model_retry(), "scope errors always retry");
        ToolFailure::retry(error.message())
    }
}

impl From<super::tools_local_store::StoreError> for ToolFailure {
    fn from(error: super::tools_local_store::StoreError) -> Self {
        ToolFailure::internal(error.message())
    }
}

/// Django's `ValidationError` rendering for a malformed UUID id
/// (`['“<value>” is not a valid UUID.']`, `django/core/exceptions.py`,
/// probed on the vendored Django): `_scoping.get_project` and
/// `_resolve_state` do not catch it, so a malformed project/state id
/// fails the turn internal (unlike `_scoping.get_issue`, which
/// translates to `ToolNotFound`).
pub fn invalid_uuid_message(raw: &str) -> String {
    format!("['\u{201c}{raw}\u{201d} is not a valid UUID.']")
}

// ---------------------------------------------------------------------------
// Reference classification (`uuid.UUID` grammar + `PROJ-123` split)
// ---------------------------------------------------------------------------

/// Parse a token the way `uuid.UUID(str)` does (CPython 3.12
/// `uuid.py:175-179`, probed): strip every lowercase `urn:`/`uuid:`,
/// strip `{}` from both ends, drop all hyphens, require 32 hex digits
/// (single underscores between digits allowed, `int(x, 16)` rules), and
/// canonicalize. Returns the canonical hyphenated lowercase form.
pub fn parse_uuid_token(raw: &str) -> Option<Uuid> {
    let no_urn = raw.replace("urn:", "").replace("uuid:", "");
    let stripped = no_urn.trim_matches(|ch| ch == '{' || ch == '}');
    let compact: String = stripped.chars().filter(|ch| *ch != '-').collect();
    if compact.len() != 32 {
        return None;
    }
    let bytes = compact.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'_' {
            let prev_ok = index > 0 && bytes[index - 1] != b'_';
            let next_ok = index + 1 < bytes.len() && bytes[index + 1] != b'_';
            if !(prev_ok && next_ok) {
                return None;
            }
        } else if !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    let hex: String = bytes
        .iter()
        .filter(|b| **b != b'_')
        .map(|b| *b as char)
        .collect();
    // `Uuid::parse_str` needs exactly 32 hex digits, but Python's
    // `int(hex, 16)` accepts the 31-digit remainder when one
    // underscore was stripped (a small value with a leading zero
    // nibble), so parse as a number. The loop above already
    // restricted every char to hex-or-single-underscore, so this
    // cannot fail on reachable inputs.
    u128::from_str_radix(&hex, 16).ok().map(Uuid::from_u128)
}

/// A `resolve_refs` reference (`relations.py:138-168`): a UUID id, a
/// `PROJ-123` identifier (split at the last `-`, ASCII-digit tail —
/// the documented ASCII limitation, matching
/// `search_sequence_tokens`), or unresolved (never an error here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueRef {
    Id(Uuid),
    Identifier {
        project_code: String,
        sequence_id: i64,
    },
    Unresolved,
}

pub fn classify_issue_ref(raw: &str) -> IssueRef {
    use pidash_services::assistant::tools_issues::py_strip;
    let reference = py_strip(raw);
    if reference.is_empty() {
        return IssueRef::Unresolved;
    }
    if let Some(id) = parse_uuid_token(reference) {
        return IssueRef::Id(id);
    }
    match reference.rsplit_once('-') {
        Some((code, tail))
            if !code.is_empty()
                && !tail.is_empty()
                && tail.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            match tail.parse::<i64>() {
                // Beyond i64 the lookup cannot match (the column is
                // int4): Python's query raises (internal) on the
                // out-of-range bind while the port misses (retry) —
                // the documented retry-vs-internal deviation. In-range
                // tails keep exact parity via the `::int` casts.
                Ok(sequence_id) => IssueRef::Identifier {
                    project_code: code.to_owned(),
                    sequence_id,
                },
                Err(_) => IssueRef::Unresolved,
            }
        }
        _ => IssueRef::Unresolved,
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Python `repr()` for a short string (the unknown-tool message,
/// `tool_manager.py:369`): single quotes, backslash/quote/control
/// escapes — or double quotes, unescaped `'`, when the value contains
/// a `'` but no `"` (CPython `unicode_repr` quote choice). Tool names
/// from models are identifiers in practice; the exotic arms
/// (non-printables) render raw rather than `\x`-escaped — the only
/// deliberate gap, unreachable from real tool calls.
fn py_repr_str(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' if !use_double => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// `Unknown tool name: {name!r}. Available tools: {...}`
/// (`tool_manager.py:364-369`): names sorted, each `repr()`'d,
/// comma-space joined.
pub fn unknown_tool_message(name: &str) -> String {
    let available: Vec<String> = tool_schemas::tool_names_sorted()
        .iter()
        .map(|tool| py_repr_str(tool))
        .collect();
    format!(
        "Unknown tool name: {}. Available tools: {}",
        py_repr_str(name),
        available.join(", ")
    )
}

/// Run one local tool by name over `args` (the model's JSON object):
/// validate args (lax, pydantic-identical), dispatch to the executor,
/// and return the result value or the model/turn failure.
pub async fn execute_local_tool<S: ToolStore>(
    store: &S,
    deps: &mut AssistantDeps,
    env: &ToolEnv,
    tool_name: &str,
    args: &Value,
) -> Result<Value, ToolFailure> {
    let Some(spec) = tool_args::tool_spec(tool_name) else {
        return Err(ToolFailure::retry(unknown_tool_message(tool_name)));
    };
    // Validate once for the shared failure shape; the typed structs
    // re-validate (cheap, infallible the second time) for access.
    if let Err(errors) = tool_args::validate_tool_args(spec, args) {
        return Err(ToolFailure::InvalidArgs {
            details: Value::Array(errors.iter().map(tool_args::ArgError::to_value).collect()),
        });
    }
    match tool_name {
        "search_issues" => {
            execute_search_issues(
                store,
                deps,
                env,
                &tool_args::SearchIssuesArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_issues" => {
            execute_list_issues(
                store,
                deps,
                env,
                &tool_args::ListIssuesArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_my_issues" => {
            execute_list_my_issues(
                store,
                deps,
                env,
                &tool_args::ListMyIssuesArgs::parse(args).expect("validated"),
            )
            .await
        }
        "get_issue" => {
            execute_get_issue(
                store,
                deps,
                env,
                &tool_args::GetIssueArgs::parse(args).expect("validated"),
            )
            .await
        }
        "create_issue" => {
            execute_create_issue(
                store,
                deps,
                env,
                &tool_args::CreateIssueArgs::parse(args).expect("validated"),
            )
            .await
        }
        "update_issue" => {
            execute_update_issue(
                store,
                deps,
                env,
                &tool_args::UpdateIssueArgs::parse(args).expect("validated"),
            )
            .await
        }
        "relate_issues" => {
            execute_relate_issues(
                store,
                deps,
                env,
                &tool_args::RelateIssuesArgs::parse(args).expect("validated"),
            )
            .await
        }
        "unrelate_issues" => {
            execute_unrelate_issues(
                store,
                deps,
                env,
                &tool_args::RelateIssuesArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_issue_relations" => {
            execute_list_issue_relations(
                store,
                deps,
                env,
                &tool_args::IssueRefArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_projects" => execute_list_projects(store, deps, env).await,
        "list_states" => {
            execute_list_states(
                store,
                deps,
                env,
                &tool_args::ProjectIdArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_labels" => {
            execute_list_labels(
                store,
                deps,
                env,
                &tool_args::ProjectIdArgs::parse(args).expect("validated"),
            )
            .await
        }
        "list_project_members" => {
            execute_list_project_members(
                store,
                deps,
                env,
                &tool_args::ProjectIdArgs::parse(args).expect("validated"),
            )
            .await
        }
        "create_comment" => {
            execute_create_comment(
                store,
                deps,
                env,
                &tool_args::CreateCommentArgs::parse(args).expect("validated"),
            )
            .await
        }
        "get_run_status" => {
            execute_get_run_status(
                store,
                deps,
                env,
                &tool_args::IssueIdArgs::parse(args).expect("validated"),
            )
            .await
        }
        "dispatch_coding_run" => {
            execute_dispatch_coding_run(
                store,
                deps,
                env,
                &tool_args::DispatchRunArgs::parse(args).expect("validated"),
            )
            .await
        }
        "get_pull_request_status" => {
            execute_get_pull_request_status(
                store,
                deps,
                env,
                &tool_args::PrStatusArgs::parse(args).expect("validated"),
            )
            .await
        }
        _ => Err(ToolFailure::retry(unknown_tool_message(tool_name))),
    }
}

// ---------------------------------------------------------------------------
// Shared executor plumbing
// ---------------------------------------------------------------------------

/// Saturate a validated (possibly absurd) int for the i64 helpers: the
/// saturation preserves every clamp outcome (`clamp_limit`,
/// `clamp_offset`), and `next_offset` saturates rather than rendering
/// beyond i64 (documented on `coerce_int`).
fn saturate_i64(value: i128) -> i64 {
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// `_scoping.get_project` (`_scoping.py:58-64`): member-scope hit, else
/// `ToolNotFound` — and a malformed id fails the turn internal (the
/// uncaught `ValidationError`, see [`invalid_uuid_message`]).
async fn executor_get_project<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    project_id: &str,
) -> Result<super::tools_local_store::ProjectRow, ToolFailure> {
    let Some(id) = parse_uuid_token(project_id) else {
        return Err(ToolFailure::internal(invalid_uuid_message(project_id)));
    };
    store
        .member_project(deps, &id)
        .await?
        .ok_or_else(|| ToolScopeError::project_not_found(project_id).into())
}

/// `_scoping.get_issue` (`_scoping.py:67-85`): malformed ids and misses
/// both retry as `ToolNotFound`.
async fn executor_get_issue<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    issue_id: &str,
) -> Result<IssueRow, ToolFailure> {
    let Some(id) = parse_uuid_token(issue_id) else {
        return Err(ToolScopeError::issue_not_found(issue_id).into());
    };
    store
        .scoped_issue(deps, &id)
        .await?
        .ok_or_else(|| ToolScopeError::issue_not_found(issue_id).into())
}

/// `_scoping.require_project_write` (`_scoping.py:104-107`).
async fn executor_require_project_write<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    project_id: &Uuid,
) -> Result<(), ToolFailure> {
    if store.check_project_write(deps, project_id).await? {
        Ok(())
    } else {
        Err(ToolScopeError::write_denied().into())
    }
}

/// `require_user` (`_scoping.user_for`): the actor row, missing → internal.
async fn executor_actor<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
) -> Result<Uuid, ToolFailure> {
    Ok(store.require_user(deps).await?)
}

/// `issue_link` for one row (`_results.issue_link`).
fn row_link(deps: &AssistantDeps, row: &IssueRow) -> Value {
    pidash_services::assistant::tools_results::issue_link(
        &deps.workspace_slug,
        &row.project_id.to_string(),
        &row.id.to_string(),
    )
}

/// Brief shape for one row (`issues._brief`).
fn row_brief(row: &IssueRow) -> Value {
    use pidash_services::assistant::tools_issues::{brief_row, BriefRow};
    let id = row.id.to_string();
    let project_id = row.project_id.to_string();
    let parent_id = row.parent_id.map(|id| id.to_string());
    brief_row(&BriefRow {
        id: &id,
        project_id: &project_id,
        project_identifier: &row.project_identifier,
        sequence_id: row.sequence_id,
        name: &row.name,
        parent_id: parent_id.as_deref(),
        state_name: row.state_name.as_deref(),
        state_group: row.state_group.as_deref(),
        priority: &row.priority,
    })
}

/// Paginated `{"results", "has_more", "next_offset"}` over a lookahead
/// window (`issues.py:74-80,124-130,146-152`).
fn page_result(rows: Vec<IssueRow>, limit: i64, offset: i64) -> Value {
    use pidash_services::assistant::tools_issues::page_window;
    let (has_more, next_offset) = page_window(rows.len(), limit, offset);
    let issues: Vec<Value> = rows.iter().take(limit as usize).map(row_brief).collect();
    json!({
        "results": issues,
        "has_more": has_more,
        "next_offset": next_offset,
    })
}

/// `_resolve_state` over the store's states (`issues._resolve_state`),
/// with the malformed-id internal (the uncaught `ValidationError`).
async fn executor_resolve_state<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    project_id: &Uuid,
    state_id: Option<&str>,
) -> Result<Option<super::tools_local_store::StateRow>, ToolFailure> {
    if let Some(wanted) = state_id {
        if !wanted.is_empty() && parse_uuid_token(wanted).is_none() {
            return Err(ToolFailure::internal(invalid_uuid_message(wanted)));
        }
    }
    let states = store.project_states(deps, project_id).await?;
    // The `resolve_state` rules (`issues.py:185-192`) over owned rows:
    // an explicit id must hit (comparing canonically — the queryset
    // compares UUID objects, so an exotic-but-valid spelling still
    // hits), else the default state, else the lowest-`sequence` state,
    // else None when the project has no states.
    if let Some(raw) = state_id {
        if !raw.is_empty() {
            let wanted = parse_uuid_token(raw)
                .map(|id| id.to_string())
                .unwrap_or_default();
            return states
                .iter()
                .find(|state| state.id.to_string() == wanted)
                .cloned()
                .map(Some)
                .ok_or_else(|| ToolError::invalid_state(raw).into());
        }
    }
    Ok(states
        .iter()
        .find(|state| state.default)
        .or_else(|| {
            states
                .iter()
                .min_by(|a, b| a.sequence.total_cmp(&b.sequence))
        })
        .cloned())
}

/// Resolve one UUID-or-identifier ref inside the member scope
/// (`_resolve_issue_refs`, `issues.py:394-408`).
async fn executor_resolve_ref<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    raw: &str,
) -> Result<IssueRow, ToolFailure> {
    use pidash_services::assistant::tools_issues::py_strip;
    let reference = py_strip(raw).to_owned();
    let found = match classify_issue_ref(&reference) {
        IssueRef::Id(id) => store.scoped_issue(deps, &id).await?,
        IssueRef::Identifier {
            project_code,
            sequence_id,
        } => {
            store
                .scoped_issue_by_identifier(deps, &project_code, sequence_id)
                .await?
        }
        IssueRef::Unresolved => None,
    };
    found.ok_or_else(|| {
        // `ref or str(raw)` (`relations.py:160`): padded refs report
        // stripped, but whitespace-only originals report verbatim.
        let shown = if reference.is_empty() {
            raw
        } else {
            reference.as_str()
        };
        ToolFailure::retry(
            pidash_services::assistant::tools_issues::unresolved_refs_message(&[shown]),
        )
    })
}

// ---------------------------------------------------------------------------
// Read executors (`issues.py`, `projects.py`, `runs.get_run_status`)
// ---------------------------------------------------------------------------

/// `search_issues` (`issues.py:56-80`).
pub async fn execute_search_issues<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::SearchIssuesArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::{clamp_limit, clamp_offset};
    // `scoped_issues` → `user_for` is the first statement
    // (`issues.py:62`, `_scoping.py:65`): a missing user fails the
    // turn internal before the project scope check.
    executor_actor(store, deps).await?;
    let limit = clamp_limit(saturate_i64(args.limit));
    let offset = clamp_offset(saturate_i64(args.offset));
    let project_id = match &args.project_id {
        Some(raw) if !raw.is_empty() => Some(executor_get_project(store, deps, raw).await?.id),
        // `if project_id:` — empty string skips the narrowing (and the
        // scope check) exactly like None.
        _ => None,
    };
    let rows = store
        .search_issues_page(deps, project_id, &args.query, limit, offset)
        .await?;
    Ok(page_result(rows, limit, offset))
}

/// `_resolve_states` (`issue_filters.py:503-520`): UUID tokens pass
/// through (canonicalized for binding); names resolve case-insensitively
/// in the project; unknown names retry with the sorted valid list.
fn resolve_state_tokens(
    tokens: &[String],
    states: &[super::tools_local_store::StateRow],
) -> Result<Vec<String>, ToolFailure> {
    use pidash_services::assistant::tools_issues::valid_name_list;
    let mut ids = Vec::new();
    let mut unknown = Vec::new();
    for token in tokens {
        if let Some(id) = parse_uuid_token(token) {
            ids.push(id.to_string());
            continue;
        }
        let hits: Vec<&super::tools_local_store::StateRow> = states
            .iter()
            .filter(|state| state.name.to_lowercase() == token.to_lowercase())
            .collect();
        if hits.is_empty() {
            unknown.push(token.clone());
        } else {
            ids.extend(hits.iter().map(|state| state.id.to_string()));
        }
    }
    if !unknown.is_empty() {
        // Python joins a *set* (deduped); the port's list sorts
        // deterministically (see `valid_name_list`). Unlike the
        // labels branch, the states branch has no `or "(none)"`
        // (`issue_filters.py:515` vs `:543`): a project with no
        // states renders `Valid states: .`.
        let mut names: Vec<&str> = states.iter().map(|state| state.name.as_str()).collect();
        names.sort();
        names.dedup();
        let valid = if names.is_empty() {
            String::new()
        } else {
            valid_name_list(&names)
        };
        return Err(ToolFailure::retry(format!(
            "Unknown state name(s): {}. Valid states: {}.",
            unknown.join(", "),
            valid
        )));
    }
    Ok(ids)
}

/// `_resolve_labels` (`issue_filters.py:522-549`): UUIDs pass through;
/// names resolve against project + workspace-level (project null)
/// labels; unknown names retry.
fn resolve_label_tokens(
    tokens: &[String],
    project_labels: &[super::tools_local_store::LabelRow],
    workspace_labels: &[super::tools_local_store::LabelRow],
) -> Result<Vec<String>, ToolFailure> {
    use pidash_services::assistant::tools_issues::valid_name_list;
    let mut ids = Vec::new();
    let mut unknown = Vec::new();
    for token in tokens {
        if let Some(id) = parse_uuid_token(token) {
            ids.push(id.to_string());
            continue;
        }
        let hits: Vec<&super::tools_local_store::LabelRow> = project_labels
            .iter()
            .chain(workspace_labels.iter())
            .filter(|label| label.name.to_lowercase() == token.to_lowercase())
            .collect();
        if hits.is_empty() {
            unknown.push(token.clone());
        } else {
            ids.extend(hits.iter().map(|label| label.id.to_string()));
        }
    }
    if !unknown.is_empty() {
        let mut names: Vec<&str> = project_labels
            .iter()
            .chain(workspace_labels.iter())
            .map(|label| label.name.as_str())
            .collect();
        names.sort();
        names.dedup();
        return Err(ToolFailure::retry(format!(
            "Unknown label name(s): {}. Valid labels: {}.",
            unknown.join(", "),
            valid_name_list(&names)
        )));
    }
    Ok(ids)
}

/// `_resolve_parents` (`issue_filters.py:545-569`): UUIDs pass through;
/// `PROJ-123` identifiers resolve against the workspace (a miss retries
/// as unknown); anything else retries as invalid. The digit tail is
/// ASCII-only (the documented `search_sequence_tokens` limitation);
/// beyond-i64 tails miss (retry) where Python's out-of-range bind
/// raises (internal) — the documented retry-vs-internal deviation.
/// In-range tails keep exact parity via the `::int` casts.
async fn resolve_parent_tokens<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    tokens: &[String],
) -> Result<Vec<String>, ToolFailure> {
    let mut ids = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let Some(id) = parse_uuid_token(token) {
            ids.push(id.to_string());
            continue;
        }
        let resolved = match token.rsplit_once('-') {
            Some((code, tail))
                if !code.is_empty()
                    && !tail.is_empty()
                    && tail.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                match tail.parse::<i64>() {
                    Ok(sequence_id) => store
                        .workspace_issue_by_identifier(deps, code, sequence_id)
                        .await?
                        .map(|issue| issue.id.to_string()),
                    Err(_) => None,
                }
            }
            _ => {
                return Err(ToolFailure::retry(format!(
                    "Invalid parent '{token}': expected an issue UUID, an identifier like PROJ-123, or null."
                )));
            }
        };
        match resolved {
            Some(id) => ids.push(id),
            None => {
                return Err(ToolFailure::retry(format!(
                    "Unknown parent issue '{token}'."
                )));
            }
        }
    }
    Ok(ids)
}

/// `list_issues` (`issues.py:83-130`).
pub async fn execute_list_issues<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::ListIssuesArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::{
        clamp_limit, clamp_offset, normalize_list_filters, split_tokens, ListFilterInput,
    };
    let project = executor_get_project(store, deps, &args.project_id).await?;
    let limit = clamp_limit(saturate_i64(args.limit));
    let offset = clamp_offset(saturate_i64(args.offset));
    // `work_item_list_filters` (`issue_filters.py:573-641`) validates
    // in states → groups → priorities → parents → labels order, and
    // the first bad filter wins the retry message. The enum/null half
    // lives in `normalize_list_filters`, so run it once up front with
    // the unresolved parent tokens (groups, priorities and the
    // null-combo; the predicates are discarded), then resolve parents
    // and labels, then normalize again for the real predicates.
    let states = store.project_states(deps, &project.id).await?;
    let state_ids = resolve_state_tokens(&split_tokens(args.state.as_deref()), &states)?;
    let parent_tokens = split_tokens(args.parent_issue_id.as_deref());
    let _ = normalize_list_filters(&ListFilterInput {
        state_ids: Vec::new(),
        state_groups: split_tokens(args.state_group.as_deref()),
        priorities: split_tokens(args.priority.as_deref()),
        parent_tokens: parent_tokens.clone(),
        label_ids: Vec::new(),
    })?;
    // Nulls are detected on the raw tokens (`issue_filters.py:616`),
    // before any resolution: `_resolve_parents` never sees them.
    let has_null_parents = parent_tokens
        .iter()
        .any(|token| matches!(token.to_lowercase().as_str(), "null" | "none"));
    let parent_for_norm = if has_null_parents {
        parent_tokens
    } else {
        resolve_parent_tokens(store, deps, &parent_tokens).await?
    };
    let project_labels = store.project_labels(deps, &project.id).await?;
    let workspace_labels = store.workspace_labels(deps).await?;
    let label_ids = resolve_label_tokens(
        &split_tokens(args.labels.as_deref()),
        &project_labels,
        &workspace_labels,
    )?;
    let filters = normalize_list_filters(&ListFilterInput {
        state_ids,
        state_groups: split_tokens(args.state_group.as_deref()),
        priorities: split_tokens(args.priority.as_deref()),
        parent_tokens: parent_for_norm,
        label_ids,
    })?;
    // `scoped_issues` → `user_for` runs after filter validation and
    // before the page query (`issues.py:113-116`): a missing user
    // fails internal, but bad filters still retry first.
    executor_actor(store, deps).await?;
    let rows = store
        .list_issues_page(deps, &project.id, &filters, limit, offset)
        .await?;
    Ok(page_result(rows, limit, offset))
}

/// `list_my_issues` (`issues.py:133-152`): unknown scopes fall back to
/// `"all"` rather than retrying (`:141-142`).
pub async fn execute_list_my_issues<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::ListMyIssuesArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::{clamp_limit, clamp_offset};
    // `my_issues` → `user_for` (`issues.py:141`, `_scoping.py:68`):
    // the scope fallback below is infallible, so this check first
    // matches Python's order exactly.
    executor_actor(store, deps).await?;
    let scope = match args.scope.as_str() {
        "assigned" | "created" => args.scope.as_str(),
        _ => "all",
    };
    let limit = clamp_limit(saturate_i64(args.limit));
    let offset = clamp_offset(saturate_i64(args.offset));
    let rows = store.my_issues_page(deps, scope, limit, offset).await?;
    Ok(page_result(rows, limit, offset))
}

/// `get_issue` (`issues.py:155-182`): the brief plus the wrapped
/// description and the ten newest comments, reversed to chronological.
pub async fn execute_get_issue<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::GetIssueArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::{
        comment_author, detail_comment_row, detail_shape, truncate, COMMENT_CAP, DESC_CAP,
    };
    // `get_issue` → `scoped_issues` → `user_for` runs before the
    // malformed-id translation (`_scoping.py:73-81`): a missing user
    // fails internal even for a malformed id.
    executor_actor(store, deps).await?;
    let issue = executor_get_issue(store, deps, &args.issue_id).await?;
    let comments = store.issue_comments(&issue.id).await?;
    // Newest-first read, oldest ten kept (`[:10]`), reversed to
    // chronological (`issues.py:161-179`).
    let mut rows: Vec<Value> = comments
        .iter()
        .take(10)
        .map(|comment| {
            let (body, body_truncated) = truncate(
                comment.comment_stripped.as_deref().unwrap_or(""),
                COMMENT_CAP,
            );
            detail_comment_row(
                comment_author(
                    &comment.speaker_type,
                    comment.actor_display_name.as_deref(),
                    comment.actor_email.as_deref(),
                    comment.actor_id.is_some(),
                )
                .as_deref(),
                &body,
                body_truncated,
                &crate::runner_enroll::enroll::render_isoformat(comment.created_at),
            )
        })
        .collect();
    rows.reverse();
    let (description, description_truncated) = truncate(
        issue.description_stripped.as_deref().unwrap_or(""),
        DESC_CAP,
    );
    Ok(detail_shape(
        row_brief(&issue),
        &description,
        description_truncated,
        rows,
    ))
}

/// `list_projects` (`projects.py:17-24`).
pub async fn execute_list_projects<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_projects::project_row;
    let projects = store.member_projects(deps).await?;
    Ok(Value::Array(
        projects
            .iter()
            .map(|project| project_row(&project.id.to_string(), &project.identifier, &project.name))
            .collect(),
    ))
}

/// `list_states` (`projects.py:27-34`).
pub async fn execute_list_states<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::ProjectIdArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_projects::state_row;
    let project = executor_get_project(store, deps, &args.project_id).await?;
    let states = store.project_states(deps, &project.id).await?;
    let mut states = states;
    states.sort_by(|a, b| a.sequence.total_cmp(&b.sequence));
    Ok(Value::Array(
        states
            .iter()
            .map(|state| {
                state_row(
                    &state.id.to_string(),
                    &state.name,
                    &state.group,
                    state.default,
                )
            })
            .collect(),
    ))
}

/// `list_labels` (`projects.py:37-44`).
pub async fn execute_list_labels<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::ProjectIdArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_projects::label_row;
    let project = executor_get_project(store, deps, &args.project_id).await?;
    let labels = store.project_labels(deps, &project.id).await?;
    Ok(Value::Array(
        labels
            .iter()
            .map(|label| label_row(&label.id.to_string(), &label.name))
            .collect(),
    ))
}

/// `list_project_members` (`projects.py:47-65`): rows without a member
/// are skipped, display names fall back to email.
pub async fn execute_list_project_members<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::ProjectIdArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_projects::{member_display_name, member_row};
    let project = executor_get_project(store, deps, &args.project_id).await?;
    let members = store.project_members(deps, &project.id).await?;
    Ok(Value::Array(
        members
            .iter()
            .map(|member| {
                member_row(
                    &member.member_id.to_string(),
                    &member_display_name(member.display_name.as_deref(), member.email.as_deref()),
                    i64::from(member.role),
                )
            })
            .collect(),
    ))
}

/// `get_run_status` (`runs.py:18-34`).
pub async fn execute_get_run_status<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::IssueIdArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_runs::{run_row, run_status_result};
    // `get_issue` → `scoped_issues` → `user_for` runs before the
    // malformed-id translation (`runs.py:21`, `_scoping.py:73-81`).
    executor_actor(store, deps).await?;
    let issue = executor_get_issue(store, deps, &args.issue_id).await?;
    let runs = store.agent_runs(&issue.id).await?;
    let rows: Vec<Value> = runs
        .iter()
        .map(|run| {
            let created_at = run
                .created_at
                .map(crate::runner_enroll::enroll::render_isoformat);
            run_row(&run.id.to_string(), &run.status, created_at.as_deref())
        })
        .collect();
    Ok(run_status_result(&issue.id.to_string(), rows))
}

// ---------------------------------------------------------------------------
// Write executors — issues
// ---------------------------------------------------------------------------

/// `_resolve_parent` (`issues.py:195-224`): scope check, same-project
/// check, and (when `child_id` is set) the self/cycle walk. The walk
/// starts from the parent's in-memory `parent_id` (no query for the
/// first step) and queries one ancestor at a time with the `seen`
/// guard, exactly as Python does.
async fn executor_resolve_parent<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    project_id: &Uuid,
    parent_issue_id: &str,
    child_id: Option<&Uuid>,
) -> Result<IssueRow, ToolFailure> {
    use pidash_services::assistant::tools_issues::check_parent_link;
    let parent = executor_get_issue(store, deps, parent_issue_id).await?;
    if parent.project_id != *project_id {
        return Err(ToolFailure::retry(
            "The parent issue must be in the same project as the child issue.",
        ));
    }
    if let Some(child) = child_id {
        if parent.id == *child {
            return Err(ToolFailure::retry("An issue can't be its own parent."));
        }
        let mut chain: Vec<Option<String>> = vec![parent.parent_id.map(|id| id.to_string())];
        let mut seen: Vec<Uuid> = Vec::new();
        let mut current = parent.parent_id;
        while let Some(ancestor) = current {
            if ancestor == *child || seen.contains(&ancestor) {
                break;
            }
            seen.push(ancestor);
            let next = store.issue_parent_id(&ancestor).await?;
            chain.push(next.map(|id| id.to_string()));
            current = next;
        }
        check_parent_link(
            &project_id.to_string(),
            &parent.project_id.to_string(),
            &parent.id.to_string(),
            Some(&child.to_string()),
            &chain,
        )?;
    }
    Ok(parent)
}

/// `create_issue` (`issues.py:228-284`).
pub async fn execute_create_issue<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::CreateIssueArgs,
) -> Result<Value, ToolFailure> {
    use super::tools_local_store::NewIssueInput;
    use pidash_services::assistant::tools_issues::{
        create_description_html, created_summary, tool_identifier, validate_create_name,
        validate_priority,
    };
    let project = executor_get_project(store, deps, &args.project_id).await?;
    // `require_project_write` hits `user_for` before the role check
    // (`_scoping.py:104-107`): a missing user fails internal even when
    // validation or the role gate would also fail.
    let actor = executor_actor(store, deps).await?;
    executor_require_project_write(store, deps, &project.id).await?;
    let name = validate_create_name(&args.name)?;
    let priority = validate_priority(args.priority.as_deref())?;
    let state = executor_resolve_state(store, deps, &project.id, args.state_id.as_deref()).await?;
    let parent = match &args.parent_issue_id {
        Some(raw) if !raw.is_empty() => {
            Some(executor_resolve_parent(store, deps, &project.id, raw, None).await?)
        }
        _ => None,
    };
    let row = store
        .create_issue_row(
            &NewIssueInput {
                project_id: project.id,
                project_identifier: project.identifier.clone(),
                workspace_id: project.workspace_id,
                name,
                description_html: create_description_html(args.description_md.as_deref()),
                priority,
                state_id: state.as_ref().map(|state| state.id),
                parent_id: parent.as_ref().map(|parent| parent.id),
                created_by: actor,
                created_via: deps.created_via().to_owned(),
            },
            &env.now,
        )
        .await?;
    // The `Issue.save` post-save fire (`signals.py:74-108`): the
    // receiver sees prior state None and the saved state, arms the
    // clock and may dispatch an entry run. A stateless save (no
    // states in the project at all) is a non-transition (`prev ==
    // current`, `:79-80`) and drives nothing. The fire swallows
    // handler failures with the verbatim log line — the save
    // stands — and the tool result carries no run id (`:284`), so
    // the outcome is dropped.
    if let Some(state_id) = row.state_id {
        store
            .drive_create_transition(
                &row.id,
                pidash_services::orchestration::creation::StateView {
                    id: state_id,
                    name: row.state_name.clone().unwrap_or_default(),
                    group: row.state_group.clone().unwrap_or_default(),
                },
                &actor,
                &env.now,
            )
            .await?;
    }
    // The drive may have bounced the issue to a Backlog target
    // (`scheduling.py:1111-1113` mutates the in-memory issue Django
    // briefs at `issues.py:284`): re-read so the brief reports the
    // post-drive row. A miss (a concurrent delete narrowed the scoped
    // read) falls back to the saved row — Django briefs its in-memory
    // issue regardless (PIDASHCONV-1444).
    let row = store.scoped_issue(deps, &row.id).await?.unwrap_or(row);
    let identifier = tool_identifier(&row.project_identifier, row.sequence_id);
    let link = row_link(deps, &row);
    store
        .record_write(
            deps,
            &created_summary(&identifier, &row.name),
            std::slice::from_ref(&link),
            &env.now,
        )
        .await?;
    // `{"created": True, **_brief(issue), "url_path": ...}` (`:284`).
    let mut result = row_brief(&row);
    let map = result.as_object_mut().expect("brief is an object");
    let mut ordered = serde_json::Map::with_capacity(map.len() + 2);
    ordered.insert("created".to_owned(), Value::Bool(true));
    for (key, value) in map.iter() {
        ordered.insert(key.clone(), value.clone());
    }
    ordered.insert(
        "url_path".to_owned(),
        link.get("url_path").cloned().unwrap_or(Value::Null),
    );
    Ok(Value::Object(ordered))
}

/// `update_issue` (`issues.py:287-391`).
///
/// The nothing-to-update check runs on validation *givenness* (`name is
/// None and ... and to_state is None and not parent_touched*, `:320-327`),
/// not on the effective write list — so an update carrying only a
/// whitespace name still proceeds to an audit-only save. That differs
/// from the services `plan_update` (which rejects an empty effective
/// list); the executor mirrors Python, and the follow-up to reconcile
/// the port is tracked separately.
pub async fn execute_update_issue<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::UpdateIssueArgs,
) -> Result<Value, ToolFailure> {
    use super::tools_local_store::IssuePatch;
    use pidash_services::assistant::tools_issues::{
        create_description_html, py_strip, tool_identifier, updated_summary,
        validate_update_priority,
    };
    // `_scoping.get_issue` → `scoped_issues` → `user_for` runs before
    // the malformed-id translation (`_scoping.py:67-85`): a missing user
    // fails internal even for a malformed id.
    let actor = executor_actor(store, deps).await?;
    let issue = executor_get_issue(store, deps, &args.issue_id).await?;
    executor_require_project_write(store, deps, &issue.project_id).await?;
    let priority = validate_update_priority(args.priority.as_deref())?;
    let state_given = args.state_id.is_some();
    let to_state = if state_given {
        executor_resolve_state(store, deps, &issue.project_id, args.state_id.as_deref()).await?
    } else {
        None
    };
    let parent_touched = !matches!(args.parent_issue_id, tool_args::ParentArg::Unset);
    let new_parent = match &args.parent_issue_id {
        tool_args::ParentArg::Unset | tool_args::ParentArg::Null => None,
        tool_args::ParentArg::Value(raw) if raw.is_empty() => None,
        tool_args::ParentArg::Value(raw) => Some(
            executor_resolve_parent(store, deps, &issue.project_id, raw, Some(&issue.id)).await?,
        ),
    };
    if args.name.is_none()
        && args.description_md.is_none()
        && priority.is_none()
        && to_state.is_none()
        && !parent_touched
    {
        // `issues.py:320-327` — note the em dash (U+2014), byte-exact.
        return Err(ToolFailure::retry(
            "Nothing to update \u{2014} provide at least one field to change.",
        ));
    }
    // Effective writes (`issues.py:337-361`): a whitespace-only name is
    // silently ignored (no change recorded, no error).
    let mut changed: Vec<&str> = Vec::new();
    let mut patch = IssuePatch::default();
    if let Some(name) = &args.name {
        if !py_strip(name).is_empty() {
            changed.push("name");
            patch.name = Some(py_strip(name).chars().take(255).collect());
        }
    }
    if let Some(description_md) = &args.description_md {
        changed.push("description");
        patch.description_html = Some(create_description_html(Some(description_md)));
    }
    if let Some(prio) = priority {
        changed.push("priority");
        patch.priority = Some(prio);
    }
    // `if to_state is not None` (`issues.py:351`): a `state_id` of
    // `""` on a stateless project resolves to no state, so the arm is
    // skipped entirely (no write, no `changed` entry) — gating on
    // `state_given` would write `state_id=NULL` and report `state`.
    if to_state.is_some() {
        changed.push("state");
        patch.state_given = true;
        patch.state_id = to_state.as_ref().map(|state| state.id);
    }
    if parent_touched {
        changed.push("parent");
        patch.parent_touched = true;
        patch.parent_id = new_parent.as_ref().map(|parent| parent.id);
    }
    // The resolved target view travels into the fused save: the store
    // locks, saves, and — when the target differs from the locked
    // row's pre-save state — drives the transition on the same
    // transaction (`issues.py:334-381`, one atomic).
    let to_view =
        to_state.as_ref().map(
            |state| pidash_services::orchestration::creation::StateView {
                id: state.id,
                name: state.name.clone(),
                group: state.group.clone(),
            },
        );
    let (row, drive_outcome) = store
        .update_issue_row(&patch, &issue.id, to_view, &actor, &env.now)
        .await?;
    let mut run_id: Option<String> = None;
    if let Some(outcome) = drive_outcome {
        run_id = outcome.created_run_id.map(|id| id.to_string());
    }
    let identifier = tool_identifier(&row.project_identifier, row.sequence_id);
    store
        .record_write(
            deps,
            &updated_summary(&identifier, &changed),
            &[row_link(deps, &row)],
            &env.now,
        )
        .await?;
    // `{"updated": True, "changed": changed, **_brief(issue)}` (`:387`).
    let brief = row_brief(&row);
    let map = brief.as_object().expect("brief is an object");
    let mut ordered = serde_json::Map::with_capacity(map.len() + 3);
    ordered.insert("updated".to_owned(), Value::Bool(true));
    ordered.insert(
        "changed".to_owned(),
        Value::Array(
            changed
                .into_iter()
                .map(|label| Value::String(label.to_owned()))
                .collect(),
        ),
    );
    for (key, value) in map.iter() {
        ordered.insert(key.clone(), value.clone());
    }
    if let Some(run_id) = run_id {
        ordered.insert("dispatched_run_id".to_owned(), Value::String(run_id));
    }
    Ok(Value::Object(ordered))
}

/// The `_relation_write` prefix (`issues.py:422-434`): exactly one
/// source must resolve, write access to its project is required, at
/// least one target must be listed, and every target must resolve —
/// all before any write.
async fn relation_write_prefix<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    issue: &str,
    related_issues: &[String],
) -> Result<(IssueRow, Vec<IssueRow>), ToolFailure> {
    use pidash_services::assistant::tools_issues::EMPTY_RELATED_MESSAGE;
    // `_resolve_issue_refs` → `scoped_issues` → `user_for` runs before
    // source resolution (`issues.py:394-398`): a missing user fails
    // internal even when the source ref is unresolvable. (The actor is
    // re-fetched by the caller afterwards, as Python's explicit
    // `user_for` does.)
    executor_actor(store, deps).await?;
    let source = executor_resolve_ref(store, deps, issue).await?;
    executor_require_project_write(store, deps, &source.project_id).await?;
    if related_issues.is_empty() {
        return Err(ToolFailure::retry(EMPTY_RELATED_MESSAGE));
    }
    let mut targets = Vec::with_capacity(related_issues.len());
    let mut unresolved = Vec::new();
    for raw in related_issues {
        match executor_resolve_ref(store, deps, raw).await {
            Ok(target) => targets.push(target),
            // Only resolution misses (`Retry`) join the unresolved
            // list; store failures propagate instead of masquerading
            // as "not found" (item 21). Each miss reports
            // stripped-or-raw (`ref or str(raw)`, `relations.py:160`).
            Err(ToolFailure::Retry { .. }) => {
                let stripped = pidash_services::assistant::tools_issues::py_strip(raw);
                unresolved.push(if stripped.is_empty() {
                    raw.as_str()
                } else {
                    stripped
                });
            }
            Err(other) => return Err(other),
        }
    }
    if !unresolved.is_empty() {
        return Err(ToolFailure::retry(
            pidash_services::assistant::tools_issues::unresolved_refs_message(&unresolved),
        ));
    }
    Ok((source, targets))
}

/// Current relations from the source's side over live pair rows
/// (`relations.relate :212-218`), as a sorted set.
fn current_relations(
    pairs: &[super::tools_local_store::PairRow],
    viewpoint_id: &Uuid,
) -> std::collections::BTreeSet<String> {
    use pidash_services::assistant::tools_issues::{type_from_viewpoint, RelationRow};
    let viewpoint = viewpoint_id.to_string();
    pairs
        .iter()
        .map(|pair| {
            type_from_viewpoint(
                &RelationRow {
                    issue_id: &pair.issue_id.to_string(),
                    related_issue_id: &pair.related_issue_id.to_string(),
                    stored_type: &pair.relation_type,
                },
                &viewpoint,
            )
            .to_owned()
        })
        .collect()
}

/// `relate_issues` (`issues.py:449-465` over `relations.relate`).
pub async fn execute_relate_issues<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::RelateIssuesArgs,
) -> Result<Value, ToolFailure> {
    use super::tools_local_store::RelationActivity;
    use pidash_services::assistant::tools_issues::{
        check_relation_targets, relate_result, relation_identifier, relation_write_summary,
        stored_edge, tool_identifier, validate_relation_type, RelationEndpoint,
        RELATION_CREATED_ACTIVITY,
    };
    let (source, targets) =
        relation_write_prefix(store, deps, &args.issue, &args.related_issues).await?;
    let actor = executor_actor(store, deps).await?;
    let relation_type = validate_relation_type(&args.relation_type)?;
    let source_endpoint = RelationEndpoint {
        id: source.id.to_string(),
        workspace_id: source.workspace_id.to_string(),
        display: relation_identifier(&source.project_identifier, source.sequence_id),
    };
    let endpoints: Vec<RelationEndpoint> = targets
        .iter()
        .map(|target| RelationEndpoint {
            id: target.id.to_string(),
            workspace_id: target.workspace_id.to_string(),
            display: relation_identifier(&target.project_identifier, target.sequence_id),
        })
        .collect();
    let unique = check_relation_targets(&source_endpoint, &endpoints)?;
    let unique_targets: Vec<&IssueRow> = unique
        .iter()
        .map(|endpoint| {
            targets
                .iter()
                .find(|target| target.id.to_string() == endpoint.id)
                .expect("endpoint came from targets")
        })
        .collect();
    let mut created: Vec<String> = Vec::new();
    let mut created_ids: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut conflicts: Vec<(String, String)> = Vec::new();
    for target in unique_targets {
        let display = relation_identifier(&target.project_identifier, target.sequence_id);
        let (swap_ends, stored) = stored_edge(&relation_type);
        let (issue_id, related_id) = if swap_ends {
            (target.id, source.id)
        } else {
            (source.id, target.id)
        };
        let existing = store.pair_rows(&source.id, &target.id).await?;
        let mut current = current_relations(&existing, &source.id);
        if existing.is_empty() {
            // `relations.relate :198-207`: the INSERT, with the
            // unique-violation race re-reading into the verdict below.
            let inserted = store
                .insert_relation(
                    &issue_id,
                    &related_id,
                    stored,
                    &source.project_id,
                    &source.workspace_id,
                    &actor,
                    &env.now,
                )
                .await?;
            if inserted {
                created.push(display);
                created_ids.push(target.id.to_string());
                continue;
            }
            current =
                current_relations(&store.pair_rows(&source.id, &target.id).await?, &source.id);
        }
        if current.contains(&relation_type) && current.len() == 1 {
            unchanged.push(display);
        } else {
            let existing_relation = current.iter().next().cloned().unwrap_or_default();
            conflicts.push((display, existing_relation));
        }
    }
    // One activity for the whole batch (`relations.relate :220-226`),
    // carrying every created target id — never one per target.
    if !created_ids.is_empty() {
        let mut requested = serde_json::Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.clone()),
        );
        requested.insert(
            "issues".to_owned(),
            Value::Array(
                created_ids
                    .iter()
                    .map(|id| Value::String(id.clone()))
                    .collect(),
            ),
        );
        store
            .enqueue_relation_activity(&RelationActivity {
                activity_type: RELATION_CREATED_ACTIVITY.to_owned(),
                requested_data: Some(
                    pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(
                        requested,
                    )),
                ),
                actor_id: actor,
                issue_id: source.id,
                project_id: source.project_id,
                current_instance: None,
                epoch: env.now.timestamp(),
            })
            .await?;
    }
    if let Some(summary) = relation_write_summary(
        "Related",
        &tool_identifier(&source.project_identifier, source.sequence_id),
        &relation_type,
        &created,
    ) {
        store
            .record_write(deps, &summary, &[row_link(deps, &source)], &env.now)
            .await?;
    }
    let mut result = relate_result(
        &source_endpoint.display,
        &relation_type,
        &created,
        &unchanged,
        &conflicts,
    );
    result.as_object_mut().expect("result is an object").insert(
        "relations".to_owned(),
        relations_view(store, deps, &source).await?,
    );
    Ok(result)
}

/// `unrelate_issues` (`issues.py:468-483` over `relations.unrelate`).
pub async fn execute_unrelate_issues<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::RelateIssuesArgs,
) -> Result<Value, ToolFailure> {
    use super::tools_local_store::RelationActivity;
    use pidash_services::assistant::tools_issues::{
        check_relation_targets, relation_identifier, relation_write_summary, tool_identifier,
        type_from_viewpoint, unrelate_result, validate_relation_type, RelationEndpoint,
        RelationRow, RELATION_DELETED_ACTIVITY,
    };
    let (source, targets) =
        relation_write_prefix(store, deps, &args.issue, &args.related_issues).await?;
    let actor = executor_actor(store, deps).await?;
    let relation_type = validate_relation_type(&args.relation_type)?;
    let source_endpoint = RelationEndpoint {
        id: source.id.to_string(),
        workspace_id: source.workspace_id.to_string(),
        display: relation_identifier(&source.project_identifier, source.sequence_id),
    };
    let endpoints: Vec<RelationEndpoint> = targets
        .iter()
        .map(|target| RelationEndpoint {
            id: target.id.to_string(),
            workspace_id: target.workspace_id.to_string(),
            display: relation_identifier(&target.project_identifier, target.sequence_id),
        })
        .collect();
    let unique = check_relation_targets(&source_endpoint, &endpoints)?;
    let unique_targets: Vec<&IssueRow> = unique
        .iter()
        .map(|endpoint| {
            targets
                .iter()
                .find(|target| target.id.to_string() == endpoint.id)
                .expect("endpoint came from targets")
        })
        .collect();
    let viewpoint = source.id.to_string();
    let mut removed: Vec<String> = Vec::new();
    let mut not_related: Vec<String> = Vec::new();
    for target in unique_targets {
        let display = relation_identifier(&target.project_identifier, target.sequence_id);
        // `relations.unrelate :248`: rows whose *viewpoint* type equals
        // the requested type (reverse-stored twins match too).
        let pairs = store.pair_rows(&source.id, &target.id).await?;
        let matching: Vec<&super::tools_local_store::PairRow> = pairs
            .iter()
            .filter(|pair| {
                type_from_viewpoint(
                    &RelationRow {
                        issue_id: &pair.issue_id.to_string(),
                        related_issue_id: &pair.related_issue_id.to_string(),
                        stored_type: &pair.relation_type,
                    },
                    &viewpoint,
                ) == relation_type
            })
            .collect();
        if matching.is_empty() {
            not_related.push(display);
            continue;
        }
        for pair in matching {
            store.delete_relation(&pair.id, &actor, &env.now).await?;
        }
        removed.push(display.clone());
        // `relations.unrelate :258-265`: per removed target, both the
        // `requested` payload and the `current_instance` twin.
        let mut requested = serde_json::Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.clone()),
        );
        requested.insert(
            "related_issue".to_owned(),
            Value::String(target.id.to_string()),
        );
        let mut current = serde_json::Map::with_capacity(1);
        current.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.clone()),
        );
        store
            .enqueue_relation_activity(&RelationActivity {
                activity_type: RELATION_DELETED_ACTIVITY.to_owned(),
                requested_data: Some(
                    pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(
                        requested,
                    )),
                ),
                actor_id: actor,
                issue_id: source.id,
                project_id: source.project_id,
                current_instance: Some(
                    pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&Value::Object(
                        current,
                    )),
                ),
                epoch: env.now.timestamp(),
            })
            .await?;
    }
    if let Some(summary) = relation_write_summary(
        "Unrelated",
        &tool_identifier(&source.project_identifier, source.sequence_id),
        &relation_type,
        &removed,
    ) {
        store
            .record_write(deps, &summary, &[row_link(deps, &source)], &env.now)
            .await?;
    }
    let mut result = unrelate_result(
        &source_endpoint.display,
        &relation_type,
        &removed,
        &not_related,
    );
    result.as_object_mut().expect("result is an object").insert(
        "relations".to_owned(),
        relations_view(store, deps, &source).await?,
    );
    Ok(result)
}

/// `_relations_view` (`issues.py:411-419` over
/// `relations.grouped_relations`): every live relation grouped by type
/// from the source's side, each item truncated + wrapped. Every type
/// key is always present; unknown stored types are dropped; other
/// ends outside the member scope are left out.
async fn relations_view<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    source: &IssueRow,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::{
        relation_identifier, relation_item, relation_sort_key, type_from_viewpoint, RelationRow,
        GROUP_LIMIT, RELATION_TYPES,
    };
    use std::collections::{BTreeMap, BTreeSet};
    let rows = store
        .grouped_relation_rows(&source.id, &source.workspace_id)
        .await?;
    let viewpoint = source.id.to_string();
    let mut other_by_type: BTreeMap<&str, BTreeSet<Uuid>> = RELATION_TYPES
        .iter()
        .map(|name| (*name, BTreeSet::new()))
        .collect();
    for pair in &rows {
        let other = if pair.issue_id == source.id {
            pair.related_issue_id
        } else {
            pair.issue_id
        };
        let issue_id = pair.issue_id.to_string();
        let related_issue_id = pair.related_issue_id.to_string();
        let relation = type_from_viewpoint(
            &RelationRow {
                issue_id: &issue_id,
                related_issue_id: &related_issue_id,
                stored_type: &pair.relation_type,
            },
            &viewpoint,
        )
        .to_owned();
        if let Some(ids) = other_by_type.get_mut(relation.as_str()) {
            ids.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .values()
        .flat_map(|ids| ids.iter().copied())
        .collect();
    let visible = store
        .visible_grouped_issues(deps, &wanted, &source.workspace_id)
        .await?;
    let by_id: std::collections::HashMap<Uuid, &super::tools_local_store::GroupedIssueRow> =
        visible.iter().map(|issue| (issue.id, issue)).collect();
    let mut grouped = serde_json::Map::with_capacity(RELATION_TYPES.len());
    for name in RELATION_TYPES {
        let mut items: Vec<&super::tools_local_store::GroupedIssueRow> = other_by_type[name]
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .collect();
        items.sort_by_key(|issue| relation_sort_key(&issue.project_identifier, issue.sequence_id));
        let rows: Vec<Value> = items
            .iter()
            .take(GROUP_LIMIT)
            .map(|issue| {
                relation_item(
                    &issue.id.to_string(),
                    &relation_identifier(&issue.project_identifier, issue.sequence_id),
                    &issue.name,
                    issue.state_name.as_deref(),
                    issue.state_group.as_deref(),
                )
            })
            .collect();
        grouped.insert(name.to_owned(), Value::Array(rows));
    }
    Ok(Value::Object(grouped))
}

/// `list_issue_relations` (`issues.py:483-490`).
pub async fn execute_list_issue_relations<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    _env: &ToolEnv,
    args: &tool_args::IssueRefArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_issues::tool_identifier;
    // `_resolve_issue_refs` → `scoped_issues` → `user_for`
    // (`issues.py:402-406`): same missing-user-internal as the other
    // read paths (item 7 also covers this path).
    executor_actor(store, deps).await?;
    let (source,) = (executor_resolve_ref(store, deps, &args.issue).await?,);
    Ok(json!({
        "issue": tool_identifier(&source.project_identifier, source.sequence_id),
        "relations": relations_view(store, deps, &source).await?,
    }))
}

// ---------------------------------------------------------------------------
// Write executors — comments, runs, github
// ---------------------------------------------------------------------------

/// `create_comment` (`comments.py:32-77`).
pub async fn execute_create_comment<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::CreateCommentArgs,
) -> Result<Value, ToolFailure> {
    use super::tools_local_store::NewCommentInput;
    use pidash_services::assistant::tools_comments::{
        comment_result, comment_summary, guest_denial, guest_may_comment, validate_body,
    };
    // `_scoping.get_issue` → `scoped_issues` → `user_for` is the first
    // statement (`comments.py:37`, `_scoping.py:67-85`): a missing user
    // fails internal before the guest gate and the body check.
    let actor = executor_actor(store, deps).await?;
    let issue = executor_get_issue(store, deps, &args.issue_id).await?;
    // The guest gate reads the issue's project row (`comments.py:39`).
    // The issue is scoped, so its project is a member project; a TOCTOU
    // miss retries as the issue being inaccessible.
    let project = store
        .member_project(deps, &issue.project_id)
        .await?
        .ok_or_else(|| ToolScopeError::issue_not_found(&args.issue_id))?;
    let created_by = issue
        .created_by_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    if !guest_may_comment(
        i64::from(deps.workspace_role),
        project.guest_view_all_features,
        &created_by,
        &deps.user_id.to_string(),
    ) {
        return Err(guest_denial().into());
    }
    let body = validate_body(Some(args.body_md.as_str()))?;
    let comment_id = store
        .create_comment_row(
            &NewCommentInput {
                issue_id: issue.id,
                project_id: issue.project_id,
                workspace_id: issue.workspace_id,
                actor_id: actor,
                html: pidash_services::assistant::markdown::to_safe_html(Some(body)),
                fold: args.fold,
            },
            &env.now,
        )
        .await?;
    store
        .record_write(
            deps,
            &comment_summary(&issue.project_identifier, issue.sequence_id),
            &[row_link(deps, &issue)],
            &env.now,
        )
        .await?;
    Ok(comment_result(
        &comment_id.to_string(),
        &issue.id.to_string(),
        args.fold,
    ))
}

/// `dispatch_coding_run` (`runs.py:37-96`).
pub async fn execute_dispatch_coding_run<S: ToolStore>(
    store: &S,
    deps: &AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::DispatchRunArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_runs::{
        already_in_state_result, dispatch_success_result, dispatch_summary, invalid_state_error,
        is_ticking_state, no_delegation_state_result, non_ticking_state_error, PHASES,
    };
    // `_scoping.get_issue` → `scoped_issues` → `user_for` is the first
    // statement (`runs.py:41`, `_scoping.py:67-85`): a missing user fails
    // internal before target resolution.
    let actor = executor_actor(store, deps).await?;
    let issue = executor_get_issue(store, deps, &args.issue_id).await?;
    executor_require_project_write(store, deps, &issue.project_id).await?;
    let states = store.project_states(deps, &issue.project_id).await?;
    let target = match args.target_state_id.as_deref() {
        Some(raw) if !raw.is_empty() => {
            // A malformed id fails the turn internal (the uncaught
            // `ValidationError` on `states.filter(id=...)`).
            let Some(wanted) = parse_uuid_token(raw) else {
                return Err(ToolFailure::internal(invalid_uuid_message(raw)));
            };
            let target = states
                .iter()
                .find(|state| state.id == wanted)
                .cloned()
                .ok_or_else(invalid_state_error)?;
            if !is_ticking_state(Some(&target.group), Some(&target.name)) {
                return Err(non_ticking_state_error().into());
            }
            target
        }
        // `states.filter(group__in=list(PHASES.keys()))` + lowest
        // sequence (`runs.py:63-65`).
        _ => {
            let mut candidates: Vec<&super::tools_local_store::StateRow> = states
                .iter()
                .filter(|state| PHASES.iter().any(|phase| phase.group == state.group))
                .collect();
            candidates.sort_by(|a, b| a.sequence.total_cmp(&b.sequence));
            match candidates.first() {
                Some(target) => (*target).clone(),
                None => return Ok(no_delegation_state_result()),
            }
        }
    };
    if issue.state_id.is_some_and(|id| id == target.id) {
        return Ok(already_in_state_result(&target.name));
    }
    let from_view = issue
        .state_id
        .map(|id| pidash_services::orchestration::creation::StateView {
            id,
            name: issue.state_name.clone().unwrap_or_default(),
            group: issue.state_group.clone().unwrap_or_default(),
        });
    let outcome = store
        .drive_transition(
            &issue.id,
            from_view,
            pidash_services::orchestration::creation::StateView {
                id: target.id,
                name: target.name.clone(),
                group: target.group.clone(),
            },
            &actor,
            Some(actor),
            &env.now,
        )
        .await?;
    let run_id = outcome.created_run_id.map(|id| id.to_string());
    store
        .record_write(
            deps,
            &dispatch_summary(&issue.project_identifier, issue.sequence_id, &target.name),
            &[row_link(deps, &issue)],
            &env.now,
        )
        .await?;
    Ok(dispatch_success_result(
        run_id.as_deref(),
        &target.name,
        &outcome.reason,
    ))
}

/// Python truthiness over a JSON value (`github.py:153`): `None`,
/// `False`, `0`, `""`, `[]` and `{}` are falsy, everything else —
/// including exotic-but-truthy scalars — is truthy.
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(int) = number.as_u64() {
                int != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `get_pull_request_status` (`github.py:85-165`): the never-raises
/// PR/MR probe. Every failure mode answers `{"state": "unknown",
/// "reason": ...}`; only a decoded 200 shapes a verdict.
pub async fn execute_get_pull_request_status<S: ToolStore>(
    store: &S,
    deps: &mut AssistantDeps,
    env: &ToolEnv,
    args: &tool_args::PrStatusArgs,
) -> Result<Value, ToolFailure> {
    use pidash_services::assistant::tools_github::{
        check_pr_budget, parse_code_review_url, pick_sync_token, review_api_url, review_headers,
        status_reason, unknown_state,
    };
    // Per-run budget (`github.py:94-98`): exhausted answers without
    // consuming; otherwise the counter increments before parsing.
    let Some(next) = check_pr_budget(deps.budget.pr_lookups, env.pr_lookups_cap) else {
        return Ok(unknown_state("budget_exhausted"));
    };
    deps.budget.pr_lookups = next;
    let Some(parsed) = parse_code_review_url(&args.url, &env.gitlab_hosts) else {
        return Ok(unknown_state("unsupported_url"));
    };
    let Some(api_url) = review_api_url(&parsed) else {
        // Unreachable: the registry only yields github/gitlab providers.
        return Ok(unknown_state("unsupported_url"));
    };
    if pidash_services::assistant::ssrf::is_blocked(
        &api_url,
        env.block_private_urls,
        &*env.ssrf_resolver,
    ) {
        return Ok(unknown_state("blocked"));
    }
    // Token (`github._find_token :48-82`): the binding row wins
    // outright — even an empty/undecryptable token returns without
    // consulting the sync fallback (`github.py:63-65` sits inside
    // `if binding is not None: return ...`). Each stored token
    // decrypts (failures read back as `""`, which behaves as no
    // token — the ported `_maybe_decrypt` bug, preserved).
    let binding = store
        .github_binding_config(deps, &parsed.provider, &parsed.namespace, &parsed.repo_name)
        .await?;
    let mut token: Option<String> = None;
    if let Some(config) = binding {
        // `config.get("token")` (`github.py:63`).
        token = store.decrypt_token(config.get("token").and_then(Value::as_str));
    } else if parsed.provider == "github" {
        if let Some(credentials) = store
            .github_sync_credentials(deps, &parsed.namespace, &parsed.repo_name)
            .await?
        {
            // `creds.get("access_token") or creds.get("token")`
            // (`github.py:81`).
            token = store.decrypt_token(
                pick_sync_token(
                    credentials.get("access_token").and_then(Value::as_str),
                    credentials.get("token").and_then(Value::as_str),
                )
                .as_deref(),
            );
        }
    }
    let headers = review_headers(&parsed.provider, token.as_deref());
    match store.fetch_pr(&api_url, &headers).await {
        PrFetch::NetworkError => Ok(unknown_state("network_error")),
        PrFetch::Fetched { status, body } => {
            if let Some(reason) = status_reason(status) {
                return Ok(unknown_state(&reason));
            }
            let Some(data) = body else {
                // `resp.json()` raising (`github.py:147-149`).
                return Ok(unknown_state("bad_response"));
            };
            // `github.py:151-158`, with JSON truthiness for `merged` /
            // `merged_at` (the services `classify_review_state` takes
            // `bool`/`&str` and would collapse exotic-but-truthy JSON,
            // so the verdict stays inline).
            let provider_state = data.get("state").and_then(Value::as_str);
            let state = if data.get("merged").is_some_and(json_truthy)
                || data.get("merged_at").is_some_and(json_truthy)
                || provider_state == Some("merged")
            {
                "merged"
            } else if provider_state == Some("closed") {
                "closed"
            } else {
                "open"
            };
            // The success shape passes `title`/`merged_at` through raw
            // (`data.get(...)`, any JSON type — `github.py:159-165`).
            Ok(json!({
                "state": state,
                "title": data.get("title").cloned().unwrap_or(Value::Null),
                "merged_at": data.get("merged_at").cloned().unwrap_or(Value::Null),
                "url": parsed.url,
                "provider": parsed.provider,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools_local_store::*;
    use super::*;
    use chrono::DateTime;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn dt(day: u32, hour: u32) -> DateTime<chrono::Utc> {
        chrono::DateTime::from_naive_utc_and_offset(
            chrono::NaiveDate::from_ymd_opt(2026, 3, day)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap(),
            chrono::Utc,
        )
    }

    /// Deterministic fixture world: workspace `acme`, user `u1` (member),
    /// project `PROJ` (member + write), `OTHER` (non-member), `NOWRITE`
    /// (member, read-only), issues PROJ-1/2/3, NOWRITE-1, OTHER-1, four
    /// states, labels, members, comments, a run, and relation rows.
    /// One recorded `fetch_pr` call: the API URL plus the headers sent.
    type PrFetchCall = (String, Vec<(String, String)>);

    struct FixtureStore {
        user_id: Uuid,
        missing_user: bool,
        /// When set, identifier lookups fail `Db` (pins error
        /// propagation out of multi-target resolution).
        fail_identifier_lookups: bool,
        /// When set, the pre-lock `scoped_issue` read reports a stale
        /// state name/group (pins the locked-row `from_view`).
        stale_pre_lock_state: bool,
        projects: Vec<ProjectRow>,
        member_project_ids: Vec<Uuid>,
        write_project_ids: Vec<Uuid>,
        issues: Vec<IssueRow>,
        assignees: HashMap<Uuid, Vec<Uuid>>,
        issue_labels: HashMap<Uuid, Vec<Uuid>>,
        comments: HashMap<Uuid, Vec<IssueCommentRow>>,
        states: HashMap<Uuid, Vec<StateRow>>,
        labels: HashMap<Uuid, Vec<LabelRow>>,
        workspace_labels: Vec<LabelRow>,
        members: HashMap<Uuid, Vec<MemberRow>>,
        runs: HashMap<Uuid, Vec<AgentRunRow>>,
        pairs: Mutex<Vec<PairRow>>,
        insert_race_once: Mutex<bool>,
        activities: Mutex<Vec<RelationActivity>>,
        created_issues: Mutex<Vec<NewIssueInput>>,
        /// The last row `create_issue_row` returned (pins the
        /// post-drive re-read target).
        last_created: Mutex<Option<IssueRow>>,
        /// When set, `scoped_issue` answers the last created id with
        /// the created row moved to this state (pins the post-drive
        /// brief, PIDASHCONV-1444).
        post_drive_state: Option<(Uuid, String, String)>,
        updated_issues: Mutex<Vec<(Uuid, IssuePatch)>>,
        transitions: Mutex<Vec<Uuid>>,
        transition_outcome: TransitionOutcome,
        created_comments: Mutex<Vec<NewCommentInput>>,
        record_writes: Mutex<Vec<(String, Vec<Value>)>>,
        /// `from_state` views received by the fused update drive and
        /// `drive_transition`, in call order (pins the locked-row
        /// `from_view`).
        transition_from: Mutex<Vec<Option<(Uuid, String, String)>>>,
        /// `to_state` views received by `drive_create_transition`, in
        /// call order (pins the saved-row target view).
        transition_to: Mutex<Vec<(Uuid, String, String)>>,
        binding_configs: HashMap<(String, String, String), Value>,
        sync_creds: HashMap<(String, String), Value>,
        decrypt_map: HashMap<String, String>,
        pr_fetch: PrFetch,
        pr_fetch_calls: Mutex<Vec<PrFetchCall>>,
        next_seq: Mutex<i64>,
    }

    fn project_row(
        id: u128,
        identifier: &str,
        name: &str,
        workspace: Uuid,
        flag: bool,
    ) -> ProjectRow {
        ProjectRow {
            id: uid(id),
            identifier: identifier.to_owned(),
            name: name.to_owned(),
            workspace_id: workspace,
            guest_view_all_features: flag,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn issue_row(
        id: u128,
        project: &ProjectRow,
        sequence_id: i64,
        name: &str,
        state: Option<(&str, &str, Uuid)>,
        priority: &str,
        stripped: Option<&str>,
        created_by: Option<Uuid>,
        updated_day: u32,
    ) -> IssueRow {
        IssueRow {
            id: uid(id),
            project_id: project.id,
            project_identifier: project.identifier.clone(),
            workspace_id: project.workspace_id,
            sequence_id,
            name: name.to_owned(),
            parent_id: None,
            state_id: state.as_ref().map(|(_, _, id)| *id),
            state_name: state.as_ref().map(|(_, name, _)| (*name).to_owned()),
            state_group: state.as_ref().map(|(group, _, _)| (*group).to_owned()),
            priority: priority.to_owned(),
            description_stripped: stripped.map(str::to_owned),
            created_by_id: created_by,
            updated_at: dt(updated_day, 12),
        }
    }

    fn state_row(id: u128, name: &str, group: &str, default: bool, sequence: f64) -> StateRow {
        StateRow {
            id: uid(id),
            name: name.to_owned(),
            group: group.to_owned(),
            default,
            sequence,
        }
    }

    impl FixtureStore {
        fn standard() -> Self {
            let workspace = uid(900);
            let user = uid(1);
            let proj = project_row(101, "PROJ", "Project", workspace, false);
            let other = project_row(102, "OTHER", "Other", workspace, false);
            let nowrite = project_row(103, "NOWRITE", "Nowrite", workspace, false);
            let backlog = state_row(201, "Backlog", "backlog", true, 1.0);
            let progress = state_row(202, "In Progress", "started", false, 2.0);
            let review = state_row(203, "In Review", "review", false, 3.0);
            let done = state_row(204, "Done", "completed", false, 4.0);
            let mut states = HashMap::new();
            states.insert(
                proj.id,
                vec![
                    backlog.clone(),
                    progress.clone(),
                    review.clone(),
                    done.clone(),
                ],
            );
            states.insert(other.id, vec![backlog.clone()]);
            states.insert(nowrite.id, vec![backlog.clone()]);
            let issue1 = issue_row(
                301,
                &proj,
                1,
                "First issue",
                Some(("backlog", "Backlog", backlog.id)),
                "high",
                Some("first body"),
                Some(user),
                10,
            );
            let mut issue2 = issue_row(
                302,
                &proj,
                2,
                "Second <i>issue</i>",
                Some(("started", "In Progress", progress.id)),
                "none",
                Some("second body"),
                Some(uid(2)),
                12,
            );
            issue2.parent_id = Some(issue1.id);
            let issue3 = issue_row(
                303,
                &proj,
                3,
                "Third issue",
                Some(("unstarted", "Todo", uid(205))),
                "low",
                None,
                Some(user),
                11,
            );
            let other_issue = issue_row(
                304,
                &other,
                1,
                "Other issue",
                Some(("backlog", "Backlog", backlog.id)),
                "medium",
                None,
                Some(user),
                9,
            );
            // Read-visible (member project) but write-denied; created
            // by someone else and unassigned, so `list_my_issues`
            // skips it while the other reads include it.
            let nowrite_issue = issue_row(
                305,
                &nowrite,
                1,
                "Nowrite task",
                Some(("backlog", "Backlog", backlog.id)),
                "medium",
                None,
                Some(uid(2)),
                8,
            );
            let mut assignees = HashMap::new();
            assignees.insert(issue2.id, vec![user]);
            let mut issue_labels = HashMap::new();
            issue_labels.insert(issue1.id, vec![uid(401)]);
            let mut labels = HashMap::new();
            labels.insert(
                proj.id,
                vec![
                    LabelRow {
                        id: uid(401),
                        name: "bug".to_owned(),
                    },
                    LabelRow {
                        id: uid(402),
                        name: "frontend".to_owned(),
                    },
                ],
            );
            let mut members = HashMap::new();
            members.insert(
                proj.id,
                vec![
                    MemberRow {
                        member_id: user,
                        display_name: Some("Alice".to_owned()),
                        email: Some("alice@acme.test".to_owned()),
                        role: 20,
                    },
                    MemberRow {
                        member_id: uid(2),
                        display_name: None,
                        email: Some("bob@acme.test".to_owned()),
                        role: 15,
                    },
                ],
            );
            let mut comments = HashMap::new();
            comments.insert(
                issue1.id,
                vec![
                    IssueCommentRow {
                        comment_stripped: Some("newer comment".to_owned()),
                        created_at: dt(12, 8),
                        speaker_type: "agent".to_owned(),
                        actor_id: None,
                        actor_display_name: None,
                        actor_email: None,
                    },
                    IssueCommentRow {
                        comment_stripped: Some("older comment".to_owned()),
                        created_at: dt(11, 8),
                        speaker_type: "user".to_owned(),
                        actor_id: Some(uid(2)),
                        actor_display_name: None,
                        actor_email: Some("bob@acme.test".to_owned()),
                    },
                ],
            );
            let mut runs = HashMap::new();
            runs.insert(
                issue2.id,
                vec![AgentRunRow {
                    id: uid(501),
                    status: "queued".to_owned(),
                    created_at: Some(dt(12, 9)),
                }],
            );
            // PROJ-2 blocked_by PROJ-1 (forward-stored) plus a
            // reverse-stored PROJ-3-blocking-PROJ-1 twin (viewpoint
            // `blocked_by` from PROJ-1's side) plus a junk-typed row
            // the grouped read must drop.
            let pairs = vec![
                PairRow {
                    id: uid(601),
                    issue_id: issue2.id,
                    related_issue_id: issue1.id,
                    relation_type: "blocked_by".to_owned(),
                },
                PairRow {
                    id: uid(602),
                    issue_id: issue3.id,
                    related_issue_id: issue1.id,
                    relation_type: "blocking".to_owned(),
                },
                PairRow {
                    id: uid(603),
                    issue_id: issue1.id,
                    related_issue_id: issue2.id,
                    relation_type: "frobnicate".to_owned(),
                },
            ];
            Self {
                user_id: user,
                missing_user: false,
                fail_identifier_lookups: false,
                stale_pre_lock_state: false,
                projects: vec![proj.clone(), other, nowrite.clone()],
                member_project_ids: vec![proj.id, nowrite.id],
                write_project_ids: vec![proj.id],
                issues: vec![issue1, issue2, issue3, other_issue, nowrite_issue],
                assignees,
                issue_labels,
                comments,
                states,
                labels,
                workspace_labels: vec![LabelRow {
                    id: uid(403),
                    name: "all-hands".to_owned(),
                }],
                members,
                runs,
                pairs: Mutex::new(pairs),
                insert_race_once: Mutex::new(false),
                activities: Mutex::new(Vec::new()),
                created_issues: Mutex::new(Vec::new()),
                last_created: Mutex::new(None),
                post_drive_state: None,
                updated_issues: Mutex::new(Vec::new()),
                transitions: Mutex::new(Vec::new()),
                transition_outcome: TransitionOutcome {
                    created_run_id: Some(uid(502)),
                    reason: "entered the ticking bucket".to_owned(),
                },
                created_comments: Mutex::new(Vec::new()),
                record_writes: Mutex::new(Vec::new()),
                transition_from: Mutex::new(Vec::new()),
                transition_to: Mutex::new(Vec::new()),
                binding_configs: HashMap::new(),
                sync_creds: HashMap::new(),
                decrypt_map: HashMap::new(),
                pr_fetch: PrFetch::NetworkError,
                pr_fetch_calls: Mutex::new(Vec::new()),
                next_seq: Mutex::new(100),
            }
        }

        fn deps(&self) -> AssistantDeps {
            AssistantDeps {
                user_id: self.user_id,
                user_display: "u1".to_owned(),
                workspace_id: uid(900),
                workspace_slug: "acme".to_owned(),
                workspace_name: "Acme".to_owned(),
                workspace_role: 15,
                thread_id: uid(700),
                turn_id: uid(701),
                mode: "chat".to_owned(),
                budget: pidash_services::assistant::agent::RunBudget { pr_lookups: 0 },
            }
        }

        fn env(&self) -> ToolEnv {
            ToolEnv {
                now: dt(13, 12),
                ..ToolEnv::default()
            }
        }

        fn scoped(&self) -> Vec<IssueRow> {
            self.issues
                .iter()
                .filter(|issue| self.member_project_ids.contains(&issue.project_id))
                .cloned()
                .collect()
        }

        fn page(rows: Vec<IssueRow>, limit: i64, offset: i64) -> Vec<IssueRow> {
            let mut rows = rows;
            rows.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
            let start = offset.max(0) as usize;
            let end = start.saturating_add(limit.max(0) as usize + 1);
            rows.into_iter().skip(start).take(end - start).collect()
        }
    }

    impl ToolStore for FixtureStore {
        async fn require_user(&self, deps: &AssistantDeps) -> Result<Uuid, StoreError> {
            if self.missing_user {
                Err(StoreError::Missing { model: "User" })
            } else {
                Ok(deps.user_id)
            }
        }

        async fn member_projects(
            &self,
            _deps: &AssistantDeps,
        ) -> Result<Vec<ProjectRow>, StoreError> {
            let mut projects: Vec<ProjectRow> = self
                .projects
                .iter()
                .filter(|project| self.member_project_ids.contains(&project.id))
                .cloned()
                .collect();
            projects.sort_by(|a, b| a.name.cmp(&b.name));
            projects.truncate(50);
            Ok(projects)
        }

        async fn member_project(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
        ) -> Result<Option<ProjectRow>, StoreError> {
            Ok(self
                .projects
                .iter()
                .find(|project| {
                    project.id == *project_id && self.member_project_ids.contains(&project.id)
                })
                .cloned())
        }

        async fn scoped_issue(
            &self,
            _deps: &AssistantDeps,
            issue_id: &Uuid,
        ) -> Result<Option<IssueRow>, StoreError> {
            // The post-drive re-read: when armed, answer the created
            // id with the created row moved to the drive's target
            // state (PIDASHCONV-1444).
            if let (Some((id, name, group)), Some(created)) = (
                self.post_drive_state.clone(),
                self.last_created.lock().unwrap().clone(),
            ) {
                if created.id == *issue_id {
                    let mut row = created;
                    row.state_id = Some(id);
                    row.state_name = Some(name);
                    row.state_group = Some(group);
                    return Ok(Some(row));
                }
            }
            let mut found = self
                .scoped()
                .into_iter()
                .find(|issue| issue.id == *issue_id);
            // Model a concurrent state move between the pre-lock read
            // and the lock: the name/group here go stale while the
            // locked read in `update_issue_row` stays current.
            if self.stale_pre_lock_state {
                if let Some(row) = found.as_mut() {
                    row.state_name = Some("Stale".to_owned());
                    row.state_group = Some("stale".to_owned());
                }
            }
            Ok(found)
        }

        async fn search_issues_page(
            &self,
            _deps: &AssistantDeps,
            project_id: Option<Uuid>,
            query: &str,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<IssueRow>, StoreError> {
            // Substring match over the searchable text stands in for the
            // FTS SQL; test queries are unambiguous single tokens.
            let needle = query.to_lowercase();
            let rows: Vec<IssueRow> = self
                .scoped()
                .into_iter()
                .filter(|issue| project_id.is_none_or(|id| issue.project_id == id))
                .filter(|issue| {
                    query.trim().is_empty()
                        || issue.name.to_lowercase().contains(&needle)
                        || issue
                            .description_stripped
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&needle)
                        || format!("{}-{}", issue.project_identifier, issue.sequence_id)
                            .to_lowercase()
                            .contains(&needle)
                })
                .collect();
            Ok(Self::page(rows, limit, offset))
        }

        async fn list_issues_page(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
            filters: &pidash_services::assistant::tools_issues::ListFilterPredicates,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<IssueRow>, StoreError> {
            let rows: Vec<IssueRow> = self
                .scoped()
                .into_iter()
                .filter(|issue| issue.project_id == *project_id)
                .filter(|issue| {
                    filters.state_in.is_empty()
                        || issue
                            .state_id
                            .is_some_and(|id| filters.state_in.contains(&id.to_string()))
                })
                .filter(|issue| {
                    filters.state_group_in.is_empty()
                        || issue
                            .state_group
                            .as_deref()
                            .is_some_and(|group| filters.state_group_in.contains(&group.to_owned()))
                })
                .filter(|issue| {
                    filters.priority_in.is_empty() || filters.priority_in.contains(&issue.priority)
                })
                .filter(|issue| {
                    if filters.parent_is_null {
                        issue.parent_id.is_none()
                    } else if filters.parent_in.is_empty() {
                        true
                    } else {
                        issue
                            .parent_id
                            .is_some_and(|id| filters.parent_in.contains(&id.to_string()))
                    }
                })
                .filter(|issue| {
                    filters.label_ids_in.is_empty()
                        || self.issue_labels.get(&issue.id).is_some_and(|labels| {
                            labels
                                .iter()
                                .any(|id| filters.label_ids_in.contains(&id.to_string()))
                        })
                })
                .collect();
            Ok(Self::page(rows, limit, offset))
        }

        async fn my_issues_page(
            &self,
            deps: &AssistantDeps,
            scope: &str,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<IssueRow>, StoreError> {
            let rows: Vec<IssueRow> = self
                .scoped()
                .into_iter()
                .filter(|issue| match scope {
                    "assigned" => self
                        .assignees
                        .get(&issue.id)
                        .is_some_and(|assignees| assignees.contains(&deps.user_id)),
                    "created" => issue.created_by_id == Some(deps.user_id),
                    _ => {
                        issue.created_by_id == Some(deps.user_id)
                            || self
                                .assignees
                                .get(&issue.id)
                                .is_some_and(|assignees| assignees.contains(&deps.user_id))
                    }
                })
                .collect();
            Ok(Self::page(rows, limit, offset))
        }

        async fn issue_comments(
            &self,
            issue_id: &Uuid,
        ) -> Result<Vec<IssueCommentRow>, StoreError> {
            Ok(self.comments.get(issue_id).cloned().unwrap_or_default())
        }

        async fn project_states(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
        ) -> Result<Vec<StateRow>, StoreError> {
            Ok(self.states.get(project_id).cloned().unwrap_or_default())
        }

        async fn project_labels(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
        ) -> Result<Vec<LabelRow>, StoreError> {
            let mut labels = self.labels.get(project_id).cloned().unwrap_or_default();
            labels.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(labels)
        }

        async fn workspace_labels(
            &self,
            _deps: &AssistantDeps,
        ) -> Result<Vec<LabelRow>, StoreError> {
            Ok(self.workspace_labels.clone())
        }

        async fn workspace_issue_by_identifier(
            &self,
            _deps: &AssistantDeps,
            project_ident: &str,
            sequence_id: i64,
        ) -> Result<Option<IssueRow>, StoreError> {
            if self.fail_identifier_lookups {
                return Err(StoreError::Db("injected identifier failure".to_owned()));
            }
            Ok(self
                .issues
                .iter()
                .find(|issue| {
                    issue.project_identifier.eq_ignore_ascii_case(project_ident)
                        && issue.sequence_id == sequence_id
                })
                .cloned())
        }

        async fn project_members(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
        ) -> Result<Vec<MemberRow>, StoreError> {
            Ok(self.members.get(project_id).cloned().unwrap_or_default())
        }

        async fn check_project_write(
            &self,
            _deps: &AssistantDeps,
            project_id: &Uuid,
        ) -> Result<bool, StoreError> {
            Ok(self.write_project_ids.contains(project_id))
        }

        async fn agent_runs(&self, issue_id: &Uuid) -> Result<Vec<AgentRunRow>, StoreError> {
            Ok(self.runs.get(issue_id).cloned().unwrap_or_default())
        }

        async fn scoped_issue_by_identifier(
            &self,
            _deps: &AssistantDeps,
            project_ident: &str,
            sequence_id: i64,
        ) -> Result<Option<IssueRow>, StoreError> {
            if self.fail_identifier_lookups {
                return Err(StoreError::Db("injected identifier failure".to_owned()));
            }
            Ok(self.scoped().into_iter().find(|issue| {
                issue.project_identifier.eq_ignore_ascii_case(project_ident)
                    && issue.sequence_id == sequence_id
            }))
        }

        async fn pair_rows(&self, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, StoreError> {
            Ok(self
                .pairs
                .lock()
                .unwrap()
                .iter()
                .filter(|pair| {
                    (pair.issue_id == *a_id && pair.related_issue_id == *b_id)
                        || (pair.issue_id == *b_id && pair.related_issue_id == *a_id)
                })
                .cloned()
                .collect())
        }

        async fn insert_relation(
            &self,
            issue_id: &Uuid,
            related_id: &Uuid,
            stored_type: &str,
            _project_id: &Uuid,
            _workspace_id: &Uuid,
            _actor_id: &Uuid,
            _now: &DateTime<chrono::Utc>,
        ) -> Result<bool, StoreError> {
            // A set race flag mimics the `IntegrityError` path: the row
            // lands (written by the "concurrent" writer) but the insert
            // reports failure, forcing the re-read below.
            let raced = std::mem::replace(&mut *self.insert_race_once.lock().unwrap(), false);
            self.pairs.lock().unwrap().push(PairRow {
                id: Uuid::new_v4(),
                issue_id: *issue_id,
                related_issue_id: *related_id,
                relation_type: stored_type.to_owned(),
            });
            Ok(!raced)
        }

        async fn delete_relation(
            &self,
            row_id: &Uuid,
            _actor_id: &Uuid,
            _now: &DateTime<chrono::Utc>,
        ) -> Result<(), StoreError> {
            self.pairs.lock().unwrap().retain(|pair| pair.id != *row_id);
            Ok(())
        }

        async fn enqueue_relation_activity(
            &self,
            activity: &RelationActivity,
        ) -> Result<(), StoreError> {
            self.activities.lock().unwrap().push(activity.clone());
            Ok(())
        }

        async fn grouped_relation_rows(
            &self,
            issue_id: &Uuid,
            _workspace_id: &Uuid,
        ) -> Result<Vec<PairRow>, StoreError> {
            Ok(self
                .pairs
                .lock()
                .unwrap()
                .iter()
                .filter(|pair| {
                    (pair.issue_id == *issue_id || pair.related_issue_id == *issue_id)
                        && pair.issue_id != pair.related_issue_id
                })
                .cloned()
                .collect())
        }

        async fn visible_grouped_issues(
            &self,
            _deps: &AssistantDeps,
            ids: &[Uuid],
            _workspace_id: &Uuid,
        ) -> Result<Vec<GroupedIssueRow>, StoreError> {
            Ok(self
                .scoped()
                .into_iter()
                .filter(|issue| ids.contains(&issue.id))
                .map(|issue| GroupedIssueRow {
                    id: issue.id,
                    project_identifier: issue.project_identifier,
                    sequence_id: issue.sequence_id,
                    name: issue.name,
                    state_name: issue.state_name,
                    state_group: issue.state_group,
                })
                .collect())
        }

        async fn issue_parent_id(&self, issue_id: &Uuid) -> Result<Option<Uuid>, StoreError> {
            Ok(self
                .issues
                .iter()
                .find(|issue| issue.id == *issue_id)
                .and_then(|issue| issue.parent_id))
        }

        async fn create_issue_row(
            &self,
            input: &NewIssueInput,
            now: &DateTime<chrono::Utc>,
        ) -> Result<IssueRow, StoreError> {
            self.created_issues.lock().unwrap().push(input.clone());
            let mut seq = self.next_seq.lock().unwrap();
            *seq += 1;
            let sequence_id = *seq;
            // Mirror the `Issue.save` None-state arm (default, else
            // lowest sequence, else None).
            let state_id = match input.state_id {
                Some(id) => Some(id),
                None => {
                    let states = self
                        .states
                        .get(&input.project_id)
                        .cloned()
                        .unwrap_or_default();
                    states
                        .iter()
                        .find(|state| state.default)
                        .or_else(|| {
                            states
                                .iter()
                                .min_by(|a, b| a.sequence.total_cmp(&b.sequence))
                        })
                        .map(|state| state.id)
                }
            };
            let (state_name, state_group) = state_id
                .and_then(|id| {
                    self.states.get(&input.project_id).and_then(|states| {
                        states
                            .iter()
                            .find(|state| state.id == id)
                            .map(|state| (Some(state.name.clone()), Some(state.group.clone())))
                    })
                })
                .unwrap_or((None, None));
            let row = IssueRow {
                id: Uuid::new_v4(),
                project_id: input.project_id,
                project_identifier: input.project_identifier.clone(),
                workspace_id: input.workspace_id,
                sequence_id,
                name: input.name.clone(),
                parent_id: input.parent_id,
                state_id,
                state_name,
                state_group,
                priority: input.priority.clone(),
                description_stripped: if input.description_html.is_empty() {
                    None
                } else {
                    Some(input.description_html.clone())
                },
                created_by_id: Some(input.created_by),
                updated_at: *now,
            };
            *self.last_created.lock().unwrap() = Some(row.clone());
            Ok(row)
        }

        async fn update_issue_row(
            &self,
            patch: &IssuePatch,
            issue_id: &Uuid,
            to_state: Option<pidash_services::orchestration::creation::StateView>,
            _actor_id: &Uuid,
            now: &DateTime<chrono::Utc>,
        ) -> Result<(IssueRow, Option<TransitionOutcome>), StoreError> {
            self.updated_issues
                .lock()
                .unwrap()
                .push((*issue_id, patch.clone()));
            let Some(issue) = self
                .issues
                .iter()
                .find(|issue| issue.id == *issue_id)
                .cloned()
            else {
                // A lost race (`DoesNotExist` → internal), never a
                // panic.
                return Err(StoreError::Missing { model: "Issue" });
            };
            // The "locked" read: the map row, not the pre-lock read
            // (which `stale_pre_lock_state` may have staled).
            let from_id = issue.state_id;
            let from_view = issue.state_id.map(|id| {
                (
                    id,
                    issue.state_name.clone().unwrap_or_default(),
                    issue.state_group.clone().unwrap_or_default(),
                )
            });
            let mut row = issue;
            if let Some(name) = &patch.name {
                row.name = name.clone();
            }
            if patch.description_html.is_some() {
                // The stale-`description_stripped` quirk: not rewritten.
            }
            if let Some(priority) = &patch.priority {
                row.priority = priority.clone();
            }
            if patch.state_given {
                row.state_id = patch.state_id;
                let found = patch.state_id.and_then(|id| {
                    self.states.get(&row.project_id).and_then(|states| {
                        states
                            .iter()
                            .find(|state| state.id == id)
                            .map(|state| (state.name.clone(), state.group.clone()))
                    })
                });
                row.state_name = found.as_ref().map(|(name, _)| name.clone());
                row.state_group = found.as_ref().map(|(_, group)| group.clone());
            }
            if patch.parent_touched {
                row.parent_id = patch.parent_id;
            }
            row.updated_at = *now;
            // The fused drive (`to_state is not None and from_state !=
            // to_state`, `issues.py:373`): recorded exactly as the live
            // store drives, so the transition assertions keep pinning
            // the executor's target view.
            let changed = to_state.as_ref().is_some_and(|to| from_id != Some(to.id));
            if changed {
                self.transitions.lock().unwrap().push(*issue_id);
                self.transition_from.lock().unwrap().push(from_view);
                Ok((row, Some(self.transition_outcome.clone())))
            } else {
                Ok((row, None))
            }
        }

        async fn drive_transition(
            &self,
            issue_id: &Uuid,
            from_state: Option<pidash_services::orchestration::creation::StateView>,
            _to_state: pidash_services::orchestration::creation::StateView,
            _actor_id: &Uuid,
            _created_by: Option<Uuid>,
            _now: &DateTime<chrono::Utc>,
        ) -> Result<TransitionOutcome, StoreError> {
            self.transitions.lock().unwrap().push(*issue_id);
            self.transition_from
                .lock()
                .unwrap()
                .push(from_state.map(|state| (state.id, state.name, state.group)));
            Ok(self.transition_outcome.clone())
        }

        async fn drive_create_transition(
            &self,
            issue_id: &Uuid,
            to_state: pidash_services::orchestration::creation::StateView,
            _actor_id: &Uuid,
            _now: &DateTime<chrono::Utc>,
        ) -> Result<Option<TransitionOutcome>, StoreError> {
            self.transitions.lock().unwrap().push(*issue_id);
            self.transition_from.lock().unwrap().push(None);
            self.transition_to
                .lock()
                .unwrap()
                .push((to_state.id, to_state.name, to_state.group));
            Ok(Some(self.transition_outcome.clone()))
        }

        async fn create_comment_row(
            &self,
            input: &NewCommentInput,
            _now: &DateTime<chrono::Utc>,
        ) -> Result<Uuid, StoreError> {
            self.created_comments.lock().unwrap().push(input.clone());
            Ok(Uuid::new_v4())
        }

        async fn record_write(
            &self,
            _deps: &AssistantDeps,
            summary: &str,
            links: &[Value],
            _now: &DateTime<chrono::Utc>,
        ) -> Result<(), StoreError> {
            self.record_writes
                .lock()
                .unwrap()
                .push((summary.to_owned(), links.to_vec()));
            Ok(())
        }

        async fn github_binding_config(
            &self,
            _deps: &AssistantDeps,
            provider: &str,
            namespace: &str,
            repo: &str,
        ) -> Result<Option<Value>, StoreError> {
            Ok(self
                .binding_configs
                .get(&(provider.to_owned(), namespace.to_owned(), repo.to_owned()))
                .cloned())
        }

        async fn github_sync_credentials(
            &self,
            _deps: &AssistantDeps,
            namespace: &str,
            repo: &str,
        ) -> Result<Option<Value>, StoreError> {
            Ok(self
                .sync_creds
                .get(&(namespace.to_owned(), repo.to_owned()))
                .cloned())
        }

        fn decrypt_token(&self, stored: Option<&str>) -> Option<String> {
            // `_maybe_decrypt` over a `decrypt_data` that fails closed:
            // falsy in → `None`; mapped → the plaintext; unmapped →
            // `Some("")` (decrypt failure, behaves as no token).
            stored
                .filter(|token| !token.is_empty())
                .map(|token| self.decrypt_map.get(token).cloned().unwrap_or_default())
        }

        async fn fetch_pr(&self, api_url: &str, headers: &[(String, String)]) -> PrFetch {
            self.pr_fetch_calls
                .lock()
                .unwrap()
                .push((api_url.to_owned(), headers.to_vec()));
            self.pr_fetch.clone()
        }
    }

    // ------------------------------------------------------------------
    // Harness
    // ------------------------------------------------------------------

    async fn call(store: &FixtureStore, name: &str, raw: Value) -> Result<Value, ToolFailure> {
        let mut deps = store.deps();
        execute_local_tool(store, &mut deps, &store.env(), name, &raw).await
    }

    fn ok_value(result: Result<Value, ToolFailure>) -> Value {
        result.unwrap_or_else(|failure| panic!("expected ok, got {failure:?}"))
    }

    fn retry_message(result: &Result<Value, ToolFailure>) -> &str {
        match result {
            Err(ToolFailure::Retry { message }) => message,
            other => panic!("expected retry, got {other:?}"),
        }
    }

    fn internal_message(result: &Result<Value, ToolFailure>) -> &str {
        match result {
            Err(ToolFailure::Internal { message }) => message,
            other => panic!("expected internal, got {other:?}"),
        }
    }

    /// `_brief` for an unwrapped short name (no truncation in the fixture).
    fn brief(
        id: u128,
        ident: &str,
        project: u128,
        name: &str,
        parent: Option<u128>,
        state: Option<(&str, &str)>,
        priority: &str,
    ) -> Value {
        json!({
            "id": uid(id).to_string(),
            "identifier": ident,
            "project_id": uid(project).to_string(),
            "name": format!("<untrusted>{name}</untrusted>"),
            "name_truncated": false,
            "parent_id": parent.map(|id| uid(id).to_string()),
            "state": state.map(|(name, _)| name),
            "state_group": state.map(|(_, group)| group),
            "priority": priority,
        })
    }

    fn proj1() -> Value {
        brief(
            301,
            "PROJ-1",
            101,
            "First issue",
            None,
            Some(("Backlog", "backlog")),
            "high",
        )
    }

    fn proj2() -> Value {
        brief(
            302,
            "PROJ-2",
            101,
            "Second <i>issue</i>",
            Some(301),
            Some(("In Progress", "started")),
            "none",
        )
    }

    fn proj3() -> Value {
        brief(
            303,
            "PROJ-3",
            101,
            "Third issue",
            None,
            Some(("Todo", "unstarted")),
            "low",
        )
    }

    fn nowrite1() -> Value {
        brief(
            305,
            "NOWRITE-1",
            103,
            "Nowrite task",
            None,
            Some(("Backlog", "backlog")),
            "medium",
        )
    }

    // ------------------------------------------------------------------
    // Read tools
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn search_issues_shapes_briefs() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "search_issues",
                json!({"query": "second", "project_id": uid(101).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj2()], "has_more": false, "next_offset": Value::Null})
        );
    }

    #[tokio::test]
    async fn search_issues_denied_project_retries() {
        let store = FixtureStore::standard();
        // The message echoes the raw id string (`_scoping.py:55`).
        let raw = uid(102).to_string();
        let result = call(
            &store,
            "search_issues",
            json!({"query": "x", "project_id": raw.clone()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            format!("Project {raw} not found or not accessible.")
        );
    }

    #[tokio::test]
    async fn search_issues_blank_query_paginates() {
        let store = FixtureStore::standard();
        // Blank query skips the FTS arm: every scoped issue, newest first.
        let out = ok_value(call(&store, "search_issues", json!({"query": "  ", "limit": 2})).await);
        assert_eq!(
            out,
            json!({"results": [proj2(), proj3()], "has_more": true, "next_offset": 2})
        );
        let out = ok_value(
            call(
                &store,
                "search_issues",
                json!({"query": "  ", "limit": 2, "offset": 2}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj1(), nowrite1()], "has_more": false, "next_offset": Value::Null})
        );
    }

    #[tokio::test]
    async fn search_issues_clamps_limit_and_offset() {
        let store = FixtureStore::standard();
        // `max(1, min(limit, 20))` / `max(0, offset)`.
        let out = ok_value(
            call(
                &store,
                "search_issues",
                json!({"query": "", "limit": 99, "offset": -5}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "results": [proj2(), proj3(), proj1(), nowrite1()],
                "has_more": false,
                "next_offset": Value::Null,
            })
        );
    }

    #[tokio::test]
    async fn search_issues_sees_read_only_member_project() {
        let store = FixtureStore::standard();
        // The write gate does not narrow reads: NOWRITE is searchable.
        let out = ok_value(call(&store, "search_issues", json!({"query": "task"})).await);
        assert_eq!(
            out,
            json!({"results": [nowrite1()], "has_more": false, "next_offset": Value::Null})
        );
    }

    #[tokio::test]
    async fn list_issues_state_and_group_filters() {
        let store = FixtureStore::standard();
        let project = uid(101).to_string();
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": project, "state": "In Progress"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj2()], "has_more": false, "next_offset": Value::Null})
        );
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": uid(101).to_string(), "state_group": "backlog,unstarted"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj3(), proj1()], "has_more": false, "next_offset": Value::Null})
        );
    }

    #[tokio::test]
    async fn list_issues_parent_label_priority_filters() {
        let store = FixtureStore::standard();
        let project = uid(101).to_string();
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": project, "parent_issue_id": "null"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj3(), proj1()], "has_more": false, "next_offset": Value::Null})
        );
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": uid(101).to_string(), "parent_issue_id": "PROJ-1"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj2()], "has_more": false, "next_offset": Value::Null})
        );
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": uid(101).to_string(), "labels": "bug"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj1()], "has_more": false, "next_offset": Value::Null})
        );
        // Workspace-level labels resolve (here: matching nothing).
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": uid(101).to_string(), "labels": "all-hands"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [], "has_more": false, "next_offset": Value::Null})
        );
        // Priorities lowercase before matching (`issue_filters.py:606`).
        let out = ok_value(
            call(
                &store,
                "list_issues",
                json!({"project_id": uid(101).to_string(), "priority": "HIGH, low"}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({"results": [proj3(), proj1()], "has_more": false, "next_offset": Value::Null})
        );
    }

    #[tokio::test]
    async fn list_issues_unknown_filter_values_retry() {
        let store = FixtureStore::standard();
        let project = uid(101).to_string();
        for (params, message) in [
            (
                json!({"project_id": project.clone(), "state": "Nope"}),
                "Unknown state name(s): Nope. Valid states: Backlog, Done, In Progress, In Review.",
            ),
            (
                json!({"project_id": project.clone(), "state_group": "bogus"}),
                "Unknown state group(s): bogus. Valid groups: backlog, unstarted, started, review, test, completed, cancelled.",
            ),
            (
                json!({"project_id": project.clone(), "priority": "bogus"}),
                "Unknown priority value(s): bogus. Valid priorities: urgent, high, medium, low, none.",
            ),
            (
                json!({"project_id": project.clone(), "labels": "nope"}),
                "Unknown label name(s): nope. Valid labels: all-hands, bug, frontend.",
            ),
            (
                json!({"project_id": project.clone(), "parent_issue_id": "bogus"}),
                "Invalid parent 'bogus': expected an issue UUID, an identifier like PROJ-123, or null.",
            ),
            (
                json!({"project_id": project.clone(), "parent_issue_id": "PROJ-999"}),
                "Unknown parent issue 'PROJ-999'.",
            ),
            (
                json!({"project_id": project.clone(), "parent_issue_id": "null,PROJ-1"}),
                "parent=null cannot be combined with specific parent issues.",
            ),
        ] {
            let result = call(&store, "list_issues", params).await;
            assert_eq!(retry_message(&result), message);
        }
    }

    #[tokio::test]
    async fn list_my_issues_scopes() {
        let store = FixtureStore::standard();
        // OTHER-1 is created by the user but outside the member scope:
        // never listed, in any scope.
        let out = ok_value(call(&store, "list_my_issues", json!({})).await);
        assert_eq!(
            out,
            json!({
                "results": [proj2(), proj3(), proj1()],
                "has_more": false,
                "next_offset": Value::Null,
            })
        );
        let out = ok_value(call(&store, "list_my_issues", json!({"scope": "assigned"})).await);
        assert_eq!(
            out,
            json!({"results": [proj2()], "has_more": false, "next_offset": Value::Null})
        );
        let out = ok_value(call(&store, "list_my_issues", json!({"scope": "created"})).await);
        assert_eq!(
            out,
            json!({"results": [proj3(), proj1()], "has_more": false, "next_offset": Value::Null})
        );
        // Unknown scopes fall back to "all" (`issues.py:141-142`).
        let out = ok_value(call(&store, "list_my_issues", json!({"scope": "bogus"})).await);
        assert_eq!(out["results"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn get_issue_detail_shapes_comments() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "get_issue",
                json!({"issue_id": uid(301).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "id": uid(301).to_string(),
                "identifier": "PROJ-1",
                "project_id": uid(101).to_string(),
                "name": "<untrusted>First issue</untrusted>",
                "name_truncated": false,
                "parent_id": Value::Null,
                "state": "Backlog",
                "state_group": "backlog",
                "priority": "high",
                "description": "<untrusted>first body</untrusted>",
                "description_truncated": false,
                // Newest-first read, reversed to chronological
                // (`issues.py:161-179`); agent speakers render as
                // "Pi Dash AI", missing display names fall to email.
                "recent_comments": [
                    {
                        "author": "bob@acme.test",
                        "body": "<untrusted>older comment</untrusted>",
                        "body_truncated": false,
                        "created_at": "2026-03-11T08:00:00+00:00",
                    },
                    {
                        "author": "Pi Dash AI",
                        "body": "<untrusted>newer comment</untrusted>",
                        "body_truncated": false,
                        "created_at": "2026-03-12T08:00:00+00:00",
                    },
                ],
            })
        );
    }

    #[tokio::test]
    async fn get_issue_not_found_covers_malformed_and_out_of_scope() {
        let store = FixtureStore::standard();
        // A human-readable identifier (malformed UUID) retries rather
        // than failing the turn (`_scoping.py:67-84`).
        let result = call(&store, "get_issue", json!({"issue_id": "PROJ-1"})).await;
        assert_eq!(
            retry_message(&result),
            "Issue PROJ-1 not found or not accessible."
        );
        // A valid UUID outside the member scope is indistinguishable.
        let raw = uid(304).to_string();
        let result = call(&store, "get_issue", json!({"issue_id": raw.clone()})).await;
        assert_eq!(
            retry_message(&result),
            format!("Issue {raw} not found or not accessible.")
        );
    }

    #[tokio::test]
    async fn list_projects_lists_member_projects_by_name() {
        let store = FixtureStore::standard();
        let out = ok_value(call(&store, "list_projects", json!({})).await);
        assert_eq!(
            out,
            json!([
                {"id": uid(103).to_string(), "identifier": "NOWRITE", "name": "Nowrite"},
                {"id": uid(101).to_string(), "identifier": "PROJ", "name": "Project"},
            ])
        );
    }

    #[tokio::test]
    async fn list_states_lists_project_states_in_sequence() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "list_states",
                json!({"project_id": uid(101).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!([
                {"id": uid(201).to_string(), "name": "Backlog", "group": "backlog", "default": true},
                {"id": uid(202).to_string(), "name": "In Progress", "group": "started", "default": false},
                {"id": uid(203).to_string(), "name": "In Review", "group": "review", "default": false},
                {"id": uid(204).to_string(), "name": "Done", "group": "completed", "default": false},
            ])
        );
    }

    #[tokio::test]
    async fn list_labels_lists_project_labels_by_name() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "list_labels",
                json!({"project_id": uid(101).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!([
                {"id": uid(401).to_string(), "name": "bug"},
                {"id": uid(402).to_string(), "name": "frontend"},
            ])
        );
    }

    #[tokio::test]
    async fn list_project_members_falls_back_to_email() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "list_project_members",
                json!({"project_id": uid(101).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!([
                {"user_id": uid(1).to_string(), "display_name": "Alice", "role": 20},
                {"user_id": uid(2).to_string(), "display_name": "bob@acme.test", "role": 15},
            ])
        );
    }

    #[tokio::test]
    async fn get_run_status_lists_runs_newest_first() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "get_run_status",
                json!({"issue_id": uid(302).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "issue_id": uid(302).to_string(),
                "runs": [
                    {
                        "id": uid(501).to_string(),
                        "status": "queued",
                        "created_at": "2026-03-12T09:00:00+00:00",
                    },
                ],
            })
        );
        let out = ok_value(
            call(
                &store,
                "get_run_status",
                json!({"issue_id": uid(301).to_string()}),
            )
            .await,
        );
        assert_eq!(out, json!({"issue_id": uid(301).to_string(), "runs": []}));
    }

    #[tokio::test]
    async fn list_issue_relations_groups_from_viewpoint() {
        let store = FixtureStore::standard();
        let out = ok_value(call(&store, "list_issue_relations", json!({"issue": "PROJ-1"})).await);
        // Row 601 (PROJ-2 blocked_by PROJ-1) reads as `blocking` from
        // PROJ-1's side; the legacy reverse-stored row 602 reads as
        // `blocked_by`; the junk-typed row 603 is dropped. Every type
        // key is always present (`relations.py:291-315`).
        assert_eq!(
            out,
            json!({
                "issue": "PROJ-1",
                "relations": {
                    "blocked_by": [
                        {
                            "id": uid(303).to_string(),
                            "identifier": "PROJ-3",
                            "name": "<untrusted>Third issue</untrusted>",
                            "state": "Todo",
                            "state_group": "unstarted",
                        },
                    ],
                    "blocking": [
                        {
                            "id": uid(302).to_string(),
                            "identifier": "PROJ-2",
                            "name": "<untrusted>Second <i>issue</i></untrusted>",
                            "state": "In Progress",
                            "state_group": "started",
                        },
                    ],
                    "relates_to": [],
                    "duplicate": [],
                    "start_before": [],
                    "start_after": [],
                    "finish_before": [],
                    "finish_after": [],
                    "implemented_by": [],
                    "implements": [],
                },
            })
        );
        let result = call(&store, "list_issue_relations", json!({"issue": "NOPE-9"})).await;
        assert_eq!(
            retry_message(&result),
            "Issues not found or not accessible: NOPE-9"
        );
    }

    // ------------------------------------------------------------------
    // Write tools — issues
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_issue_happy_path() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "create_issue",
                json!({
                    "project_id": uid(101).to_string(),
                    "name": "  New issue  ",
                    "description_md": "New body",
                    "priority": "HIGH",
                }),
            )
            .await,
        );
        // The name strips, the priority lowercases, the state defaults.
        assert_eq!(out["created"], json!(true));
        assert_eq!(out["identifier"], json!("PROJ-101"));
        assert_eq!(out["name"], json!("<untrusted>New issue</untrusted>"));
        assert_eq!(out["state"], json!("Backlog"));
        assert_eq!(out["state_group"], json!("backlog"));
        assert_eq!(out["priority"], json!("high"));
        assert_eq!(out["parent_id"], Value::Null);
        assert_eq!(out["project_id"], json!(uid(101).to_string()));
        let id = out["id"].as_str().expect("uuid id");
        assert_eq!(
            out["url_path"],
            json!(format!("/acme/projects/{}/issues/{id}", uid(101)))
        );
        let created = store.created_issues.lock().unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].name, "New issue");
        assert_eq!(created[0].description_html, "<p>New body</p>");
        assert_eq!(created[0].priority, "high");
        assert_eq!(created[0].state_id, Some(uid(201)));
        assert_eq!(created[0].parent_id, None);
        assert_eq!(created[0].created_by, uid(1));
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "Created issue PROJ-101 \u{2014} New issue");
        assert_eq!(
            writes[0].1,
            vec![json!({
                "type": "issue",
                "workspace_slug": "acme",
                "project_id": uid(101).to_string(),
                "issue_id": id,
                "url_path": format!("/acme/projects/{}/issues/{id}", uid(101)),
            })]
        );
    }

    #[tokio::test]
    async fn create_issue_fires_post_save_transition() {
        // `signals.py:74-108`: prior state None, the saved state, the
        // receiver defaults. The outcome is dropped — the tool result
        // carries no run id (`issues.py:284`).
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "create_issue",
                json!({
                    "project_id": uid(101).to_string(),
                    "name": "Triage me",
                    "state_id": uid(202).to_string(),
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!(true));
        assert_eq!(out["state"], json!("In Progress"));
        assert!(out.get("dispatched_run_id").is_none());
        let id: Uuid = out["id"]
            .as_str()
            .expect("uuid id")
            .parse()
            .expect("uuid parses");
        assert_eq!(*store.transitions.lock().unwrap(), vec![id]);
        assert_eq!(*store.transition_from.lock().unwrap(), vec![None]);
        assert_eq!(
            *store.transition_to.lock().unwrap(),
            vec![(uid(202), "In Progress".to_owned(), "started".to_owned())]
        );
    }

    #[tokio::test]
    async fn create_issue_brief_reports_post_drive_row() {
        // PIDASHCONV-1444: the drive bounced the issue to Backlog, so
        // the re-read answers the Backlog row and the brief reports it
        // — while the drive itself saw the saved ticking target. The
        // sibling tests above (no `post_drive_state`: the re-read
        // misses and falls back) pin the unmoved shape.
        let mut store = FixtureStore::standard();
        store.post_drive_state = Some((uid(201), "Backlog".to_owned(), "backlog".to_owned()));
        let out = ok_value(
            call(
                &store,
                "create_issue",
                json!({
                    "project_id": uid(101).to_string(),
                    "name": "Bounced",
                    "state_id": uid(202).to_string(),
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!(true));
        assert_eq!(out["state"], json!("Backlog"));
        assert_eq!(out["state_group"], json!("backlog"));
        assert_eq!(
            *store.transition_to.lock().unwrap(),
            vec![(uid(202), "In Progress".to_owned(), "started".to_owned())]
        );
    }

    #[tokio::test]
    async fn create_issue_stateless_project_skips_transition() {
        // No states in the project at all: a non-transition (`prev ==
        // current`, `signals.py:79-80`) drives nothing.
        let mut store = FixtureStore::standard();
        store.states.insert(uid(101), Vec::new());
        let out = ok_value(
            call(
                &store,
                "create_issue",
                json!({
                    "project_id": uid(101).to_string(),
                    "name": "Stateless",
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!(true));
        assert_eq!(out["state"], Value::Null);
        assert!(store.transitions.lock().unwrap().is_empty());
        assert!(store.transition_to.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_issue_validation_and_scope_retries() {
        let store = FixtureStore::standard();
        let project = uid(101).to_string();
        for (params, message) in [
            (
                json!({"project_id": project.clone(), "name": "   "}),
                "An issue name is required.",
            ),
            (
                json!({"project_id": project.clone(), "name": "x", "priority": "bogus"}),
                "Priority must be one of ['high', 'low', 'medium', 'none', 'urgent'].",
            ),
        ] {
            let result = call(&store, "create_issue", params).await;
            assert_eq!(retry_message(&result), message);
        }
        let raw = uid(999).to_string();
        let result = call(
            &store,
            "create_issue",
            json!({"project_id": project.clone(), "name": "x", "state_id": raw.clone()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            format!("State {raw} is not a valid state for this project.")
        );
        // A parent outside the member scope is unresolvable.
        let result = call(
            &store,
            "create_issue",
            json!({"project_id": project, "name": "x", "parent_issue_id": uid(304).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            format!("Issue {} not found or not accessible.", uid(304))
        );
        // A member-but-read-only project denies the write.
        let result = call(
            &store,
            "create_issue",
            json!({"project_id": uid(103).to_string(), "name": "x"}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "You don't have permission to make changes in this project."
        );
        assert!(store.created_issues.lock().unwrap().is_empty());
        assert!(store.record_writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_issue_state_change_dispatches() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({
                    "issue_id": uid(301).to_string(),
                    "name": "Renamed",
                    "state_id": uid(202).to_string(),
                }),
            )
            .await,
        );
        assert_eq!(out["updated"], json!(true));
        assert_eq!(out["changed"], json!(["name", "state"]));
        assert_eq!(out["name"], json!("<untrusted>Renamed</untrusted>"));
        assert_eq!(out["state"], json!("In Progress"));
        assert_eq!(out["dispatched_run_id"], json!(uid(502).to_string()));
        let updated = store.updated_issues.lock().unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, uid(301));
        assert_eq!(updated[0].1.name.as_deref(), Some("Renamed"));
        assert!(updated[0].1.state_given);
        assert_eq!(updated[0].1.state_id, Some(uid(202)));
        assert!(!updated[0].1.parent_touched);
        assert_eq!(*store.transitions.lock().unwrap(), vec![uid(301)]);
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes[0].0, "Updated issue PROJ-1 (name, state)");
    }

    #[tokio::test]
    async fn update_issue_no_fields_retries_and_whitespace_name_is_silent() {
        let store = FixtureStore::standard();
        let result = call(
            &store,
            "update_issue",
            json!({"issue_id": uid(301).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Nothing to update \u{2014} provide at least one field to change."
        );
        // A whitespace-only name is given but unwritable: success with an
        // empty `changed` list, and the summary keeps its empty parens.
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({"issue_id": uid(301).to_string(), "name": "  "}),
            )
            .await,
        );
        assert_eq!(out["updated"], json!(true));
        assert_eq!(out["changed"], json!([]));
        assert!(out.get("dispatched_run_id").is_none());
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes[0].0, "Updated issue PROJ-1 ()");
    }

    #[tokio::test]
    async fn update_issue_parent_unlink_cycle_and_scope() {
        let store = FixtureStore::standard();
        // Explicit null unlinks (`parent_touched`, no new parent).
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({"issue_id": uid(302).to_string(), "parent_issue_id": Value::Null}),
            )
            .await,
        );
        assert_eq!(out["changed"], json!(["parent"]));
        assert_eq!(out["parent_id"], Value::Null);
        // Snapshot under the lock: the assertions below must not hold
        // the guard across the later calls (the executor re-locks
        // these mutexes, and `std::Mutex` is not re-entrant).
        let updated = store.updated_issues.lock().unwrap().clone();
        assert!(updated[0].1.parent_touched);
        assert_eq!(updated[0].1.parent_id, None);
        // Omitted `parent_issue_id` leaves the parent untouched.
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({"issue_id": uid(302).to_string(), "priority": "low"}),
            )
            .await,
        );
        assert_eq!(out["parent_id"], json!(uid(301).to_string()));
        // Self-parenting and cycles retry.
        let result = call(
            &store,
            "update_issue",
            json!({"issue_id": uid(301).to_string(), "parent_issue_id": uid(301).to_string()}),
        )
        .await;
        assert_eq!(retry_message(&result), "An issue can't be its own parent.");
        let result = call(
            &store,
            "update_issue",
            json!({"issue_id": uid(301).to_string(), "parent_issue_id": uid(302).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "That parent link would create a cycle."
        );
        // A parent in another project retries, even when visible.
        let result = call(
            &store,
            "update_issue",
            json!({"issue_id": uid(301).to_string(), "parent_issue_id": uid(305).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "The parent issue must be in the same project as the child issue."
        );
        // Bad priorities reuse the create message (`issues.py:311`).
        let result = call(
            &store,
            "update_issue",
            json!({"issue_id": uid(301).to_string(), "priority": "bogus"}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Priority must be one of ['high', 'low', 'medium', 'none', 'urgent']."
        );
    }

    // ------------------------------------------------------------------
    // Write tools — relations
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn relate_issues_creates_and_batches_activity() {
        let store = FixtureStore::standard();
        // Both pairs are clean (PROJ-1 already relates to PROJ-2/3 via
        // the fixture rows, so the source is PROJ-2).
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "relates_to",
                    "related_issues": ["PROJ-3", "NOWRITE-1"],
                }),
            )
            .await,
        );
        assert_eq!(out["issue"], json!("PROJ-2"));
        assert_eq!(out["relation_type"], json!("relates_to"));
        assert_eq!(out["created"], json!(["PROJ-3", "NOWRITE-1"]));
        assert_eq!(out["unchanged"], json!([]));
        assert_eq!(out["conflicts"], json!([]));
        assert_eq!(out["relations"]["relates_to"].as_array().unwrap().len(), 2);
        // One activity for the batch, carrying both target ids.
        let activities = store.activities.lock().unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(
            activities[0].activity_type,
            "issue_relation.activity.created"
        );
        assert_eq!(
            activities[0].requested_data.as_deref(),
            Some(
                format!(
                    "{{\"relation_type\": \"relates_to\", \"issues\": [\"{}\", \"{}\"]}}",
                    uid(303),
                    uid(305)
                )
                .as_str()
            ),
        );
        assert_eq!(activities[0].actor_id, uid(1));
        assert_eq!(activities[0].issue_id, uid(302));
        assert!(activities[0].current_instance.is_none());
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes[0].0, "Related PROJ-2 relates_to PROJ-3, NOWRITE-1");
    }

    #[tokio::test]
    async fn relate_issues_idempotent_conflict_reverse_and_errors() {
        let store = FixtureStore::standard();
        // The junk-typed twin row keeps this pair out of `unchanged`:
        // Python's verdict set spans every live row, so the pair
        // conflicts with the sorted-first existing relation.
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "blocked_by",
                    "related_issues": ["PROJ-1"],
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!([]));
        assert_eq!(out["unchanged"], json!([]));
        assert_eq!(
            out["conflicts"],
            json!([{"identifier": "PROJ-1", "existing_relation": "blocked_by"}])
        );
        assert!(store.activities.lock().unwrap().is_empty());
        assert!(store.record_writes.lock().unwrap().is_empty());
        // A different relation on the pair conflicts the same way
        // (existing named from the source's side).
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "relates_to",
                    "related_issues": ["PROJ-1"],
                }),
            )
            .await,
        );
        assert_eq!(
            out["conflicts"],
            json!([{"identifier": "PROJ-1", "existing_relation": "blocked_by"}])
        );
        // Reverse types store swapped under the forward name
        // (`_stored_edge`): the pair (PROJ-2, PROJ-3) is clean.
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "blocking",
                    "related_issues": ["PROJ-3"],
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!(["PROJ-3"]));
        let pairs = store.pairs.lock().unwrap().clone();
        // The new row, not the pre-existing (303, 301) twin.
        let stored = pairs
            .iter()
            .find(|pair| pair.issue_id == uid(303) && pair.related_issue_id == uid(302))
            .expect("stored row");
        assert_eq!(stored.relation_type, "blocked_by");
        // The legacy reverse-stored twin reads back as its own type.
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-3",
                    "relation_type": "blocking",
                    "related_issues": ["PROJ-1"],
                }),
            )
            .await,
        );
        assert_eq!(out["unchanged"], json!(["PROJ-1"]));
        // Errors: unknown type, self-relation, empty targets.
        let result = call(
            &store,
            "relate_issues",
            json!({
                "issue": "PROJ-1",
                "relation_type": "frobnicate",
                "related_issues": ["PROJ-2"],
            }),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "relation_type must be one of: blocked_by, blocking, relates_to, duplicate, start_before, start_after, finish_before, finish_after, implemented_by, implements"
        );
        let result = call(
            &store,
            "relate_issues",
            json!({
                "issue": "PROJ-1",
                "relation_type": "blocked_by",
                "related_issues": ["PROJ-1"],
            }),
        )
        .await;
        assert_eq!(retry_message(&result), "PROJ-1 cannot be related to itself");
        let result = call(
            &store,
            "relate_issues",
            json!({"issue": "PROJ-1", "relation_type": "blocked_by", "related_issues": []}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "related_issues must list at least one issue."
        );
    }

    #[tokio::test]
    async fn relate_issues_insert_race_rereads() {
        let store = FixtureStore::standard();
        *store.insert_race_once.lock().unwrap() = true;
        // The insert reports failure but the row lands: the re-read
        // classifies the pair as unchanged (`relations.relate :208-218`).
        let out = ok_value(
            call(
                &store,
                "relate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "relates_to",
                    "related_issues": ["PROJ-3"],
                }),
            )
            .await,
        );
        assert_eq!(out["created"], json!([]));
        assert_eq!(out["unchanged"], json!(["PROJ-3"]));
    }

    #[tokio::test]
    async fn unrelate_issues_removes_exact_type_only() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "unrelate_issues",
                json!({
                    "issue": "PROJ-2",
                    "relation_type": "blocked_by",
                    "related_issues": ["PROJ-1", "PROJ-3"],
                }),
            )
            .await,
        );
        assert_eq!(out["removed"], json!(["PROJ-1"]));
        assert_eq!(out["not_related"], json!(["PROJ-3"]));
        // Only row 601 is gone; the junk-typed 603 survives.
        let pairs = store.pairs.lock().unwrap().clone();
        assert!(!pairs.iter().any(|pair| pair.id == uid(601)));
        assert!(pairs.iter().any(|pair| pair.id == uid(603)));
        // Per removed target, both payloads (`relations.unrelate :258-265`).
        let activities = store.activities.lock().unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(
            activities[0].activity_type,
            "issue_relation.activity.deleted"
        );
        assert_eq!(
            activities[0].requested_data.as_deref(),
            Some(
                format!(
                    "{{\"relation_type\": \"blocked_by\", \"related_issue\": \"{}\"}}",
                    uid(301)
                )
                .as_str()
            ),
        );
        assert_eq!(
            activities[0].current_instance.as_deref(),
            Some("{\"relation_type\": \"blocked_by\"}")
        );
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes[0].0, "Unrelated PROJ-2 blocked_by PROJ-1");
    }

    // ------------------------------------------------------------------
    // Write tools — comments
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_comment_happy_path_and_guest_gate() {
        let store = FixtureStore::standard();
        let out = ok_value(
            call(
                &store,
                "create_comment",
                json!({"issue_id": uid(301).to_string(), "body_md": "hello", "fold": true}),
            )
            .await,
        );
        assert_eq!(out["created"], json!(true));
        assert_eq!(out["issue_id"], json!(uid(301).to_string()));
        assert_eq!(out["folded"], json!(true));
        assert!(out["comment_id"].as_str().is_some());
        let created = store.created_comments.lock().unwrap().clone();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].issue_id, uid(301));
        assert_eq!(created[0].project_id, uid(101));
        assert_eq!(created[0].actor_id, uid(1));
        assert_eq!(created[0].html, "<p>hello</p>");
        assert!(created[0].fold);
        let writes = store.record_writes.lock().unwrap().clone();
        assert_eq!(writes[0].0, "Commented on issue PROJ-1");
        // Empty bodies retry.
        let result = call(
            &store,
            "create_comment",
            json!({"issue_id": uid(301).to_string(), "body_md": "  "}),
        )
        .await;
        assert_eq!(retry_message(&result), "Comment body cannot be empty.");
    }

    #[tokio::test]
    async fn create_comment_guest_gate() {
        let store = FixtureStore::standard();
        // A guest may not comment on someone else's issue (`comments.py:39-46`).
        let mut deps = store.deps();
        deps.workspace_role = 5;
        let result = execute_local_tool(
            &store,
            &mut deps,
            &store.env(),
            "create_comment",
            &json!({"issue_id": uid(302).to_string(), "body_md": "hi"}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Guests can only comment on issues they created."
        );
        // ... but may on their own.
        let mut deps = store.deps();
        deps.workspace_role = 5;
        let out = ok_value(
            execute_local_tool(
                &store,
                &mut deps,
                &store.env(),
                "create_comment",
                &json!({"issue_id": uid(301).to_string(), "body_md": "hi"}),
            )
            .await,
        );
        assert_eq!(out["created"], json!(true));
    }

    // ------------------------------------------------------------------
    // Write tools — dispatch
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_coding_run_moves_to_default_delegated_state() {
        let store = FixtureStore::standard();
        // No target: the lowest-sequence ticking-groups state (In Progress).
        let out = ok_value(
            call(
                &store,
                "dispatch_coding_run",
                json!({"issue_id": uid(301).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "dispatched": true,
                "run_id": uid(502).to_string(),
                "new_state": "In Progress",
                "reason": "entered the ticking bucket",
            })
        );
        assert_eq!(*store.transitions.lock().unwrap(), vec![uid(301)]);
        let writes = store.record_writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            writes[0].0,
            "Started a coding run on PROJ-1 (moved to 'In Progress')"
        );
        // An explicit ticking target wins over the default.
        let out = ok_value(
            call(
                &store,
                "dispatch_coding_run",
                json!({"issue_id": uid(301).to_string(), "target_state_id": uid(203).to_string()}),
            )
            .await,
        );
        assert_eq!(out["new_state"], json!("In Review"));
    }

    #[tokio::test]
    async fn dispatch_coding_run_already_there_bad_targets_and_scope() {
        let store = FixtureStore::standard();
        // PROJ-2 already sits in the default target: no transition, no write.
        let out = ok_value(
            call(
                &store,
                "dispatch_coding_run",
                json!({"issue_id": uid(302).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "dispatched": false,
                "error": "already_in_state",
                "detail": "Issue is already in 'In Progress'.",
            })
        );
        assert!(store.transitions.lock().unwrap().is_empty());
        assert!(store.record_writes.lock().unwrap().is_empty());
        // Unknown target state.
        let result = call(
            &store,
            "dispatch_coding_run",
            json!({"issue_id": uid(301).to_string(), "target_state_id": uid(999).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "That state is not valid for this project."
        );
        // Valid but non-ticking target (Backlog).
        let result = call(
            &store,
            "dispatch_coding_run",
            json!({"issue_id": uid(301).to_string(), "target_state_id": uid(201).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "That state does not start a coding run. Pick a delegated state such as 'In Progress' or 'In Review'."
        );
        // Out-of-scope and read-only issues deny before any transition.
        let result = call(
            &store,
            "dispatch_coding_run",
            json!({"issue_id": uid(304).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            format!("Issue {} not found or not accessible.", uid(304))
        );
        let result = call(
            &store,
            "dispatch_coding_run",
            json!({"issue_id": uid(305).to_string()}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "You don't have permission to make changes in this project."
        );
    }

    #[tokio::test]
    async fn dispatch_coding_run_without_delegation_state_or_run() {
        let mut store = FixtureStore::standard();
        // A project whose states never tick: Backlog only.
        store.states.insert(
            uid(101),
            vec![state_row(201, "Backlog", "backlog", true, 1.0)],
        );
        let out = ok_value(
            call(
                &store,
                "dispatch_coding_run",
                json!({"issue_id": uid(301).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "dispatched": false,
                "error": "no_delegation_state",
                "detail": "This project has no state that triggers a coding run.",
            })
        );
        // A transition that creates no run still records the write
        // (`runs.py:85-90` runs before the `run_id is not None` check).
        let mut store = FixtureStore::standard();
        store.transition_outcome = TransitionOutcome {
            created_run_id: None,
            reason: "no runner free".to_owned(),
        };
        let out = ok_value(
            call(
                &store,
                "dispatch_coding_run",
                json!({"issue_id": uid(301).to_string()}),
            )
            .await,
        );
        assert_eq!(
            out,
            json!({
                "dispatched": false,
                "run_id": Value::Null,
                "new_state": "In Progress",
                "reason": "no runner free",
            })
        );
        assert_eq!(store.record_writes.lock().unwrap().len(), 1);
    }

    // ------------------------------------------------------------------
    // PR status
    // ------------------------------------------------------------------

    struct FakeResolver {
        addresses: HashMap<String, Vec<String>>,
    }

    impl pidash_services::assistant::ssrf::HostResolver for FakeResolver {
        fn resolve(
            &self,
            host: &str,
        ) -> Result<Vec<String>, pidash_services::assistant::ssrf::ResolveError> {
            self.addresses
                .get(host)
                .cloned()
                .ok_or(pidash_services::assistant::ssrf::ResolveError)
        }
    }

    fn pr_env(block_private: bool, addresses: HashMap<String, Vec<String>>) -> ToolEnv {
        ToolEnv {
            now: dt(13, 12),
            block_private_urls: block_private,
            ssrf_resolver: std::sync::Arc::new(FakeResolver { addresses }),
            ..ToolEnv::default()
        }
    }

    async fn pr_call(
        store: &FixtureStore,
        deps: &mut AssistantDeps,
        env: &ToolEnv,
        url: &str,
    ) -> Result<Value, ToolFailure> {
        execute_local_tool(
            store,
            deps,
            env,
            "get_pull_request_status",
            &json!({"url": url}),
        )
        .await
    }

    #[tokio::test]
    async fn pr_status_budget_unsupported_and_blocked() {
        let store = FixtureStore::standard();
        let env = pr_env(false, HashMap::new());
        // Exhausted budget answers without fetching.
        let mut deps = store.deps();
        deps.budget.pr_lookups = env.pr_lookups_cap;
        let out = ok_value(pr_call(&store, &mut deps, &env, "https://github.com/a/b/pull/1").await);
        assert_eq!(
            out,
            json!({"state": "unknown", "reason": "budget_exhausted"})
        );
        assert!(store.pr_fetch_calls.lock().unwrap().is_empty());
        // A live call consumes one lookup.
        let mut deps = store.deps();
        let out = ok_value(pr_call(&store, &mut deps, &env, "https://github.com/a/b/pull/1").await);
        assert_eq!(out, json!({"state": "unknown", "reason": "network_error"}));
        assert_eq!(deps.budget.pr_lookups, 1);
        // Unparseable URLs never fetch either (but still consume).
        let mut deps = store.deps();
        let out = ok_value(pr_call(&store, &mut deps, &env, "not a url").await);
        assert_eq!(
            out,
            json!({"state": "unknown", "reason": "unsupported_url"})
        );
        assert_eq!(deps.budget.pr_lookups, 1);
        // SSRF: unresolvable API hosts and private addresses block.
        for addresses in [
            HashMap::new(),
            HashMap::from([("api.github.com".to_owned(), vec!["10.0.0.1".to_owned()])]),
        ] {
            let store = FixtureStore::standard();
            let env = pr_env(true, addresses);
            let mut deps = store.deps();
            let out =
                ok_value(pr_call(&store, &mut deps, &env, "https://github.com/a/b/pull/1").await);
            assert_eq!(out, json!({"state": "unknown", "reason": "blocked"}));
            assert!(store.pr_fetch_calls.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn pr_status_token_paths() {
        // Binding token decrypts onto the request; the sync fallback is
        // not consulted when a binding row exists.
        let mut store = FixtureStore::standard();
        store.binding_configs.insert(
            ("github".to_owned(), "acme".to_owned(), "web".to_owned()),
            json!({"token": "enc-binding"}),
        );
        store.sync_creds.insert(
            ("acme".to_owned(), "web".to_owned()),
            json!({"access_token": "sync-plain"}),
        );
        store
            .decrypt_map
            .insert("enc-binding".to_owned(), "binding-plain".to_owned());
        store
            .decrypt_map
            .insert("sync-plain".to_owned(), "sync-plain".to_owned());
        let env = pr_env(false, HashMap::new());
        let mut deps = store.deps();
        let _ = pr_call(
            &store,
            &mut deps,
            &env,
            "https://github.com/Acme/Web/pull/42",
        )
        .await;
        let calls = store.pr_fetch_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "https://api.github.com/repos/acme/web/pulls/42");
        assert_eq!(
            calls[0].1,
            vec![
                ("Accept".to_owned(), "application/json".to_owned()),
                ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
                (
                    "Authorization".to_owned(),
                    "Bearer binding-plain".to_owned()
                ),
            ]
        );
        // A binding row with no usable token still wins outright: no
        // sync fallback (`github.py:63-65`).
        let mut store = FixtureStore::standard();
        store.binding_configs.insert(
            ("github".to_owned(), "acme".to_owned(), "web".to_owned()),
            json!({}),
        );
        store.sync_creds.insert(
            ("acme".to_owned(), "web".to_owned()),
            json!({"access_token": "sync-plain"}),
        );
        store
            .decrypt_map
            .insert("sync-plain".to_owned(), "sync-plain".to_owned());
        let mut deps = store.deps();
        let _ = pr_call(
            &store,
            &mut deps,
            &env,
            "https://github.com/acme/web/pull/42",
        )
        .await;
        let calls = store.pr_fetch_calls.lock().unwrap().clone();
        assert_eq!(
            calls[0].1,
            vec![("Accept".to_owned(), "application/json".to_owned())]
        );
        // Undecryptable stored tokens read back as `""` (the ported
        // `_maybe_decrypt` bug): the request goes out unauthenticated.
        let mut store = FixtureStore::standard();
        store.binding_configs.insert(
            ("github".to_owned(), "acme".to_owned(), "web".to_owned()),
            json!({"token": "junk"}),
        );
        let mut deps = store.deps();
        let _ = pr_call(
            &store,
            &mut deps,
            &env,
            "https://github.com/acme/web/pull/42",
        )
        .await;
        let calls = store.pr_fetch_calls.lock().unwrap().clone();
        assert_eq!(
            calls[0].1,
            vec![("Accept".to_owned(), "application/json".to_owned())]
        );
        // No binding: the sync fallback answers, preferring
        // `access_token` over `token` — first non-empty wins.
        for credentials in [
            json!({"access_token": "sync-plain", "token": "other"}),
            json!({"access_token": "", "token": "sync-plain"}),
        ] {
            let mut store = FixtureStore::standard();
            store
                .sync_creds
                .insert(("acme".to_owned(), "web".to_owned()), credentials);
            store
                .decrypt_map
                .insert("sync-plain".to_owned(), "sync-plain".to_owned());
            store
                .decrypt_map
                .insert("other".to_owned(), "other".to_owned());
            let mut deps = store.deps();
            let _ = pr_call(
                &store,
                &mut deps,
                &env,
                "https://github.com/acme/web/pull/42",
            )
            .await;
            let calls = store.pr_fetch_calls.lock().unwrap();
            assert_eq!(
                calls[0].1,
                vec![
                    ("Accept".to_owned(), "application/json".to_owned()),
                    ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
                    ("Authorization".to_owned(), "Bearer sync-plain".to_owned()),
                ]
            );
        }
        // GitLab tokens ride `PRIVATE-TOKEN`, never `Authorization`.
        let mut store = FixtureStore::standard();
        store.binding_configs.insert(
            ("gitlab".to_owned(), "acme".to_owned(), "web".to_owned()),
            json!({"token": "enc-gl"}),
        );
        store
            .decrypt_map
            .insert("enc-gl".to_owned(), "gl-plain".to_owned());
        let mut deps = store.deps();
        let _ = pr_call(
            &store,
            &mut deps,
            &env,
            "https://gitlab.com/acme/web/-/merge_requests/7",
        )
        .await;
        let calls = store.pr_fetch_calls.lock().unwrap();
        assert_eq!(
            calls[0].0,
            "https://gitlab.com/api/v4/projects/acme%2Fweb/merge_requests/7"
        );
        assert_eq!(
            calls[0].1,
            vec![
                ("Accept".to_owned(), "application/json".to_owned()),
                ("PRIVATE-TOKEN".to_owned(), "gl-plain".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn pr_status_response_mapping() {
        let env = pr_env(false, HashMap::new());
        for (fetch, reason) in [
            (PrFetch::NetworkError, "network_error"),
            (
                PrFetch::Fetched {
                    status: 404,
                    body: None,
                },
                "not_found",
            ),
            (
                PrFetch::Fetched {
                    status: 403,
                    body: None,
                },
                "rate_limited",
            ),
            (
                PrFetch::Fetched {
                    status: 429,
                    body: None,
                },
                "rate_limited",
            ),
            (
                PrFetch::Fetched {
                    status: 451,
                    body: None,
                },
                "blocked",
            ),
            (
                PrFetch::Fetched {
                    status: 500,
                    body: None,
                },
                "http_500",
            ),
            (
                PrFetch::Fetched {
                    status: 200,
                    body: None,
                },
                "bad_response",
            ),
        ] {
            let mut store = FixtureStore::standard();
            store.pr_fetch = fetch;
            let mut deps = store.deps();
            let out =
                ok_value(pr_call(&store, &mut deps, &env, "https://github.com/a/b/pull/1").await);
            assert_eq!(out, json!({"state": "unknown", "reason": reason}));
        }
        // Verdicts: merged wins over closed wins over open, with JSON
        // truthiness for `merged`/`merged_at` (`github.py:151-158`).
        for (body, state) in [
            (
                json!({"state": "open", "title": "T", "merged_at": Value::Null}),
                "open",
            ),
            (
                json!({"state": "closed", "title": "T", "merged_at": Value::Null}),
                "closed",
            ),
            (
                json!({"state": "closed", "merged": true, "title": "T"}),
                "merged",
            ),
            (
                json!({"state": "open", "merged_at": "2026-01-01", "title": "T"}),
                "merged",
            ),
            (json!({"state": "merged", "title": "T"}), "merged"),
            (
                json!({"state": "open", "merged": 1, "title": "T"}),
                "merged",
            ),
            (json!({"state": "open", "merged": 0, "title": "T"}), "open"),
            (json!({"title": "T"}), "open"),
        ] {
            let mut store = FixtureStore::standard();
            store.pr_fetch = PrFetch::Fetched {
                status: 200,
                body: Some(body.clone()),
            };
            let mut deps = store.deps();
            let out = ok_value(
                pr_call(
                    &store,
                    &mut deps,
                    &env,
                    "https://github.com/Acme/Web/pull/42",
                )
                .await,
            );
            assert_eq!(
                out,
                json!({
                    "state": state,
                    "title": body.get("title").cloned().unwrap_or(Value::Null),
                    "merged_at": body.get("merged_at").cloned().unwrap_or(Value::Null),
                    "url": "https://github.com/acme/web/pull/42",
                    "provider": "github",
                })
            );
        }
    }

    // ------------------------------------------------------------------
    // Dispatch, validation, and reference edges
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn unknown_tool_names_every_tool() {
        let store = FixtureStore::standard();
        let result = call(&store, "frobnicate", json!({})).await;
        assert_eq!(
            retry_message(&result),
            "Unknown tool name: 'frobnicate'. Available tools: 'create_comment', 'create_issue', 'dispatch_coding_run', 'get_issue', 'get_pull_request_status', 'get_run_status', 'list_issue_relations', 'list_issues', 'list_labels', 'list_my_issues', 'list_project_members', 'list_projects', 'list_states', 'relate_issues', 'search_issues', 'unrelate_issues', 'update_issue'"
        );
    }

    #[tokio::test]
    async fn invalid_args_carry_pydantic_details() {
        let store = FixtureStore::standard();
        // A missing required field fails before any store call.
        let result = call(&store, "search_issues", json!({})).await;
        match result {
            Err(ToolFailure::InvalidArgs { details }) => {
                assert_eq!(
                    details,
                    json!([{
                        "type": "missing",
                        "loc": ["query"],
                        "msg": "Field required",
                        "input": {},
                        "url": "https://errors.pydantic.dev/2.14/v/missing",
                    }])
                );
            }
            other => panic!("expected invalid args, got {other:?}"),
        }
        // Unknown tools never reach validation, even with bad args.
        let result = call(&store, "nope", json!({"a": 1})).await;
        assert!(retry_message(&result).starts_with("Unknown tool name: 'nope'."));
    }

    #[tokio::test]
    async fn missing_user_fails_internal() {
        let mut store = FixtureStore::standard();
        store.missing_user = true;
        // A missing user row escapes the tool (`User.DoesNotExist`),
        // failing the turn rather than retrying.
        let result = call(
            &store,
            "create_issue",
            json!({"project_id": uid(101).to_string(), "name": "x"}),
        )
        .await;
        assert_eq!(
            internal_message(&result),
            "User matching query does not exist."
        );
    }

    #[test]
    fn uuid_tokens_and_issue_refs() {
        // `uuid.UUID` grammar: braces, urn prefixes, case, hyphens.
        assert_eq!(
            parse_uuid_token("{00000000-0000-0000-0000-00000000012d}"),
            Some(uid(301))
        );
        assert_eq!(
            parse_uuid_token("urn:uuid:00000000-0000-0000-0000-00000000012D"),
            Some(uid(301))
        );
        assert_eq!(parse_uuid_token("PROJ-1"), None);
        assert_eq!(parse_uuid_token(""), None);
        // `int(hex, 16)` accepts single underscores between digits, so
        // a 32-char token with one interior underscore parses (a small
        // value with a leading zero nibble); leading, trailing, and
        // doubled underscores stay rejected.
        assert_eq!(
            parse_uuid_token("aaaaaaaaaaaaaaa_bbbbbbbbbbbbbbbb"),
            Some(Uuid::from_u128(0xaaaaaaaaaaaaaaabbbbbbbbbbbbbbbb))
        );
        assert_eq!(
            classify_issue_ref("aaaaaaaaaaaaaaa_bbbbbbbbbbbbbbbb"),
            IssueRef::Id(Uuid::from_u128(0xaaaaaaaaaaaaaaabbbbbbbbbbbbbbbb))
        );
        assert_eq!(parse_uuid_token("_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), None);
        assert_eq!(parse_uuid_token("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa_"), None);
        assert_eq!(parse_uuid_token("aaaaaaaaaaaaaa__bbbbbbbbbbbbbbbb"), None);
        // References: UUID, identifier at the last dash, unresolved.
        assert_eq!(
            classify_issue_ref(&uid(301).to_string()),
            IssueRef::Id(uid(301))
        );
        assert_eq!(
            classify_issue_ref("PROJ-1"),
            IssueRef::Identifier {
                project_code: "PROJ".to_owned(),
                sequence_id: 1
            }
        );
        assert_eq!(
            classify_issue_ref("MY-PROJ-12"),
            IssueRef::Identifier {
                project_code: "MY-PROJ".to_owned(),
                sequence_id: 12
            }
        );
        assert_eq!(classify_issue_ref("bogus"), IssueRef::Unresolved);
        assert_eq!(classify_issue_ref(""), IssueRef::Unresolved);
        // Beyond-i64 tails cannot match: miss, never error.
        assert_eq!(
            classify_issue_ref("PROJ-99999999999999999999999"),
            IssueRef::Unresolved
        );
        // Unknown refs stay unresolved through the tools.
        let store = FixtureStore::standard();
        let mut deps = store.deps();
        let out = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(execute_local_tool(
                &store,
                &mut deps,
                &store.env(),
                "relate_issues",
                &json!({
                    "issue": "PROJ-1",
                    "relation_type": "blocked_by",
                    "related_issues": ["PROJ-99999999999999999999999"],
                }),
            ));
        assert_eq!(
            retry_message(&out),
            "Issues not found or not accessible: PROJ-99999999999999999999999"
        );
    }

    // ------------------------------------------------------------------
    // Review fix pins (PIDASHCONV-1113 run 3)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn list_issues_filter_error_precedence() {
        // `work_item_list_filters` validates states → groups →
        // priorities → parents → labels; the first bad filter wins
        // (`issue_filters.py:593-641`).
        let store = FixtureStore::standard();
        let project = uid(101).to_string();
        for (params, message) in [
            // States still beat everything.
            (
                json!({"project_id": project.clone(), "state": "Nope", "state_group": "bogus"}),
                "Unknown state name(s): Nope. Valid states: Backlog, Done, In Progress, In Review.",
            ),
            // Groups beat priorities, parents, and labels.
            (
                json!({"project_id": project.clone(), "state_group": "bogus", "priority": "bogus"}),
                "Unknown state group(s): bogus. Valid groups: backlog, unstarted, started, review, test, completed, cancelled.",
            ),
            (
                json!({"project_id": project.clone(), "state_group": "bogus", "parent_issue_id": "bogus"}),
                "Unknown state group(s): bogus. Valid groups: backlog, unstarted, started, review, test, completed, cancelled.",
            ),
            // Priorities beat parents and labels.
            (
                json!({"project_id": project.clone(), "priority": "bogus", "labels": "nope"}),
                "Unknown priority value(s): bogus. Valid priorities: urgent, high, medium, low, none.",
            ),
            // The null-combo beats labels.
            (
                json!({"project_id": project.clone(), "parent_issue_id": "null,PROJ-1", "labels": "nope"}),
                "parent=null cannot be combined with specific parent issues.",
            ),
            // Parents beat labels.
            (
                json!({"project_id": project.clone(), "parent_issue_id": "bogus", "labels": "nope"}),
                "Invalid parent 'bogus': expected an issue UUID, an identifier like PROJ-123, or null.",
            ),
            (
                json!({"project_id": project.clone(), "parent_issue_id": "PROJ-999", "labels": "nope"}),
                "Unknown parent issue 'PROJ-999'.",
            ),
        ] {
            let result = call(&store, "list_issues", params).await;
            assert_eq!(retry_message(&result), message);
        }
    }

    #[tokio::test]
    async fn list_issues_unknown_state_names_empty_project() {
        // Only the labels branch has `or "(none)"`
        // (`issue_filters.py:515` vs `:543`): a project with no states
        // renders `Valid states: .`.
        let mut store = FixtureStore::standard();
        store.states.insert(uid(101), Vec::new());
        let result = call(
            &store,
            "list_issues",
            json!({"project_id": uid(101).to_string(), "state": "Nope"}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Unknown state name(s): Nope. Valid states: ."
        );
    }

    #[tokio::test]
    async fn update_issue_empty_state_on_stateless_project_skips_state() {
        // `if to_state is not None` (`issues.py:351`): `state_id: ""`
        // on a stateless project resolves to no state, so the arm is
        // skipped entirely — no write, no `changed` entry, no drive.
        let mut store = FixtureStore::standard();
        store.states.insert(uid(101), Vec::new());
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({
                    "issue_id": uid(301).to_string(),
                    "name": "Renamed",
                    "state_id": "",
                }),
            )
            .await,
        );
        assert_eq!(out["updated"], json!(true));
        assert_eq!(out["changed"], json!(["name"]));
        assert!(out.get("dispatched_run_id").is_none());
        let updated = store.updated_issues.lock().unwrap();
        assert_eq!(updated.len(), 1);
        assert!(!updated[0].1.state_given);
        assert_eq!(updated[0].1.state_id, None);
        assert!(store.transitions.lock().unwrap().is_empty());
        let writes = store.record_writes.lock().unwrap();
        assert_eq!(writes[0].0, "Updated issue PROJ-1 (name)");
    }

    #[tokio::test]
    async fn missing_user_fails_read_paths_internal() {
        // Every read path that reaches `scoped_issues`/`my_issues` /
        // `get_issue` hits `User.objects.get` (missing → internal),
        // positioned per Python order (`_scoping.py:39-64`).
        let mut store = FixtureStore::standard();
        store.missing_user = true;
        for (name, params) in [
            ("search_issues", json!({"query": "x"})),
            ("list_issues", json!({"project_id": uid(101).to_string()})),
            ("list_my_issues", json!({})),
            ("get_issue", json!({"issue_id": uid(301).to_string()})),
            ("get_run_status", json!({"issue_id": uid(301).to_string()})),
            ("list_issue_relations", json!({"issue": "PROJ-1"})),
        ] {
            let result = call(&store, name, params).await;
            assert_eq!(
                internal_message(&result),
                "User matching query does not exist.",
                "{name} must fail internal on a missing user"
            );
        }
        // The user check precedes the malformed-id translation on the
        // `get_issue` path (`_scoping.py:73-81`).
        let result = call(&store, "get_issue", json!({"issue_id": "bogus"})).await;
        assert_eq!(
            internal_message(&result),
            "User matching query does not exist."
        );
        // ... but filter validation still precedes it on the list path
        // (`issues.py:104-113`).
        let result = call(
            &store,
            "list_issues",
            json!({"project_id": uid(101).to_string(), "state": "Nope"}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Unknown state name(s): Nope. Valid states: Backlog, Done, In Progress, In Review."
        );
    }

    #[tokio::test]
    async fn missing_user_fails_write_paths_internal() {
        // The write paths reach `user_for` before their first retry-class
        // failure (`_scoping.py:67-85,104-107`): a missing user fails
        // internal even when validation, the role gate, or source
        // resolution would also fail.
        let mut store = FixtureStore::standard();
        store.missing_user = true;
        for (name, params) in [
            // `require_project_write` hits the user before validation.
            (
                "create_issue",
                json!({"project_id": uid(101).to_string(), "name": "x", "priority": "bogus"}),
            ),
            // ... and before the role gate.
            (
                "create_issue",
                json!({"project_id": uid(103).to_string(), "name": "x"}),
            ),
            // `get_issue` hits the user before the malformed-id translation.
            ("update_issue", json!({"issue_id": "bogus", "name": "x"})),
            // ... before the guest gate and the body check.
            (
                "create_comment",
                json!({"issue_id": uid(301).to_string(), "body_md": "   "}),
            ),
            ("dispatch_coding_run", json!({"issue_id": "bogus"})),
            // `_resolve_issue_refs` hits the user before source resolution.
            (
                "relate_issues",
                json!({
                    "issue": "PROJ-999",
                    "relation_type": "blocked_by",
                    "related_issues": ["PROJ-1"],
                }),
            ),
            (
                "unrelate_issues",
                json!({"issue": "bogus", "relation_type": "blocked_by", "related_issues": []}),
            ),
        ] {
            let result = call(&store, name, params).await;
            assert_eq!(
                internal_message(&result),
                "User matching query does not exist.",
                "{name} must fail internal on a missing user"
            );
        }
    }

    #[tokio::test]
    async fn unresolved_refs_report_stripped_or_raw() {
        // `ref or str(raw)` (`relations.py:160`): padded refs report
        // stripped, whitespace-only originals report verbatim.
        let store = FixtureStore::standard();
        let result = call(
            &store,
            "list_issue_relations",
            json!({"issue": "  PROJ-999  "}),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Issues not found or not accessible: PROJ-999"
        );
        let result = call(&store, "list_issue_relations", json!({"issue": "   "})).await;
        assert_eq!(
            retry_message(&result),
            "Issues not found or not accessible:    "
        );
        let result = call(
            &store,
            "relate_issues",
            json!({
                "issue": "PROJ-1",
                "relation_type": "blocked_by",
                "related_issues": ["  PROJ-999  ", "   "],
            }),
        )
        .await;
        assert_eq!(
            retry_message(&result),
            "Issues not found or not accessible: PROJ-999,    "
        );
    }

    #[tokio::test]
    async fn relate_target_store_error_propagates_internal() {
        // Only resolution misses join the unresolved list; a store
        // failure on a target propagates instead of masquerading as
        // "not found".
        let mut store = FixtureStore::standard();
        store.fail_identifier_lookups = true;
        let result = call(
            &store,
            "relate_issues",
            json!({
                "issue": uid(302).to_string(),
                "relation_type": "blocked_by",
                "related_issues": ["PROJ-1"],
            }),
        )
        .await;
        assert_eq!(internal_message(&result), "injected identifier failure");
    }

    #[tokio::test]
    async fn update_from_view_comes_from_locked_row() {
        // `from_state = locked.state` (`issues.py:339`): the drive's
        // from-view name/group come from the locked row, not the
        // pre-lock read (which a concurrent edit may have staled).
        let mut store = FixtureStore::standard();
        store.stale_pre_lock_state = true;
        let out = ok_value(
            call(
                &store,
                "update_issue",
                json!({
                    "issue_id": uid(301).to_string(),
                    "state_id": uid(202).to_string(),
                }),
            )
            .await,
        );
        assert_eq!(out["changed"], json!(["state"]));
        assert_eq!(
            *store.transition_from.lock().unwrap(),
            vec![Some((uid(201), "Backlog".to_owned(), "backlog".to_owned()))]
        );
    }

    #[tokio::test]
    async fn fixture_update_missing_issue_returns_missing() {
        // A lost race between the scope read and the lock is
        // `DoesNotExist` → internal, never a panic.
        let store = FixtureStore::standard();
        let error = store
            .update_issue_row(
                &IssuePatch::default(),
                &uid(999),
                None,
                &uid(1),
                &store.env().now,
            )
            .await
            .expect_err("unknown issue must miss");
        assert_eq!(error.message(), "Issue matching query does not exist.");
    }

    #[tokio::test]
    async fn unknown_tool_message_double_quotes_apostrophe() {
        // CPython `repr` quote choice: a value containing `'` but no
        // `"` wraps in double quotes, `'` unescaped.
        let store = FixtureStore::standard();
        let result = call(&store, "don't", json!({})).await;
        assert!(
            retry_message(&result)
                .starts_with("Unknown tool name: \"don't\". Available tools: 'create_comment'"),
            "got: {}",
            retry_message(&result)
        );
        // A value containing both quote kinds stays single-quoted
        // with the `'` escaped.
        let result = call(&store, "a'b\"c", json!({})).await;
        assert!(
            retry_message(&result).starts_with("Unknown tool name: 'a\\'b\"c'."),
            "got: {}",
            retry_message(&result)
        );
    }
}
