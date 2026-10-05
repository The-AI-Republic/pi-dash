//! Run web endpoints (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/runs.py:1-695`:
//!
//! * `AgentRunListEndpoint.get` (`:131-219`) → [`list_runs`]: the "my
//!   runs" involvement OR (creator, runner owner, issue creator, live
//!   assignee, workspace admin) over the member-workspace scope, the
//!   private-runner gate, workspace/project scopes, and the
//!   `{results,count,total_count,total_pages,page,per_page}` envelope.
//! * `AgentRunListEndpoint.post` (`:221-345`) → [`create_run`]:
//!   `comment_and_run` / `run_ai` / direct branches. Only the validation
//!   prefixes are owned (missing prompt / work_item 400s, issue + member
//!   404s, all pinned by FX-RUN-08); every dispatch tail proxies to
//!   Django (D-13 validation, D-12 scheduling, D-11 execution and D-04
//!   compose the tails, and only D-04/D-11 have landed).
//! * `AgentReTickEndpoint.post` (`:482-523`) → [`retick`]: the missing /
//!   malformed-UUID / not-found arms are owned; the `re_tick_ticker` tail
//!   (D-12) proxies.
//! * `AgentRunDetailEndpoint.get` (`:530-542`) → [`run_detail`], with
//!   `?include_events=1`.
//! * `AgentRunCancelEndpoint.post` (`:549-639`) → [`cancel_run`]: the
//!   cloud fast-path 202, terminal 409, cloud-duplicate stamp and the
//!   finalize-CANCELLED arm (via the L4 planners) are owned; the two arms
//!   that fan a cancel frame out (D-14 `send_to_runner`, PIDASHCONV-553)
//!   proxy.
//! * `AgentRunReleasePinEndpoint.post` (`:656-695`) → [`release_pin`]:
//!   the 404s and the cloud / not-queued / not-pinned 409s are owned; the
//!   unpin itself proxies (its `on_commit` drain is D-14,
//!   PIDASHCONV-552 — proxy-before-write, since Django re-decides under
//!   its own lock).
//!
//! Pagination ([`pidash_services::runner_runs::guards::parse_pagination`]),
//! the view/cancel guard ([`guards::can_view_run`], same set for cancel)
//! and every wire shape ([`shape`]) are L3; the cancel-finalize SQL comes
//! from the L4 [`finalization`] planners, executed here following the L6a
//! sweeps precedent (positional binds, terminal event, post-commit
//! `runner.apply_agent_run_terminal_effects` enqueue — the inline-apply
//! arm has no live provider yet, same as the sweeps).
//!
//! Django order is preserved throughout: session authN (401) before any
//! body or branch decision; the bad-UUID path proxy before auth (URL
//! resolution runs first); run lookup + guard before the cancel reason is
//! read (so a missing run 404s even with a malformed body).
//!
//! [`guards`]: pidash_services::runner_runs::guards
//! [`shape`]: pidash_services::runner_runs::shape
//! [`finalization`]: pidash_services::runner_runs::finalization

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::runner::{can_view_runner, RunnerFacts};
use pidash_auth::permissions::ROLE_ADMIN;
use pidash_db::runner_runs::agent_run;
use pidash_db::runner_runs::event;
use pidash_db::runner_runs::tool_call;
use pidash_db::runner_runs::{AgentRun, AgentRunEvent, AgentRunToolCall};
use pidash_services::runner_enroll::serializers::shapes::PodMiniRow;
use pidash_services::runner_runs::finalization;
use pidash_services::runner_runs::guards;
use pidash_services::runner_runs::shape;
use pidash_services::runner_runs::SetValue;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, ToolCallStatus};
use pidash_types::WorkspaceId;

use super::{is_uuid_path_segment, json_response, pool_of, proxy_with_body, SERVER_ERROR_BODY};
use crate::license::{query_last, resolve_actor, QueryMap, UNAUTHENTICATED_BODY};
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies (only arms this port answers; proxied arms are Django's)
// ---------------------------------------------------------------------------

/// Direct-create without a prompt (`runs.py:240-244`).
pub const PROMPT_REQUIRED_BODY: &str = r#"{"error":"prompt is required"}"#;
/// `run_ai` without `work_item` (`runs.py:360-364`).
pub const RUN_AI_WORK_ITEM_REQUIRED_BODY: &str = r#"{"error":"work_item is required for run_ai"}"#;
/// `comment_and_run` without `work_item` (`runs.py:431-436`).
pub const COMMENT_RUN_WORK_ITEM_REQUIRED_BODY: &str =
    r#"{"error":"work_item is required for comment_and_run"}"#;
/// Re-tick without `work_item` (`runs.py:489-493`).
pub const RETICK_WORK_ITEM_REQUIRED_BODY: &str = r#"{"error":"work_item is required for re-tick"}"#;
/// Re-tick with a non-UUID `work_item` (`runs.py:494-502`).
pub const RETICK_MALFORMED_BODY: &str = r#"{"error":"invalid work_item UUID format"}"#;
/// Issue lookup / membership miss on the three issue-bound creates
/// (`runs.py:367/369/439/441/505/507`).
pub const ISSUE_NOT_FOUND_BODY: &str = r#"{"error":"issue not found"}"#;
/// Cancel on a terminal run (`runs.py:572-576`).
pub const RUN_ALREADY_TERMINAL_BODY: &str =
    r#"{"error":"run already terminal","code":"run_already_terminal"}"#;
/// Release-pin on a Cloud Agent run (`runs.py:662-666`).
pub const EXECUTOR_NOT_LOCAL_BODY: &str =
    r#"{"error":"Cloud Agent runs cannot be pinned","code":"executor_not_local"}"#;
/// Release-pin on a non-QUEUED run (`runs.py:672-676`).
pub const RUN_NOT_QUEUED_BODY: &str = r#"{"error":"run not queued"}"#;
/// Release-pin on an unpinned run (`runs.py:677-681`).
pub const RUN_NOT_PINNED_BODY: &str = r#"{"error":"run not pinned"}"#;

/// Default cancel reason (`runs.py:560`).
pub const DEFAULT_CANCEL_REASON: &str = "cancelled by user";
/// `reason[:512]` counts code points (`runs.py:560`).
pub const CANCEL_REASON_MAX_CHARS: usize = 512;

// ---------------------------------------------------------------------------
// Python-semantics kernels (local twins of the `pub(crate)` L4 helpers)
// ---------------------------------------------------------------------------

/// Python truthiness for a JSON request value (`runs.py:222/360/433/489/560`).
/// Missing (`None`) is falsy, like every `request.data.get` default.
pub fn py_truthy_json(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().map(|float| float != 0.0).unwrap_or(false)
            }
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// Python `str.strip()` (no args): Rust's `White_Space` plus U+001C–U+001F,
/// which CPython's `Py_UNICODE_ISSPACE` strips but the Unicode property
/// does not (verified against CPython 3.12). Same formula as the L4 twin.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// First `max` Unicode code points of `text` (`text[:max]`, `runs.py:560`).
/// Byte-slicing would panic on a UTF-8 boundary; `chars().take()` cannot.
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

/// What `POST runs/` does with `triggered_by`
/// (`(request.data.get("triggered_by") or "").strip()`, `runs.py:222`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggeredBy {
    CommentAndRun,
    RunAi,
    /// Missing, null, falsy, or a stripped string naming no branch.
    Direct,
    /// Truthy non-string (`5`, `true`, `[..]`, `{..}`): `.strip()`
    /// raises `AttributeError` (Django 500) — proxy so Django raises it.
    ProxyInvalid,
}

/// Classify `triggered_by` without raising.
pub fn classify_triggered_by(body: &Value) -> TriggeredBy {
    let raw = body.get("triggered_by");
    if !py_truthy_json(raw) {
        return TriggeredBy::Direct;
    }
    let Some(text) = raw.and_then(Value::as_str) else {
        return TriggeredBy::ProxyInvalid;
    };
    match py_strip(text) {
        "comment_and_run" => TriggeredBy::CommentAndRun,
        "run_ai" => TriggeredBy::RunAi,
        _ => TriggeredBy::Direct,
    }
}

/// What cancel does with the body `reason`
/// (`(request.data.get("reason") or "cancelled by user")[:512]`,
/// `runs.py:560`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelReason {
    /// Missing, null, or falsy (`""`, `0`, `false`, `[]`, `{}`).
    Default,
    /// A string, truncated to 512 code points.
    Text(String),
    /// Truthy non-string: `[:512]` raises `TypeError` (Django 500).
    ProxyInvalid,
}

/// Classify the cancel reason without raising.
pub fn classify_cancel_reason(body: &Value) -> CancelReason {
    let raw = body.get("reason");
    if !py_truthy_json(raw) {
        return CancelReason::Default;
    }
    match raw.and_then(Value::as_str) {
        Some(text) => CancelReason::Text(truncate_chars(text, CANCEL_REASON_MAX_CHARS)),
        None => CancelReason::ProxyInvalid,
    }
}

/// `uuid.UUID(str(work_item_id))` (`runs.py:495`): CPython strips
/// `urn:`/`uuid:` fragments anywhere, `{}` braces at the ends, and ALL
/// hyphens, requires 32 chars, then runs `int(x, 16)` — which also
/// tolerates surrounding whitespace and one leading `+` (a leading `-`
/// parses but fails the UUID range check). Single `_` between digits is
/// the one tolerated form this port rejects (pathological; documented).
pub fn parse_py_uuid(raw: &str) -> Option<Uuid> {
    let stripped = raw.replace("urn:", "").replace("uuid:", "");
    let stripped = stripped
        .trim_start_matches(['{', '}'])
        .trim_end_matches(['{', '}']);
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.len() != 32 {
        return None;
    }
    // `int(x, 16)` strips surrounding whitespace and one leading `+`
    // before parsing (a leading `-` parses but the UUID range check
    // rejects it — verified against CPython 3.12).
    let trimmed =
        compact.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
    let trimmed = trimmed.strip_prefix('+').unwrap_or(trimmed);
    if trimmed.is_empty() || trimmed.len() > 32 {
        return None;
    }
    if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // Zero-pad short values exactly like `int(x, 16)` → 128 bits.
    let padded = format!("{trimmed:0>32}");
    Uuid::parse_str(&format!(
        "{}-{}-{}-{}-{}",
        &padded[0..8],
        &padded[8..12],
        &padded[12..16],
        &padded[16..20],
        &padded[20..32]
    ))
    .ok()
}

/// Re-tick `work_item` handling (`runs.py:489-502`): falsy → missing 400;
/// strings and numbers go through `uuid.UUID(str(value))` (an integer's
/// decimal text can itself be 32 hex digits; floats render with `.`/`e`
/// and fail the parse naturally); bools and containers render a `str()`
/// that can never parse → malformed 400 directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemRef {
    Missing,
    Malformed,
    Id(Uuid),
}

/// Classify the re-tick `work_item` without raising.
pub fn classify_retick_work_item(body: &Value) -> WorkItemRef {
    let raw = body.get("work_item");
    if !py_truthy_json(raw) {
        return WorkItemRef::Missing;
    }
    match raw {
        Some(Value::String(text)) => match parse_py_uuid(text) {
            Some(id) => WorkItemRef::Id(id),
            None => WorkItemRef::Malformed,
        },
        // `str(number)`: integers render decimal (a 32-digit int is
        // itself 32 hex digits); floats render with `.`/`e`/`inf` and
        // can never parse, so they fall out as Malformed naturally.
        Some(Value::Number(number)) => match parse_py_uuid(&number.to_string()) {
            Some(id) => WorkItemRef::Id(id),
            None => WorkItemRef::Malformed,
        },
        _ => WorkItemRef::Malformed,
    }
}

/// List `total_pages`: `max(1, ceil(total_count / per_page))`
/// (`runs.py:207`). `per_page >= 1` by construction (L3 clamp).
pub fn total_pages(total_count: i64, per_page: i64) -> i64 {
    ((total_count + per_page - 1) / per_page).max(1)
}

// ---------------------------------------------------------------------------
// SQL builders (Django compiler shapes from the FX-RUN-08 `list.sql`,
// `%s` → `$N`; joins/aliases mirror the fixture)
// ---------------------------------------------------------------------------

/// `"agent_run"."<col>", ...` in [`agent_run::COLUMNS`] order: the
/// positional run-image prefix every run-row query shares.
pub fn run_select_list() -> String {
    agent_run::COLUMNS
        .iter()
        .map(|column| format!(r#""agent_run"."{column}""#))
        .collect::<Vec<_>>()
        .join(", ")
}

/// List joins: the involvement OR's three `LEFT OUTER` legs plus the
/// `select_related("pod__project")` inner pair (`runs.py:157-182`).
/// No `pod.deleted_at` filter: forward-FK joins never carry the related
/// model's default-manager scope.
pub const LIST_JOINS: &str = concat!(
    r#"LEFT OUTER JOIN "runner" ON ("agent_run"."runner_id" = "runner"."id") "#,
    r#"LEFT OUTER JOIN "issues" ON ("agent_run"."work_item_id" = "issues"."id") "#,
    r#"LEFT OUTER JOIN "issue_assignees" ON ("issues"."id" = "issue_assignees"."issue_id") "#,
    r#"INNER JOIN "pod" ON ("agent_run"."pod_id" = "pod"."id") "#,
    r#"INNER JOIN "projects" ON ("pod"."project_id" = "projects"."id")"#,
);

/// List `WHERE`: member-workspace scope AND the five-clause involvement
/// OR AND the private-runner gate (`runs.py:157-173`), then the optional
/// workspace / project scopes (`:184-197`). Params `$1..=$8` are the
/// requester id (repeated); scope params follow in argument order, then
/// `LIMIT` / `OFFSET` for the page query. `role = 20` is the admin
/// literal (`ROLE_ADMIN`); the member subqueries carry no `is_active`
/// filter (the view reads `WorkspaceMember.objects` directly — only the
/// `is_workspace_member` / `workspace_role` helpers add it).
pub fn list_where(has_workspace_scope: bool, has_project_scope: bool) -> String {
    let mut next_param = 9;
    let mut extra = String::new();
    if has_workspace_scope {
        extra.push_str(&format!(
            r#" AND "agent_run"."workspace_id" = ${next_param}::uuid"#
        ));
        next_param += 1;
    }
    if has_project_scope {
        extra.push_str(&format!(r#" AND "pod"."project_id" = ${next_param}::uuid"#));
    }
    format!(
        concat!(
            r#"("agent_run"."workspace_id" IN "#,
            r#"(SELECT U0."workspace_id" FROM "workspace_members" U0 "#,
            r#"WHERE (U0."deleted_at" IS NULL AND U0."member_id" = $1)) "#,
            r#"AND ("agent_run"."created_by_id" = $2 OR "runner"."owner_id" = $3 "#,
            r#"OR "issues"."created_by_id" = $4 "#,
            r#"OR ("issue_assignees"."assignee_id" = $5 AND "issue_assignees"."deleted_at" IS NULL) "#,
            r#"OR "agent_run"."workspace_id" IN "#,
            r#"(SELECT U0."workspace_id" FROM "workspace_members" U0 "#,
            r#"WHERE (U0."deleted_at" IS NULL AND U0."member_id" = $6 AND U0."role" = 20))) "#,
            r#"AND ("agent_run"."runner_id" IS NULL OR "runner"."owner_id" = $7 "#,
            r#"OR "agent_run"."created_by_id" = $8){extra})"#,
        ),
        extra = extra,
    )
}

/// List count (`runs.py:206`): Django counts `DISTINCT` full rows over
/// the trimmed (LEFT-only) joins; `COUNT(DISTINCT id)` over the same
/// `WHERE` is result-identical (`id` is the PK; the pod/project inner
/// pair is 1:1 and fans nothing out). Binds: `$1..=$8` requester, then
/// the raw scope strings in argument order (`::uuid` cast: invalid text
/// errors like Django's `ValidationError`, a 500 either way).
pub fn list_count_sql(has_workspace_scope: bool, has_project_scope: bool) -> String {
    format!(
        r#"SELECT COUNT(DISTINCT "agent_run"."id") FROM "agent_run" {joins} WHERE {where_}"#,
        joins = LIST_JOINS,
        where_ = list_where(has_workspace_scope, has_project_scope),
    )
}

/// List page (`runs.py:205-209`): `DISTINCT` run image plus the pod-mini
/// columns, `-created_at`, `LIMIT` / `OFFSET` bound last.
pub fn list_page_sql(has_workspace_scope: bool, has_project_scope: bool) -> String {
    let mut next_param = 9 + usize::from(has_workspace_scope) + usize::from(has_project_scope);
    let limit_param = next_param;
    next_param += 1;
    let offset_param = next_param;
    format!(
        concat!(
            r#"SELECT DISTINCT {runs}, "pod"."id" AS "mini_id", "#,
            r#""pod"."name" AS "mini_name", "#,
            r#""pod"."is_default" AS "mini_is_default", "#,
            r#""pod"."project_id" AS "mini_project_id", "#,
            r#""projects"."identifier" AS "mini_project_identifier" "#,
            r#"FROM "agent_run" {joins} WHERE {where_} "#,
            r#"ORDER BY "agent_run"."created_at" DESC LIMIT ${limit} OFFSET ${offset}"#,
        ),
        runs = run_select_list(),
        joins = LIST_JOINS,
        where_ = list_where(has_workspace_scope, has_project_scope),
        limit = limit_param,
        offset = offset_param,
    )
}

/// `prefetch_related("tool_calls")` (`runs.py:180`): one query, no
/// `ORDER BY` (the model carries no `Meta.ordering`). Param: `$1` the
/// page's run ids.
pub fn tool_calls_for_runs_sql() -> String {
    let columns = tool_call::COLUMNS
        .iter()
        .map(|column| format!(r#""agent_run_tool_call"."{column}""#))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"SELECT {columns} FROM "agent_run_tool_call" WHERE "agent_run_tool_call"."agent_run_id" = ANY($1)"#,
        columns = columns,
    )
}

/// Detail / cancel / release-pin lookup: the run image plus the guard
/// facts (`select_related("runner", "work_item")`, `runs.py:531/553/657`).
/// Param: `$1` the run id.
pub fn run_detail_sql() -> String {
    format!(
        concat!(
            r#"SELECT {runs}, "#,
            r#""runner"."owner_id" AS "gate_runner_owner_id", "#,
            r#""runner"."visibility" AS "gate_runner_visibility", "#,
            r#""issues"."created_by_id" AS "gate_work_item_created_by_id" "#,
            r#"FROM "agent_run" "#,
            r#"LEFT OUTER JOIN "runner" ON ("agent_run"."runner_id" = "runner"."id") "#,
            r#"LEFT OUTER JOIN "issues" ON ("agent_run"."work_item_id" = "issues"."id") "#,
            r#"WHERE "agent_run"."id" = $1 LIMIT 1"#,
        ),
        runs = run_select_list(),
    )
}

/// `workspace_role` (`core/permissions.py:37-46`): the live active role,
/// or no row. Params: `$1` workspace, `$2` member.
pub fn membership_role_sql() -> String {
    r#"SELECT "workspace_members"."role" FROM "workspace_members" WHERE ("workspace_members"."workspace_id" = $1 AND "workspace_members"."member_id" = $2 AND "workspace_members"."deleted_at" IS NULL AND "workspace_members"."is_active") LIMIT 1"#.to_owned()
}

/// Live through-model assignee check (`runs.py:115`): the default
/// manager excludes soft-deleted rows, so un-assignment withdraws the
/// grant. Params: `$1` issue, `$2` assignee.
pub fn live_assignee_exists_sql() -> String {
    r#"SELECT EXISTS(SELECT 1 FROM "issue_assignees" WHERE ("issue_assignees"."issue_id" = $1 AND "issue_assignees"."assignee_id" = $2 AND "issue_assignees"."deleted_at" IS NULL))"#.to_owned()
}

/// Issue lookup for the issue-bound arms (`Issue.all_objects`, no
/// soft-delete scope, `runs.py:365/437/503`). Param: `$1` the issue id.
pub fn issue_lookup_sql() -> String {
    r#"SELECT "issues"."id", "issues"."workspace_id" FROM "issues" WHERE "issues"."id" = $1 LIMIT 1"#.to_owned()
}

/// Pod-mini columns for one pod (`pod_detail`, no deleted scope —
/// forward joins never filter). Param: `$1` the pod id.
pub fn pod_mini_sql() -> String {
    r#"SELECT "pod"."id", "pod"."name", "pod"."is_default", "pod"."project_id", "projects"."identifier" FROM "pod" INNER JOIN "projects" ON ("pod"."project_id" = "projects"."id") WHERE "pod"."id" = $1 LIMIT 1"#.to_owned()
}

/// `include_events` (`runs.py:540`): `ORDER BY seq`, first 500.
/// Param: `$1` the run id.
pub fn run_events_sql() -> String {
    let columns = event::COLUMNS
        .iter()
        .map(|column| format!(r#""agent_run_event"."{column}""#))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"SELECT {columns} FROM "agent_run_event" WHERE "agent_run_event"."agent_run_id" = $1 ORDER BY "agent_run_event"."seq" ASC LIMIT 500"#,
        columns = columns,
    )
}

/// Cloud fast-path stamp (`runs.py:562-564`): single-statement
/// first-writer check. Params: `$1` now, `$2` reason, `$3` id,
/// `$4` `running`.
pub fn cloud_fast_stamp_sql() -> String {
    r#"UPDATE "agent_run" SET "cancel_requested_at" = $1, "cancel_reason" = $2 WHERE ("agent_run"."id" = $3 AND "agent_run"."status" = $4 AND "agent_run"."cancel_requested_at" IS NULL)"#.to_owned()
}

/// Row lock for the cancel / release-pin branches
/// (`select_for_update().filter(id).first()`, `runs.py:569/669`).
/// Param: `$1` the run id.
pub fn lock_run_sql() -> String {
    format!(
        r#"SELECT {runs} FROM "agent_run" WHERE "agent_run"."id" = $1 LIMIT 1 FOR UPDATE"#,
        runs = run_select_list(),
    )
}

/// Cloud-duplicate stamp (`locked.save(update_fields=[...])`,
/// `runs.py:588-591`). Params: `$1` now, `$2` reason, `$3` id.
pub fn cloud_dup_stamp_sql() -> String {
    r#"UPDATE "agent_run" SET "cancel_requested_at" = $1, "cancel_reason" = $2 WHERE "agent_run"."id" = $3"#.to_owned()
}

/// Expected `SET` order for the cancel finalize (`finalize_agent_run`
/// base five plus `runs.py:617-623` extras in dict order). The live
/// bind below is positional; a mismatch fails loudly instead of
/// binding wrong (the L6a sweeps precedent).
pub const CANCEL_FINALIZE_COLUMNS: [&str; 9] = [
    "status",
    "ended_at",
    "queue_position",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "cancel_requested_at",
    "cancel_reason",
    "error_code",
    "error",
];

// ---------------------------------------------------------------------------
// Row decoding (positional run image à la the L6a sweeps precedent: the
// `SELECT` lists [`agent_run::COLUMNS`] first, extras decode by name)
// ---------------------------------------------------------------------------

/// What can go wrong decoding a run row: transport, or a
/// model-violating choice value on the still-typed enums (Django
/// renders those raw; the L3 shapes need typed enums, so they 500
/// here — documented). The trigger is exempt: it rides through as
/// the raw stored string, like Django reads it.
#[derive(Debug)]
pub enum RowError {
    Sql(sqlx::Error),
    Choice { column: &'static str, value: String },
}

impl From<sqlx::Error> for RowError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sql(error)
    }
}

/// Decode the 41-column run image at `base..base+41` into the L2 row.
pub fn decode_run(row: &sqlx::postgres::PgRow, base: usize) -> Result<AgentRun, RowError> {
    use sqlx::Row;
    let status: String = row.try_get(base + 10)?;
    let status = AgentRunStatus::from_value(&status).ok_or_else(|| RowError::Choice {
        column: "status",
        value: status,
    })?;
    let executor_kind: String = row.try_get(base + 11)?;
    let executor_kind =
        AgentExecutorKind::from_value(&executor_kind).ok_or_else(|| RowError::Choice {
            column: "executor_kind",
            value: executor_kind,
        })?;
    // The trigger rides through unparsed: Django's `TextChoices`
    // are choices-only (no DB check), so the stored value may sit
    // outside the trigger enum (runner migration 0029) and every
    // read path must tolerate it.
    let trigger: String = row.try_get(base + 20)?;
    Ok(AgentRun {
        id: row.try_get(base)?,
        workspace_id: row.try_get(base + 1)?,
        owner_id: row.try_get(base + 2)?,
        created_by_id: row.try_get(base + 3)?,
        pod_id: row.try_get(base + 4)?,
        runner_id: row.try_get(base + 5)?,
        pinned_runner_id: row.try_get(base + 6)?,
        work_item_id: row.try_get(base + 7)?,
        scheduler_binding_id: row.try_get(base + 8)?,
        parent_run_id: row.try_get(base + 9)?,
        status,
        executor_kind,
        dispatch_attempts: row.try_get(base + 12)?,
        cancel_requested_at: row.try_get(base + 13)?,
        cancel_reason: row.try_get(base + 14)?,
        error_code: row.try_get(base + 15)?,
        tool_plan: row.try_get(base + 16)?,
        terminal_hooks_applied_at: row.try_get(base + 17)?,
        terminal_capacity_released_at: row.try_get(base + 18)?,
        prompt: row.try_get(base + 19)?,
        trigger,
        prompt_manifest: row.try_get(base + 21)?,
        phase_kind: row.try_get(base + 22)?,
        run_config: row.try_get(base + 23)?,
        required_capabilities: row.try_get(base + 24)?,
        thread_id: row.try_get(base + 25)?,
        agent_metadata: row.try_get(base + 26)?,
        lease_expires_at: row.try_get(base + 27)?,
        done_payload: row.try_get(base + 28)?,
        error: row.try_get(base + 29)?,
        refusal_category: row.try_get(base + 30)?,
        llm_model: row.try_get(base + 31)?,
        usage: row.try_get(base + 32)?,
        input_tokens: row.try_get(base + 33)?,
        output_tokens: row.try_get(base + 34)?,
        total_tokens: row.try_get(base + 35)?,
        created_at: row.try_get(base + 36)?,
        assigned_at: row.try_get(base + 37)?,
        queue_position: row.try_get(base + 38)?,
        started_at: row.try_get(base + 39)?,
        ended_at: row.try_get(base + 40)?,
    })
}

/// One tool-call row in [`tool_call::COLUMNS`] order.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ToolCallRecord {
    pub id: Uuid,
    pub agent_run_id: Uuid,
    pub tool_call_id: String,
    pub source: String,
    pub server_key: String,
    pub tool_name: String,
    pub risk: String,
    pub status: String,
    pub request_fingerprint: String,
    pub result_fingerprint: String,
    pub idempotency_key_hash: String,
    pub external_operation_id: String,
    pub safe_replay_result: Option<Value>,
    pub error_code: String,
    pub prepared_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl ToolCallRecord {
    pub fn into_tool_call(self) -> Result<AgentRunToolCall, RowError> {
        let status = ToolCallStatus::from_value(&self.status).ok_or_else(|| RowError::Choice {
            column: "tool_call.status",
            value: self.status.clone(),
        })?;
        Ok(AgentRunToolCall {
            id: self.id,
            agent_run_id: self.agent_run_id,
            tool_call_id: self.tool_call_id,
            source: self.source,
            server_key: self.server_key,
            tool_name: self.tool_name,
            risk: self.risk,
            status,
            request_fingerprint: self.request_fingerprint,
            result_fingerprint: self.result_fingerprint,
            idempotency_key_hash: self.idempotency_key_hash,
            external_operation_id: self.external_operation_id,
            safe_replay_result: self.safe_replay_result,
            error_code: self.error_code,
            prepared_at: self.prepared_at,
            submitted_at: self.submitted_at,
            completed_at: self.completed_at,
        })
    }
}

/// One run-event row in [`event::COLUMNS`] order.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RunEventRecord {
    pub id: i64,
    pub agent_run_id: Uuid,
    pub seq: i32,
    pub kind: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl RunEventRecord {
    pub fn into_event(self) -> AgentRunEvent {
        AgentRunEvent {
            id: self.id,
            agent_run_id: self.agent_run_id,
            seq: self.seq,
            kind: self.kind,
            payload: self.payload,
            created_at: self.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared handler plumbing
// ---------------------------------------------------------------------------

/// 401 for anonymous callers on guarded endpoints (DRF `NotAuthenticated`).
fn unauthorized() -> Response {
    json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
}

/// 500 for transport / model-violation failures on owned branches.
fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
}

impl From<RowError> for Response {
    fn from(_: RowError) -> Self {
        server_error()
    }
}

/// Resolve the request actor: pool, then the Django session
/// (`ModelBackend` UUID + user row + active + session hash). Anonymous
/// (no session, stale session, wrong backend) is `Ok(None)` → 401.
#[allow(clippy::result_large_err)]
async fn request_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Option<crate::license::Actor>, Response> {
    let pool = pool_of(state)?;
    resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| server_error())
}

/// `is_workspace_member` (`core/permissions.py:28-34`): a live active
/// role row exists. (The `permissions::membership` kernel the L3 docs
/// name is not on the auth module tree, so the one-predicate check
/// lives here, like the scheduler-gate role comparisons.)
fn is_workspace_member(role: Option<i32>) -> bool {
    role.is_some()
}

/// `is_workspace_admin` (`core/permissions.py:61-64`): role `>= ADMIN`.
fn is_workspace_admin(role: Option<i32>) -> bool {
    matches!(role, Some(role) if role >= ROLE_ADMIN)
}

/// The caller's live active role in `workspace_id`
/// (`workspace_role`, `core/permissions.py:37-46`).
#[allow(clippy::result_large_err)]
async fn workspace_role(
    pool: &PgPool,
    workspace_id: Uuid,
    member_id: Uuid,
) -> Result<Option<i32>, Response> {
    let row: Option<(i16,)> = sqlx::query_as(&membership_role_sql())
        .bind(workspace_id)
        .bind(member_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(row.map(|row| i32::from(row.0)))
}

/// Live through-model assignee row (`runs.py:115`).
#[allow(clippy::result_large_err)]
async fn has_live_assignee(
    pool: &PgPool,
    issue_id: Uuid,
    assignee_id: Uuid,
) -> Result<bool, Response> {
    let exists: Option<bool> = sqlx::query_scalar(&live_assignee_exists_sql())
        .bind(issue_id)
        .bind(assignee_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(exists.unwrap_or(false))
}

/// Collect the request body for parse-then-proxy handlers.
#[allow(clippy::result_large_err)]
async fn collect_body(body: axum::body::Body) -> Result<bytes::Bytes, Response> {
    use http_body_util::BodyExt;
    body.collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| server_error())
}

/// Pod-mini strings backing one [`PodMiniRow`]: UUIDs render to owned
/// text first (the L3 view borrows `&str`).
pub struct PodMiniStrings {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub project: String,
    pub project_identifier: String,
}

impl PodMiniStrings {
    pub fn as_row(&self) -> PodMiniRow<'_> {
        PodMiniRow {
            id: &self.id,
            name: &self.name,
            is_default: self.is_default,
            project: &self.project,
            project_identifier: &self.project_identifier,
        }
    }
}

/// Fetch the pod-mini columns for `pod_id` (hard-missing pod → 500,
/// mirroring Django's `RelatedObjectDoesNotExist`).
#[allow(clippy::result_large_err)]
async fn pod_mini_for(pool: &PgPool, pod_id: Uuid) -> Result<PodMiniStrings, Response> {
    let row: Option<(Uuid, String, bool, Uuid, String)> = sqlx::query_as(&pod_mini_sql())
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some((id, name, is_default, project, project_identifier)) = row else {
        return Err(server_error());
    };
    Ok(PodMiniStrings {
        id: id.to_string(),
        name,
        is_default,
        project: project.to_string(),
        project_identifier,
    })
}

/// Fetch one run's tool calls (no ordering — the model has none).
#[allow(clippy::result_large_err)]
async fn tool_calls_for(pool: &PgPool, run_id: Uuid) -> Result<Vec<AgentRunToolCall>, Response> {
    tool_calls_for_many(pool, &[run_id])
        .await
        .map(|mut grouped| grouped.remove(&run_id).unwrap_or_default())
}

/// Fetch tool calls for many runs, grouped by run (list prefetch).
#[allow(clippy::result_large_err)]
async fn tool_calls_for_many(
    pool: &PgPool,
    run_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<AgentRunToolCall>>, Response> {
    use std::collections::HashMap;
    if run_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let records: Vec<ToolCallRecord> = sqlx::query_as(&tool_calls_for_runs_sql())
        .bind(run_ids)
        .fetch_all(pool)
        .await
        .map_err(|_| server_error())?;
    let mut grouped: HashMap<Uuid, Vec<AgentRunToolCall>> = HashMap::new();
    for record in records {
        let run_id = record.agent_run_id;
        let call = record.into_tool_call()?;
        grouped.entry(run_id).or_default().push(call);
    }
    Ok(grouped)
}

/// Render one run through the detail shape (single-object serializer:
/// `error_diagnostic` runs the classifier).
fn render_detail(run: &AgentRun, pod: &PodMiniStrings, tool_calls: &[AgentRunToolCall]) -> Value {
    let pod_row = pod.as_row();
    let view = shape::run_detail_to_representation(run, &pod_row, tool_calls);
    serde_json::to_value(&view).expect("serializable run view")
}

/// Render one run through the list shape (`error_diagnostic` is null).
fn render_list_row(run: &AgentRun, pod: &PodMiniStrings, tool_calls: &[AgentRunToolCall]) -> Value {
    let pod_row = pod.as_row();
    let view = shape::run_list_to_representation(run, &pod_row, tool_calls);
    serde_json::to_value(&view).expect("serializable run view")
}

// ---------------------------------------------------------------------------
// `GET /api/runners/runs/` — "my runs" list (`runs.py:131-219`)
// ---------------------------------------------------------------------------

/// List the caller's runs: involvement OR over the member-workspace
/// scope, private-runner gate, workspace/project scopes, page-number
/// pagination and the six-key envelope. Fully owned.
pub async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let page_raw = query_last(&query, "page");
    let per_page_raw = query_last(&query, "per_page");
    let (page, per_page) = guards::parse_pagination(page_raw.as_deref(), per_page_raw.as_deref());
    let workspace_scope = query_last(&query, "workspace").filter(|value| !value.is_empty());
    let project_scope = query_last(&query, "project").filter(|value| !value.is_empty());
    // `if workspace_id:` / `if project_id:` — empty strings scope nothing.
    let has_workspace_scope = workspace_scope.is_some();
    let has_project_scope = project_scope.is_some();

    let count_sql = list_count_sql(has_workspace_scope, has_project_scope);
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for _ in 0..8 {
        count_query = count_query.bind(actor.id);
    }
    if let Some(scope) = &workspace_scope {
        count_query = count_query.bind(scope.as_str());
    }
    if let Some(scope) = &project_scope {
        count_query = count_query.bind(scope.as_str());
    }
    let total_count: i64 = match count_query.fetch_one(pool).await {
        Ok(count) => count,
        Err(_) => return server_error(),
    };
    let pages = total_pages(total_count, per_page);
    // `page` is unbounded (`max(1, huge)`); saturate the offset into
    // `i64` — a huge page reads an empty slice either way.
    let offset = i128::from(page - 1) * i128::from(per_page);
    let offset = offset.min(i128::from(i64::MAX)) as i64;

    let page_sql = list_page_sql(has_workspace_scope, has_project_scope);
    let mut page_query = sqlx::query(&page_sql);
    for _ in 0..8 {
        page_query = page_query.bind(actor.id);
    }
    if let Some(scope) = &workspace_scope {
        page_query = page_query.bind(scope.as_str());
    }
    if let Some(scope) = &project_scope {
        page_query = page_query.bind(scope.as_str());
    }
    let rows = match page_query.bind(per_page).bind(offset).fetch_all(pool).await {
        Ok(rows) => rows,
        Err(_) => return server_error(),
    };
    let mut runs: Vec<(AgentRun, PodMiniStrings)> = Vec::with_capacity(rows.len());
    for row in &rows {
        use sqlx::Row;
        let run = match decode_run(row, 0) {
            Ok(run) => run,
            Err(error) => return error.into(),
        };
        let mini_id: Uuid = match row.try_get("mini_id") {
            Ok(id) => id,
            Err(_) => return server_error(),
        };
        let mini_name: String = match row.try_get("mini_name") {
            Ok(name) => name,
            Err(_) => return server_error(),
        };
        let mini_is_default: bool = match row.try_get("mini_is_default") {
            Ok(flag) => flag,
            Err(_) => return server_error(),
        };
        let mini_project_id: Uuid = match row.try_get("mini_project_id") {
            Ok(id) => id,
            Err(_) => return server_error(),
        };
        let mini_project_identifier: String = match row.try_get("mini_project_identifier") {
            Ok(identifier) => identifier,
            Err(_) => return server_error(),
        };
        runs.push((
            run,
            PodMiniStrings {
                id: mini_id.to_string(),
                name: mini_name,
                is_default: mini_is_default,
                project: mini_project_id.to_string(),
                project_identifier: mini_project_identifier,
            },
        ));
    }
    let run_ids: Vec<Uuid> = runs.iter().map(|(run, _)| run.id).collect();
    let tool_calls = match tool_calls_for_many(pool, &run_ids).await {
        Ok(grouped) => grouped,
        Err(response) => return response,
    };
    let empty_calls = Vec::new();
    let results: Vec<Value> = runs
        .iter()
        .map(|(run, pod)| {
            let calls = tool_calls.get(&run.id).unwrap_or(&empty_calls);
            render_list_row(run, pod, calls)
        })
        .collect();
    // `count` is the page length, not the total (`runs.py:213`).
    let count = results.len();
    let body = serde_json::json!({
        "results": results,
        "count": count,
        "total_count": total_count,
        "total_pages": pages,
        "page": page,
        "per_page": per_page,
    });
    json_response(StatusCode::OK, body.to_string())
}

// ---------------------------------------------------------------------------
// `GET /api/runners/runs/<run_id>/` — detail (`runs.py:530-542`)
// ---------------------------------------------------------------------------

/// Guard facts resolved for one run row: membership/admin from the live
/// role, runner facts from the joined runner, involvement from the
/// joined issue plus the through-model assignee check.
#[allow(clippy::result_large_err)]
async fn view_facts_for(
    pool: &PgPool,
    actor_id: Uuid,
    run: &AgentRun,
    runner_owner_id: Option<Uuid>,
    runner_visibility: Option<i16>,
    work_item_created_by_id: Option<Uuid>,
) -> Result<guards::RunViewFacts, Response> {
    // Runner set but the join missed (FK violation): Django raises on
    // `run.runner.owner_id` — a 500.
    if run.runner_id.is_some() && runner_owner_id.is_none() {
        return Err(server_error());
    }
    if run.work_item_id.is_some() && work_item_created_by_id.is_none() {
        return Err(server_error());
    }
    let role = workspace_role(pool, run.workspace_id, actor_id).await?;
    let runner = run.runner_id.map(|_| {
        let owned = runner_owner_id == Some(actor_id);
        let visible = can_view_runner(&RunnerFacts {
            workspace: WorkspaceId::new(run.workspace_id.to_string()),
            authenticated: true,
            visibility: i32::from(runner_visibility.unwrap_or(0)),
            owned_by_requester: owned,
        });
        guards::RunRunnerGate {
            owned_by_requester: owned,
            visible_to_requester: visible,
        }
    });
    let work_item_created_by_requester = work_item_created_by_id.map(|creator| creator == actor_id);
    let live_assignee = match run.work_item_id {
        Some(issue_id) => has_live_assignee(pool, issue_id, actor_id).await?,
        None => false,
    };
    Ok(guards::RunViewFacts {
        is_workspace_member: is_workspace_member(role),
        created_by_requester: run.created_by_id == actor_id,
        runner,
        work_item_created_by_requester,
        live_assignee,
        is_workspace_admin: is_workspace_admin(role),
    })
}

/// Fetch one run plus its guard-join facts, or `None` when missing.
pub struct GuardedRun {
    pub run: AgentRun,
    pub runner_owner_id: Option<Uuid>,
    pub runner_visibility: Option<i16>,
    pub work_item_created_by_id: Option<Uuid>,
}

/// `select_related("runner", "work_item").filter(id).first()`.
#[allow(clippy::result_large_err)]
pub async fn fetch_guarded_run(
    pool: &PgPool,
    run_id: Uuid,
) -> Result<Option<GuardedRun>, Response> {
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&run_detail_sql())
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let run = decode_run(&row, 0).map_err(Response::from)?;
    let runner_owner_id: Option<Uuid> = row
        .try_get("gate_runner_owner_id")
        .map_err(|_| server_error())?;
    let runner_visibility: Option<i16> = row
        .try_get("gate_runner_visibility")
        .map_err(|_| server_error())?;
    let work_item_created_by_id: Option<Uuid> = row
        .try_get("gate_work_item_created_by_id")
        .map_err(|_| server_error())?;
    Ok(Some(GuardedRun {
        run,
        runner_owner_id,
        runner_visibility,
        work_item_created_by_id,
    }))
}

/// Run detail, plus `events` (by `seq`, first 500) when
/// `?include_events=1`. Missing and forbidden both 404
/// (`{"error": "not found"}`) so existence never leaks. Fully owned.
pub async fn run_detail(
    State(state): State<AppState>,
    Path(run_raw): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if !is_uuid_path_segment(&run_raw) {
        return crate::edge::proxy(State(state), req).await;
    }
    let run_id = run_raw.parse::<Uuid>().expect("checked segment");
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let guarded = match fetch_guarded_run(pool, run_id).await {
        Ok(Some(guarded)) => guarded,
        Ok(None) => return not_found(),
        Err(response) => return response,
    };
    let facts = match view_facts_for(
        pool,
        actor.id,
        &guarded.run,
        guarded.runner_owner_id,
        guarded.runner_visibility,
        guarded.work_item_created_by_id,
    )
    .await
    {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    if !guards::can_view_run(&facts) {
        return not_found();
    }
    let pod = match pod_mini_for(pool, guarded.run.pod_id).await {
        Ok(pod) => pod,
        Err(response) => return response,
    };
    let tool_calls = match tool_calls_for(pool, guarded.run.id).await {
        Ok(calls) => calls,
        Err(response) => return response,
    };
    let mut payload = render_detail(&guarded.run, &pod, &tool_calls);
    if query_last(&query, "include_events").as_deref() == Some("1") {
        let records: Vec<RunEventRecord> = match sqlx::query_as(&run_events_sql())
            .bind(guarded.run.id)
            .fetch_all(pool)
            .await
        {
            Ok(records) => records,
            Err(_) => return server_error(),
        };
        let events: Vec<Value> = records
            .into_iter()
            .map(|record| {
                let event = record.into_event();
                let view = shape::run_event_to_representation(&event);
                serde_json::to_value(&view).expect("serializable event view")
            })
            .collect();
        // Appended last, like `payload["events"] = ...` (`runs.py:541`).
        if let Some(object) = payload.as_object_mut() {
            object.insert("events".to_owned(), Value::Array(events));
        }
    }
    json_response(StatusCode::OK, payload.to_string())
}

/// The 404-not-403 denial (L3): missing and forbidden share
/// `{"error": "not found"}` (`runs.py:533/536/555/557/659/661`).
fn not_found() -> Response {
    let denial = guards::run_not_found();
    let status = StatusCode::from_u16(denial.status).unwrap_or(StatusCode::NOT_FOUND);
    (status, Json(denial.body)).into_response()
}

// ---------------------------------------------------------------------------
// `POST /api/runners/runs/` — create (`runs.py:221-466`)
// ---------------------------------------------------------------------------

/// Issue-bound create prefix (`_post_run_ai` / `_post_comment_and_run`
/// through the membership check, `runs.py:359-369/431-442`): missing
/// `work_item` 400, unknown issue 404, non-member 404 — then `Ok(None)`
/// tells the caller to proxy the dispatch tail (D-12 scheduling).
/// A truthy non-string `work_item` proxies too: `filter(pk=123)`
/// raises Django's `ValidationError` (500), like the malformed create
/// arm the contract pins. issue row comes from `all_objects` (no
/// soft-delete scope). `Ok(())` tells the caller to proxy the tail.
#[allow(clippy::result_large_err)]
async fn issue_bound_prefix(
    pool: &PgPool,
    actor_id: Uuid,
    body: &Value,
    missing_body: &'static str,
) -> Result<(), Response> {
    let raw = body.get("work_item");
    if !py_truthy_json(raw) {
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            missing_body.to_owned(),
        ));
    }
    let Some(text) = raw.and_then(Value::as_str) else {
        return Ok(());
    };
    let Ok(work_item_id) = text.parse::<Uuid>() else {
        // `filter(pk="nope")` → `ValidationError` → Django 500.
        return Ok(());
    };
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(&issue_lookup_sql())
        .bind(work_item_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some((_, workspace_id)) = row else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            ISSUE_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let role = workspace_role(pool, workspace_id, actor_id).await?;
    if !is_workspace_member(role) {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            ISSUE_NOT_FOUND_BODY.to_owned(),
        ));
    }
    Ok(())
}

/// Create a run: the three `triggered_by` branches. Validation
/// prefixes are owned; every dispatch tail proxies (providers: D-13
/// validation for direct, D-12 scheduling for `run_ai` /
/// `comment_and_run`).
pub async fn create_run(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (parts, body) = req.into_parts();
    let bytes = match collect_body(body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    let parsed: Value = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        // Django answers the DRF `ParseError` for the same bytes.
        Err(_) => return proxy_with_body(state, parts, bytes).await,
    };
    // `request.data` is always a dict on this path: form/multipart
    // bodies parse to one in DRF. A JSON non-object has no
    // `.get` — proxy and let Django's parsers decide.
    if !parsed.is_object() {
        return proxy_with_body(state, parts, bytes).await;
    }
    match classify_triggered_by(&parsed) {
        TriggeredBy::ProxyInvalid => {
            return proxy_with_body(state, parts, bytes).await;
        }
        TriggeredBy::CommentAndRun => {
            match issue_bound_prefix(pool, actor.id, &parsed, COMMENT_RUN_WORK_ITEM_REQUIRED_BODY)
                .await
            {
                Ok(_) => return proxy_with_body(state, parts, bytes).await,
                Err(response) => return response,
            }
        }
        TriggeredBy::RunAi => {
            match issue_bound_prefix(pool, actor.id, &parsed, RUN_AI_WORK_ITEM_REQUIRED_BODY).await
            {
                Ok(_) => return proxy_with_body(state, parts, bytes).await,
                Err(response) => return response,
            }
        }
        TriggeredBy::Direct => {}
    }
    if !py_truthy_json(parsed.get("prompt")) {
        return json_response(StatusCode::BAD_REQUEST, PROMPT_REQUIRED_BODY.to_owned());
    }
    proxy_with_body(state, parts, bytes).await
}

// ---------------------------------------------------------------------------
// `POST /api/runners/re-tick/` — budget grant (`runs.py:482-523`)
// ---------------------------------------------------------------------------

/// Re-tick: missing / malformed-UUID / not-found arms are owned; the
/// `re_tick_ticker` tail (D-12) proxies.
pub async fn retick(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (parts, body) = req.into_parts();
    let bytes = match collect_body(body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    let parsed: Value = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => return proxy_with_body(state, parts, bytes).await,
    };
    if !parsed.is_object() {
        return proxy_with_body(state, parts, bytes).await;
    }
    let work_item_id = match classify_retick_work_item(&parsed) {
        WorkItemRef::Missing => {
            return json_response(
                StatusCode::BAD_REQUEST,
                RETICK_WORK_ITEM_REQUIRED_BODY.to_owned(),
            );
        }
        WorkItemRef::Malformed => {
            return json_response(StatusCode::BAD_REQUEST, RETICK_MALFORMED_BODY.to_owned());
        }
        WorkItemRef::Id(id) => id,
    };
    let row: Option<(Uuid, Uuid)> = match sqlx::query_as(&issue_lookup_sql())
        .bind(work_item_id)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some((_, workspace_id)) = row else {
        return json_response(StatusCode::NOT_FOUND, ISSUE_NOT_FOUND_BODY.to_owned());
    };
    let role = match workspace_role(pool, workspace_id, actor.id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !is_workspace_member(role) {
        return json_response(StatusCode::NOT_FOUND, ISSUE_NOT_FOUND_BODY.to_owned());
    }
    proxy_with_body(state, parts, bytes).await
}

// ---------------------------------------------------------------------------
// `POST /api/runners/runs/<run_id>/cancel/` (`runs.py:549-639`)
// ---------------------------------------------------------------------------

/// Cancel: lookup + guard first (404s), then the reason, then the
/// cloud fast-path 202 and the locked branches. The two
/// cancel-requested arms proxy (their `on_commit` fan-out is D-14);
/// everything else — terminal 409, cloud-duplicate stamp, finalize —
/// is owned.
pub async fn cancel_run(
    State(state): State<AppState>,
    Path(run_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if !is_uuid_path_segment(&run_raw) {
        return crate::edge::proxy(State(state), req).await;
    }
    let run_id = run_raw.parse::<Uuid>().expect("checked segment");
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    // Lookup + guard BEFORE the body is read (`runs.py:553-560`): a
    // missing run 404s even with a malformed body.
    let guarded = match fetch_guarded_run(pool, run_id).await {
        Ok(Some(guarded)) => guarded,
        Ok(None) => return not_found(),
        Err(response) => return response,
    };
    let facts = match view_facts_for(
        pool,
        actor.id,
        &guarded.run,
        guarded.runner_owner_id,
        guarded.runner_visibility,
        guarded.work_item_created_by_id,
    )
    .await
    {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    if !guards::can_cancel_run(&facts) {
        return not_found();
    }
    let (parts, body) = req.into_parts();
    let bytes = match collect_body(body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    let parsed: Value = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => return proxy_with_body(state, parts, bytes).await,
    };
    if !parsed.is_object() {
        return proxy_with_body(state, parts, bytes).await;
    }
    let reason = match classify_cancel_reason(&parsed) {
        CancelReason::Default => DEFAULT_CANCEL_REASON.to_owned(),
        CancelReason::Text(reason) => reason,
        CancelReason::ProxyInvalid => {
            return proxy_with_body(state, parts, bytes).await;
        }
    };

    // Cloud fast-path (`runs.py:561-567`): stamp-only, 202. A lost race
    // (0 rows) falls through to the locked branches — no early return.
    if guarded.run.executor_kind == AgentExecutorKind::CloudAgent
        && guarded.run.status == AgentRunStatus::Running
    {
        let stamped = sqlx::query(&cloud_fast_stamp_sql())
            .bind(Utc::now())
            .bind(reason.as_str())
            .bind(run_id)
            .bind(AgentRunStatus::Running.value())
            .execute(pool)
            .await;
        match stamped {
            Ok(done) if done.rows_affected() == 1 => {
                return render_cancelled(pool, run_id, StatusCode::ACCEPTED).await;
            }
            Ok(_) => {}
            Err(_) => return server_error(),
        }
    }

    cancel_locked(pool, &state, parts, bytes, run_id, &reason).await
}

/// Render the refreshed run through the detail shape after a cancel
/// write (`run.refresh_from_db()` + `AgentRunSerializer(run)`).
async fn render_cancelled(pool: &PgPool, run_id: Uuid, status: StatusCode) -> Response {
    let guarded = match fetch_guarded_run(pool, run_id).await {
        Ok(Some(guarded)) => guarded,
        Ok(None) => return not_found(),
        Err(response) => return response,
    };
    let pod = match pod_mini_for(pool, guarded.run.pod_id).await {
        Ok(pod) => pod,
        Err(response) => return response,
    };
    let tool_calls = match tool_calls_for(pool, guarded.run.id).await {
        Ok(calls) => calls,
        Err(response) => return response,
    };
    let payload = render_detail(&guarded.run, &pod, &tool_calls);
    json_response(status, payload.to_string())
}

/// The locked cancel branches (`runs.py:568-639`). Proxy arms roll the
/// probe transaction back first — Django re-decides under its own lock.
async fn cancel_locked(
    pool: &PgPool,
    state: &AppState,
    parts: http::request::Parts,
    bytes: bytes::Bytes,
    run_id: Uuid,
    reason: &str,
) -> Response {
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let row: Option<sqlx::postgres::PgRow> = match sqlx::query(&lock_run_sql())
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        let _ = tx.rollback().await;
        return not_found();
    };
    let locked = match decode_run(&row, 0) {
        Ok(run) => run,
        Err(error) => {
            let _ = tx.rollback().await;
            return error.into();
        }
    };
    if locked.status.is_terminal() {
        let _ = tx.rollback().await;
        return json_response(StatusCode::CONFLICT, RUN_ALREADY_TERMINAL_BODY.to_owned());
    }
    if locked.status == AgentRunStatus::CancelRequested {
        // Runnerless cancel-requested rows never fan out
        // (`if runner_id and ...`, `runs.py:631`) — owned 200.
        if locked.runner_id.is_none() {
            let _ = tx.rollback().await;
            return render_cancelled(pool, run_id, StatusCode::OK).await;
        }
        let _ = tx.rollback().await;
        return proxy_with_body(state.clone(), parts, bytes).await;
    }
    if locked.executor_kind == AgentExecutorKind::CloudAgent
        && locked.status == AgentRunStatus::Running
    {
        // Cloud duplicate (`runs.py:579-592`): record the request and let
        // the worker's cancellation poll finalize. 200, never a send
        // (the status stays RUNNING).
        if locked.cancel_requested_at.is_none() {
            let stamped = sqlx::query(&cloud_dup_stamp_sql())
                .bind(Utc::now())
                .bind(reason)
                .bind(run_id)
                .execute(&mut *tx)
                .await;
            if stamped.is_err() {
                let _ = tx.rollback().await;
                return server_error();
            }
        }
        if tx.commit().await.is_err() {
            return server_error();
        }
        return render_cancelled(pool, run_id, StatusCode::OK).await;
    }
    if locked.executor_kind != AgentExecutorKind::CloudAgent
        && locked.runner_id.is_some()
        && !matches!(
            locked.status,
            AgentRunStatus::Queued | AgentRunStatus::PausedAwaitingInput
        )
    {
        // Local cancel-requested (`runs.py:593-611`): the status flip
        // fans a cancel frame out on commit (D-14) — proxy.
        let _ = tx.rollback().await;
        return proxy_with_body(state.clone(), parts, bytes).await;
    }
    // Settle-cancel (`runs.py:612-626`): `finalize_agent_run` inline via
    // the L4 planners, then the refreshed row, 200.
    match finalize_cancelled(&mut tx, &locked, reason).await {
        Ok(()) => {}
        Err(response) => {
            let _ = tx.rollback().await;
            return response;
        }
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    // `_publish_effects` (`finalization.py:89-103`): the Celery emit —
    // isolated, logged on failure. The inline-apply arm has no live
    // provider yet (L6b wires it); the forwarded task carries the
    // effects, same as the sweeps.
    let job = pidash_jobs::runner_runs::sweeps_runs::terminal_effects_job(&run_id);
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::error!(
            %error,
            run_id = %run_id,
            "failed to publish terminal effects for run",
        );
    }
    render_cancelled(pool, run_id, StatusCode::OK).await
}

/// `finalize_agent_run(locked.id, CANCELLED, updates)` for settle-cancel
/// (`runs.py:615-624` + `finalization.py:48-86`): the nested `atomic()`
/// is a savepoint elided into the caller's transaction (the L6a shape);
/// the lock-miss arm is kept although the probe lock above already holds
/// this row, and the return is ignored like the source ignores it.
#[allow(clippy::result_large_err)]
async fn finalize_cancelled(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    locked: &AgentRun,
    reason: &str,
) -> Result<(), Response> {
    // `cancel_requested_at` samples its own `now()` at the call site
    // (`runs.py:619`); `ended_at` samples another inside finalize
    // (`finalization.py:52`). Two samples, like the source.
    let requested_at = Utc::now();
    let values = finalization::plan_finalize_values(
        AgentRunStatus::Cancelled,
        &[
            ("cancel_requested_at", SetValue::Now),
            ("cancel_reason", SetValue::Text(reason.to_owned())),
            ("error_code", SetValue::Text("cancelled".to_owned())),
            ("error", SetValue::Text(reason.to_owned())),
        ],
    )
    .map_err(|_| server_error())?;
    let columns: Vec<&str> = values.clauses.iter().map(|clause| clause.column).collect();
    if columns.as_slice() != CANCEL_FINALIZE_COLUMNS {
        return Err(server_error());
    }
    let lock_sql = finalization::lock_run_for_finalize_sql(false, false);
    let mut lock = sqlx::query(&lock_sql).bind(locked.id);
    for status in pidash_types::runner_runs::TERMINAL_RUN_STATUSES {
        lock = lock.bind(status.value());
    }
    let found = lock
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if found.is_none() {
        // Unreachable (the probe holds this non-terminal row); the
        // source ignores the `False` return and renders on.
        return Ok(());
    }
    sqlx::query(&finalization::finalize_update_sql(&values))
        .bind(AgentRunStatus::Cancelled.value())
        .bind(Utc::now())
        .bind(None::<i16>)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<DateTime<Utc>>)
        .bind(requested_at)
        .bind(reason)
        .bind("cancelled")
        .bind(reason)
        .bind(locked.id)
        .execute(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    if locked.executor_kind == AgentExecutorKind::CloudAgent {
        let exists = sqlx::query_scalar::<_, i32>(finalization::terminal_event_exists_sql())
            .bind(locked.id)
            .bind("terminal")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
        if exists.is_none() {
            let max_seq = sqlx::query_scalar::<_, i32>(finalization::terminal_event_max_seq_sql())
                .bind(locked.id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|_| server_error())?;
            let plan = finalization::plan_terminal_event(
                max_seq,
                AgentRunStatus::Cancelled,
                &finalization::finalize_error_code(&values),
            );
            sqlx::query(&finalization::terminal_event_insert_sql())
                .bind(locked.id)
                .bind(plan.seq)
                .bind("terminal")
                .bind(plan.payload)
                .bind(Utc::now())
                .execute(&mut **tx)
                .await
                .map_err(|_| server_error())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `POST /api/runners/runs/<run_id>/release-pin/` (`runs.py:656-695`)
// ---------------------------------------------------------------------------

/// Release-pin: the 404s and the cloud / not-queued / not-pinned 409s
/// are owned; the unpin itself proxies before any write (its
/// `on_commit` drain is D-14 — Django re-decides under its own lock).
pub async fn release_pin(
    State(state): State<AppState>,
    Path(run_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if !is_uuid_path_segment(&run_raw) {
        return crate::edge::proxy(State(state), req).await;
    }
    let run_id = run_raw.parse::<Uuid>().expect("checked segment");
    let actor = match request_actor(&state, extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(response) => return response,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let guarded = match fetch_guarded_run(pool, run_id).await {
        Ok(Some(guarded)) => guarded,
        Ok(None) => return not_found(),
        Err(response) => return response,
    };
    let facts = match view_facts_for(
        pool,
        actor.id,
        &guarded.run,
        guarded.runner_owner_id,
        guarded.runner_visibility,
        guarded.work_item_created_by_id,
    )
    .await
    {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    if !guards::can_cancel_run(&facts) {
        return not_found();
    }
    if guarded.run.executor_kind == AgentExecutorKind::CloudAgent {
        return json_response(StatusCode::CONFLICT, EXECUTOR_NOT_LOCAL_BODY.to_owned());
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let row: Option<sqlx::postgres::PgRow> = match sqlx::query(&lock_run_sql())
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        let _ = tx.rollback().await;
        return not_found();
    };
    let locked = match decode_run(&row, 0) {
        Ok(run) => run,
        Err(error) => {
            let _ = tx.rollback().await;
            return error.into();
        }
    };
    if locked.status != AgentRunStatus::Queued {
        let _ = tx.rollback().await;
        return json_response(StatusCode::CONFLICT, RUN_NOT_QUEUED_BODY.to_owned());
    }
    if locked.pinned_runner_id.is_none() {
        let _ = tx.rollback().await;
        return json_response(StatusCode::CONFLICT, RUN_NOT_PINNED_BODY.to_owned());
    }
    // Queued + pinned: Django owns the unpin + parent-thread clear +
    // pod drain. Roll back and proxy before any write.
    let _ = tx.rollback().await;
    crate::edge::proxy(State(state), req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fx08() -> Value {
        let raw =
            include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
        serde_json::from_str(raw).expect("valid fixture")
    }

    #[test]
    fn truthiness_matches_python() {
        for falsy in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!py_truthy_json(Some(&falsy)), "{falsy} is falsy");
        }
        assert!(!py_truthy_json(None));
        for truthy in [
            json!(true),
            json!(1),
            json!(-1),
            json!(0.5),
            json!("x"),
            json!(" "),
            json!([0]),
            json!({"a": 0}),
        ] {
            assert!(py_truthy_json(Some(&truthy)), "{truthy} is truthy");
        }
    }

    #[test]
    fn strip_matches_python() {
        assert_eq!(py_strip("  run_ai  "), "run_ai");
        assert_eq!(py_strip("\t\nrun_ai\r\n"), "run_ai");
        // U+001C–U+001F: CPython strips, Unicode White_Space does not.
        assert_eq!(py_strip("\u{1c}\u{1d}run_ai\u{1e}\u{1f}"), "run_ai");
        assert_eq!(py_strip("\u{85}\u{a0}run_ai\u{2003}"), "run_ai");
        assert_eq!(py_strip(""), "");
        assert_eq!(py_strip("   "), "");
    }

    #[test]
    fn truncate_counts_code_points() {
        assert_eq!(truncate_chars("abcdef", 512), "abcdef");
        assert_eq!(truncate_chars("abcdef", 3), "abc");
        // Multi-byte chars never split: 600 e-acutes → 512 whole chars.
        let wide = "é".repeat(600);
        let cut = truncate_chars(&wide, 512);
        assert_eq!(cut.chars().count(), 512);
        assert!(cut.is_char_boundary(cut.len()));
    }

    #[test]
    fn triggered_by_branches() {
        let body = |value: Value| json!({"triggered_by": value});
        assert_eq!(classify_triggered_by(&json!({})), TriggeredBy::Direct);
        assert_eq!(
            classify_triggered_by(&body(json!(null))),
            TriggeredBy::Direct
        );
        assert_eq!(
            classify_triggered_by(&body(json!("comment_and_run"))),
            TriggeredBy::CommentAndRun
        );
        assert_eq!(
            classify_triggered_by(&body(json!("  run_ai\t"))),
            TriggeredBy::RunAi
        );
        assert_eq!(
            classify_triggered_by(&body(json!("direct"))),
            TriggeredBy::Direct
        );
        assert_eq!(classify_triggered_by(&body(json!(""))), TriggeredBy::Direct);
        // Truthy non-strings raise `.strip()` in Python → proxy.
        for invalid in [json!(5), json!(true), json!([1]), json!({"a": 1})] {
            assert_eq!(
                classify_triggered_by(&body(invalid)),
                TriggeredBy::ProxyInvalid
            );
        }
        // Falsy non-strings fall through to the direct branch.
        for falsy in [json!(false), json!(0), json!([]), json!({})] {
            assert_eq!(classify_triggered_by(&body(falsy)), TriggeredBy::Direct);
        }
    }

    #[test]
    fn cancel_reason_arms() {
        assert_eq!(classify_cancel_reason(&json!({})), CancelReason::Default);
        assert_eq!(
            classify_cancel_reason(&json!({"reason": null})),
            CancelReason::Default
        );
        assert_eq!(
            classify_cancel_reason(&json!({"reason": ""})),
            CancelReason::Default
        );
        assert_eq!(
            classify_cancel_reason(&json!({"reason": "stop now"})),
            CancelReason::Text("stop now".to_owned())
        );
        let long = "R".repeat(600);
        match classify_cancel_reason(&json!({"reason": long})) {
            CancelReason::Text(reason) => assert_eq!(reason.len(), 512),
            other => panic!("expected truncation, got {other:?}"),
        }
        // Truthy non-strings raise `[:512]` in Python → proxy.
        assert_eq!(
            classify_cancel_reason(&json!({"reason": 5})),
            CancelReason::ProxyInvalid
        );
    }

    #[test]
    fn py_uuid_matches_cpython_vectors() {
        // Vectors verified against CPython 3.12 `uuid.UUID`.
        let canonical = "12345678-1234-5678-1234-567812345678";
        let parsed = parse_py_uuid(canonical).expect("canonical");
        assert_eq!(parsed.to_string(), canonical);
        assert_eq!(
            parse_py_uuid("12345678123456781234567812345678").expect("bare"),
            parsed
        );
        assert_eq!(
            parse_py_uuid("{12345678-1234-5678-1234-567812345678}").expect("braces"),
            parsed
        );
        assert_eq!(
            parse_py_uuid("urn:uuid:12345678-1234-5678-1234-567812345678").expect("urn"),
            parsed
        );
        // Hyphens anywhere are stripped before the length check.
        assert_eq!(
            parse_py_uuid("12-34-56-78-12-34-56-78-12-34-56-78-12-34-56-78").expect("scattered"),
            parsed
        );
        // Uppercase hex parses; uppercase URN prefix does not.
        assert!(parse_py_uuid(
            "12345678-1234-5678-1234-567812345678"
                .to_uppercase()
                .as_str()
        )
        .is_some());
        assert!(parse_py_uuid("URN:UUID:12345678123456781234567812345678").is_none());
        // One leading `+` and surrounding whitespace are tolerated
        // (zero-padded, like `int(x, 16)`); `-` fails the range check.
        assert_eq!(
            parse_py_uuid("+1234567812345678123456781234567").expect("plus"),
            Uuid::parse_str("01234567-8123-4567-8123-456781234567").expect("want")
        );
        assert_eq!(
            parse_py_uuid("  123456781234567812345678123456").expect("spaces"),
            Uuid::parse_str("00123456-7812-3456-7812-345678123456").expect("want")
        );
        assert!(parse_py_uuid("-1234567812345678123456781234567").is_none());
        for bad in [
            "",
            "nope",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "gggggggggggggggggggggggggggggggg",
            " 12345678123456781234567812345678 ",
            "00000000-0000-0000-0000-00000000000",
        ] {
            assert!(parse_py_uuid(bad).is_none(), "{bad:?} is malformed");
        }
        assert!(parse_py_uuid("00000000-0000-0000-0000-000000000000").is_some());
    }

    #[test]
    fn retick_work_item_arms() {
        let id = Uuid::parse_str("8dbd5acc-b3ce-4259-91b2-570768a0b918").expect("id");
        assert_eq!(classify_retick_work_item(&json!({})), WorkItemRef::Missing);
        assert_eq!(
            classify_retick_work_item(&json!({"work_item": null})),
            WorkItemRef::Missing
        );
        assert_eq!(
            classify_retick_work_item(&json!({"work_item": id.to_string()})),
            WorkItemRef::Id(id)
        );
        assert_eq!(
            classify_retick_work_item(&json!({"work_item": "nope"})),
            WorkItemRef::Malformed
        );
        // A 32-digit integer's decimal text is itself 32 hex digits.
        assert!(matches!(
            classify_retick_work_item(&json!({"work_item": 12345678123456781234567812345678u128})),
            WorkItemRef::Id(_)
        ));
        // Short integers, bools, floats and containers can never parse.
        for bad in [
            json!({"work_item": 5}),
            json!({"work_item": true}),
            json!({"work_item": 1.5}),
            json!({"work_item": []}),
            json!({"work_item": {}}),
        ] {
            // `{}` is falsy → Missing; the rest are Malformed.
            let got = classify_retick_work_item(&bad);
            assert!(
                matches!(got, WorkItemRef::Malformed | WorkItemRef::Missing),
                "{bad} → {got:?}"
            );
        }
    }

    #[test]
    fn total_pages_matches_fixture_vectors() {
        // (total_count, per_page, want): FX-RUN-08 `list` vectors.
        for (total, per_page, want) in [
            (3, 30, 1),
            (3, 1, 3),
            (3, 200, 1),
            (0, 30, 1),
            (30, 30, 1),
            (31, 30, 2),
            (200, 30, 7),
        ] {
            assert_eq!(total_pages(total, per_page), want, "{total}/{per_page}");
        }
    }

    #[test]
    fn list_where_numbers_params() {
        let bare = list_where(false, false);
        for param in ["$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8"] {
            assert!(bare.contains(param), "bare has {param}");
        }
        assert!(!bare.contains("$9"));
        assert!(bare.contains(r#"U0."role" = 20"#));
        assert!(bare.contains("issue_assignees\".\"deleted_at\" IS NULL"));
        let scoped = list_where(true, true);
        assert!(scoped.contains(r#""agent_run"."workspace_id" = $9::uuid"#));
        assert!(scoped.contains(r#""pod"."project_id" = $10::uuid"#));
        let page = list_page_sql(true, true);
        assert!(page.contains("LIMIT $11 OFFSET $12"));
        assert!(page.contains("SELECT DISTINCT"));
        let page_bare = list_page_sql(false, false);
        assert!(page_bare.contains("LIMIT $9 OFFSET $10"));
        let count = list_count_sql(false, false);
        assert!(count.starts_with(r#"SELECT COUNT(DISTINCT "agent_run"."id")"#));
    }

    #[test]
    fn run_select_list_covers_all_columns_once() {
        let list = run_select_list();
        assert_eq!(list.split(", ").count(), agent_run::COLUMNS.len());
        assert_eq!(agent_run::COLUMNS.len(), 41);
        assert!(list.starts_with(r#""agent_run"."id""#));
        assert!(list.ends_with(r#""agent_run"."ended_at""#));
    }

    #[test]
    fn cancel_finalize_columns_match_l4_plan() {
        let values = finalization::plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("cancel_requested_at", SetValue::Now),
                ("cancel_reason", SetValue::Text("r".to_owned())),
                ("error_code", SetValue::Text("cancelled".to_owned())),
                ("error", SetValue::Text("r".to_owned())),
            ],
        )
        .expect("terminal");
        let columns: Vec<&str> = values.clauses.iter().map(|clause| clause.column).collect();
        assert_eq!(columns.as_slice(), CANCEL_FINALIZE_COLUMNS);
        let sql = finalization::finalize_update_sql(&values);
        assert!(sql.ends_with(r#"WHERE "agent_run"."id" = $10"#), "{sql}");
    }

    #[test]
    fn error_bodies_match_fx08_goldens() {
        let fx = fx08();
        let body = |pointer: &str| fx.pointer(pointer).expect(pointer).to_string();
        // Compact JSON has no spaces; the consts are written compact.
        assert_eq!(PROMPT_REQUIRED_BODY, body("/create/missing_prompt/body"));
        assert_eq!(RUN_AI_WORK_ITEM_REQUIRED_BODY, body("/run_ai/missing/body"));
        assert_eq!(
            COMMENT_RUN_WORK_ITEM_REQUIRED_BODY,
            body("/comment_and_run/missing/body")
        );
        assert_eq!(RETICK_WORK_ITEM_REQUIRED_BODY, body("/retick/missing/body"));
        assert_eq!(RETICK_MALFORMED_BODY, body("/retick/malformed/body"));
        assert_eq!(ISSUE_NOT_FOUND_BODY, body("/run_ai/not_found/body"));
        assert_eq!(ISSUE_NOT_FOUND_BODY, body("/retick/not_found/body"));
        assert_eq!(RUN_ALREADY_TERMINAL_BODY, body("/cancel/terminal/body"));
        assert_eq!(EXECUTOR_NOT_LOCAL_BODY, body("/release_pin/cloud/body"));
        assert_eq!(RUN_NOT_QUEUED_BODY, body("/release_pin/not_queued/body"));
        assert_eq!(RUN_NOT_PINNED_BODY, body("/release_pin/not_pinned/body"));
        // L3's denial covers the detail/cancel/release-pin 404.
        let denial = guards::run_not_found();
        assert_eq!(denial.status, 404);
        assert_eq!(denial.body.to_string(), body("/detail/missing/body"));
        // Envelope key order is pinned by construction order.
        let envelope: Vec<String> = fx["list"]["default"]["envelope_keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| key.as_str().expect("str").to_owned())
            .collect();
        assert_eq!(
            envelope,
            [
                "results",
                "count",
                "total_count",
                "total_pages",
                "page",
                "per_page"
            ]
        );
    }

    // -- live scratch-DB tests (env-gated) -------------------------------
    // Same convention as pidash-db's orchestration runs: unset
    // DATABASE_URL (plain `cargo test`) skips these.

    async fn scratch_pool() -> Option<PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// Temp `agent_run` in [`agent_run::COLUMNS`] order (the 41-column
    /// positional image [`decode_run`] reads).
    const LIVE_DDL: &str = "CREATE TEMPORARY TABLE agent_run (
        id UUID PRIMARY KEY, workspace_id UUID, owner_id UUID,
        created_by_id UUID, pod_id UUID, runner_id UUID,
        pinned_runner_id UUID, work_item_id UUID,
        scheduler_binding_id UUID, parent_run_id UUID, status TEXT,
        executor_kind TEXT, dispatch_attempts INTEGER,
        cancel_requested_at TIMESTAMPTZ, cancel_reason TEXT,
        error_code TEXT, tool_plan JSONB,
        terminal_hooks_applied_at TIMESTAMPTZ,
        terminal_capacity_released_at TIMESTAMPTZ, prompt TEXT,
        trigger TEXT, prompt_manifest JSONB, phase_kind TEXT,
        run_config JSONB, required_capabilities JSONB, thread_id TEXT,
        agent_metadata JSONB, lease_expires_at TIMESTAMPTZ,
        done_payload JSONB, error TEXT, refusal_category TEXT,
        llm_model TEXT, usage JSONB, input_tokens BIGINT,
        output_tokens BIGINT, total_tokens BIGINT, created_at TIMESTAMPTZ,
        assigned_at TIMESTAMPTZ, queue_position SMALLINT,
        started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)";

    /// Unknown stored trigger values decode verbatim instead of
    /// 500ing the list / detail / cancel / release-pin reads:
    /// Django's `TextChoices` are choices-only (no DB check), so
    /// legacy rows (runner migration 0029 kept `blocker_completed`
    /// values) and hand-written rows (the contract harness seeds
    /// `human`) must render like any member.
    #[tokio::test]
    async fn live_unknown_trigger_values_decode_raw() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(LIVE_DDL)
            .execute(&mut *tx)
            .await
            .expect("create temp agent_run");
        let select = format!(
            "SELECT {} FROM agent_run WHERE id = $1",
            agent_run::COLUMNS.join(", ")
        );
        for trigger in ["human", "blocker_completed", "direct"] {
            let id = Uuid::new_v4();
            let ws = Uuid::new_v4();
            let user = Uuid::new_v4();
            let pod = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id,
                 status, executor_kind, dispatch_attempts, cancel_reason,
                 error_code, tool_plan, prompt, trigger, phase_kind,
                 run_config, required_capabilities, thread_id, agent_metadata,
                 error, refusal_category, llm_model, usage, created_at)
                 VALUES ($1, $2, $3, $4, 'running', 'local_runner', 0, '', '',
                 '{}', '', $5, 'work', '{}', '{}', 't', '{}', '', '', '', '{}',
                 now())",
            )
            .bind(id)
            .bind(ws)
            .bind(user)
            .bind(pod)
            .bind(trigger)
            .execute(&mut *tx)
            .await
            .expect("seed run");
            let row: sqlx::postgres::PgRow = sqlx::query(&select)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .expect("fetch run image");
            let run = decode_run(&row, 0).expect("unknown trigger decodes");
            assert_eq!(run.id, id);
            assert_eq!(run.trigger, trigger);
        }
    }
}
