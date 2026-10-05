//! Run / approval / chat serializer shapes for D-15 runner_runs (L3).
//!
//! Ports the output shapes in `apps/api/pi_dash/runner/serializers.py`:
//!
//! * `AgentRunSerializer` (`:260-312`, incl the `error_diagnostic`
//!   list-vs-detail rule at `:266-272` and the inline `tool_calls`)
//! * `AgentRunToolCallSerializer` (`:241-257`), nested under runs
//! * `AgentRunEventSerializer` (`:313-318`)
//! * `ApprovalRequestSerializer` (`:320-336`)
//! * `ApprovalDecisionSerializer` (`:338-340`, accept/decline validation)
//! * `AgentChatSessionSerializer` (`:342-369`)
//! * `AgentChatMessageSerializer` (`:372-388`)
//! * `AgentChatEventSerializer` (`:391-403`)
//! * `AgentChatApprovalRequestSerializer` (`:406-422`)
//!
//! # Reuse, not forks
//!
//! * Row inputs are the L2 structs from [`pidash_db::runner_runs`]
//!   (PIDASHCONV-528); choice columns render through the L1 `value()`
//!   methods from [`pidash_types::runner_runs`] (PIDASHCONV-527).
//! * `error_diagnostic` delegates to the L1
//!   [`pidash_types::runner_runs::error_diagnostic`] kernel, which owns
//!   the list-renders-`None` contract.
//! * `PodSerializer` (`:36-74`) and `PodMiniSerializer` (`:133-144`) are
//!   owned by D-13's shape module
//!   ([`crate::runner_enroll::serializers::shapes`], PIDASHCONV-579):
//!   `pod_detail` reuses its `PodMini` row/view kernel, and the chat
//!   session's `runner_detail` reuses its `Runner` kernel, so the nested
//!   bytes are identical by construction. No D-15-local copies exist.
//!
//! # Rendering rules
//!
//! Views follow the D-13 `to_representation` pattern: one `Serialize`
//! struct per serializer with fields in `Meta.fields` order (JSON key
//! order), borrowing text/JSON straight from the L2 row and owning only
//! what must be transformed (UUIDs and datetimes render to `String`).
//! UUID and FK primary keys render as strings; a null FK renders `null`.
//! JSON blobs pass through by reference. The token counters render the
//! real generated columns (`input_tokens` et al.), like DRF does.
//!
//! Datetimes render exactly like DRF's `DateTimeField` (`iso-8601`):
//! `isoformat` with `+00:00` rewritten to `Z`, microseconds only when
//! nonzero ([`drf_datetime`]). There are no `Decimal` or float columns
//! in these serializers, so the float traps do not apply here.
//!
//! Fixture source of truth: the shape goldens inside
//! `fx-run-08-handlers-web.golden.json` (`detail.ok`, `list.golden_row`,
//! `decide.bad_choice` / `missing_field`, `approvals_list.ok` keys) and
//! `fx-run-09-handlers-daemon.golden.json` (`web_chat.msg_post_ok.msg`,
//! `cappr_decide_ok.approval`, `sess_list_owner` / `msg_list` keys, SSE
//! event frames). Each `#[cfg(test)]` suite replays its section.
//!
//! Ported bugs: none found in these serializers on read-through beyond
//! the D-13 `QUIRK-runner-count`, which is inherited through reuse (the
//! count is caller-supplied; this module never computes it).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use crate::runner_enroll::serializers::shapes::{
    pod_mini_to_representation, runner_to_representation, PodMiniRow, PodMiniView, RunnerRow,
    RunnerView,
};
use pidash_db::runner_runs::{
    AgentChatApprovalRequest, AgentChatEvent, AgentChatMessage, AgentChatSession, AgentRun,
    AgentRunEvent, AgentRunToolCall, ApprovalRequest,
};
use pidash_types::runner_runs::{error_diagnostic, RunErrorDiagnostic};

// ---------------------------------------------------------------------------
// Wire field order (`Meta.fields` order == JSON key order)
// ---------------------------------------------------------------------------

/// `AgentRunSerializer.Meta.fields` (`serializers.py:276-309`).
pub const RUN_WIRE_FIELDS: [&str; 32] = [
    "id",
    "status",
    "executor_kind",
    "dispatch_attempts",
    "cancel_requested_at",
    "cancel_reason",
    "prompt",
    "thread_id",
    "agent_metadata",
    "runner",
    "work_item",
    "pod",
    "pod_detail",
    "created_by",
    "owner",
    "created_at",
    "assigned_at",
    "queue_position",
    "started_at",
    "ended_at",
    "done_payload",
    "error",
    "error_code",
    "error_diagnostic",
    "refusal_category",
    "llm_model",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "usage",
    "tool_plan",
    "tool_calls",
];

/// `AgentRunToolCallSerializer.Meta.fields` (`serializers.py:244-256`).
pub const TOOL_CALL_WIRE_FIELDS: [&str; 11] = [
    "id",
    "tool_call_id",
    "source",
    "server_key",
    "tool_name",
    "risk",
    "status",
    "error_code",
    "prepared_at",
    "submitted_at",
    "completed_at",
];

/// `AgentRunEventSerializer.Meta.fields` (`serializers.py:316`).
pub const RUN_EVENT_WIRE_FIELDS: [&str; 5] = ["id", "seq", "kind", "payload", "created_at"];

/// `ApprovalRequestSerializer.Meta.fields` (`serializers.py:323-334`).
pub const APPROVAL_WIRE_FIELDS: [&str; 10] = [
    "id",
    "agent_run",
    "kind",
    "payload",
    "reason",
    "status",
    "decision_source",
    "requested_at",
    "decided_at",
    "expires_at",
];

/// `AgentChatSessionSerializer.Meta.fields` (`serializers.py:347-368`).
pub const CHAT_SESSION_WIRE_FIELDS: [&str; 20] = [
    "id",
    "workspace",
    "runner",
    "runner_detail",
    "created_by",
    "pod",
    "status",
    "agent_kind",
    "local_thread_id",
    "local_session_id",
    "cwd",
    "model",
    "active_turn_id",
    "active_message_id",
    "close_requested",
    "last_message_at",
    "closed_at",
    "error",
    "created_at",
    "updated_at",
];

/// `AgentChatMessageSerializer.Meta.fields` (`serializers.py:375-387`).
pub const CHAT_MESSAGE_WIRE_FIELDS: [&str; 11] = [
    "id",
    "session",
    "role",
    "content",
    "content_parts",
    "status",
    "local_item_id",
    "local_turn_id",
    "seq",
    "created_at",
    "completed_at",
];

/// `AgentChatEventSerializer.Meta.fields` (`serializers.py:394-402`).
pub const CHAT_EVENT_WIRE_FIELDS: [&str; 7] = [
    "id",
    "session",
    "message",
    "seq",
    "kind",
    "payload",
    "created_at",
];

/// `AgentChatApprovalRequestSerializer.Meta.fields`
/// (`serializers.py:409-421`).
pub const CHAT_APPROVAL_WIRE_FIELDS: [&str; 12] = [
    "id",
    "session",
    "local_approval_id",
    "kind",
    "payload",
    "reason",
    "status",
    "decision_source",
    "decided_by",
    "requested_at",
    "expires_at",
    "decided_at",
];

// ---------------------------------------------------------------------------
// DRF scalar rendering
// ---------------------------------------------------------------------------

/// Render a stored instant exactly like DRF's `DateTimeField` with the
/// default `iso-8601` format on a UTC-aware value: `isoformat` with the
/// `+00:00` suffix rewritten to `Z`, microseconds only when nonzero.
/// (`USE_TZ`, `TIME_ZONE "UTC"`: every value here is UTC-aware, so no
/// zone shift applies, unlike the request-zone serializer paths.)
pub fn drf_datetime(dt: &DateTime<Utc>) -> String {
    let mut out = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos != 0 {
        out.push_str(&format!(".{:06}", nanos / 1000));
    }
    out.push('Z');
    out
}

fn drf_datetime_opt(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(drf_datetime)
}

fn uuid_string(id: &uuid::Uuid) -> String {
    id.to_string()
}

fn uuid_opt_string(id: &Option<uuid::Uuid>) -> Option<String> {
    id.as_ref().map(uuid_string)
}

// ---------------------------------------------------------------------------
// Run shapes
// ---------------------------------------------------------------------------

/// `AgentRunToolCallSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolCallView<'a> {
    pub id: String,
    pub tool_call_id: &'a str,
    pub source: &'a str,
    pub server_key: &'a str,
    pub tool_name: &'a str,
    pub risk: &'a str,
    pub status: &'a str,
    pub error_code: &'a str,
    pub prepared_at: String,
    pub submitted_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Port of `AgentRunToolCallSerializer` (`serializers.py:241-257`).
pub fn tool_call_to_representation(row: &AgentRunToolCall) -> ToolCallView<'_> {
    ToolCallView {
        id: uuid_string(&row.id),
        tool_call_id: &row.tool_call_id,
        source: &row.source,
        server_key: &row.server_key,
        tool_name: &row.tool_name,
        risk: &row.risk,
        status: row.status.value(),
        error_code: &row.error_code,
        prepared_at: drf_datetime(&row.prepared_at),
        submitted_at: drf_datetime_opt(&row.submitted_at),
        completed_at: drf_datetime_opt(&row.completed_at),
    }
}

/// `AgentRunSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentRunView<'a> {
    pub id: String,
    pub status: &'a str,
    pub executor_kind: &'a str,
    pub dispatch_attempts: i32,
    pub cancel_requested_at: Option<String>,
    pub cancel_reason: &'a str,
    pub prompt: &'a str,
    pub thread_id: &'a str,
    pub agent_metadata: &'a Value,
    pub runner: Option<String>,
    pub work_item: Option<String>,
    pub pod: String,
    pub pod_detail: PodMiniView<'a>,
    pub created_by: String,
    pub owner: Option<String>,
    pub created_at: String,
    pub assigned_at: Option<String>,
    pub queue_position: Option<i16>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub done_payload: Option<&'a Value>,
    pub error: &'a str,
    pub error_code: &'a str,
    pub error_diagnostic: Option<RunErrorDiagnostic>,
    pub refusal_category: &'a str,
    pub llm_model: &'a str,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub usage: &'a Value,
    pub tool_plan: &'a Value,
    pub tool_calls: Vec<ToolCallView<'a>>,
}

/// Port of `AgentRunSerializer` for a list row (`serializers.py:260-312`
/// with `many=True`): `error_diagnostic` renders `None` — the list skips
/// the classifier's per-row scans (`serializers.py:270-271`).
///
/// `pod` is the caller's D-13 mini row for the run's pod (the FK is
/// non-nullable); `tool_calls` the run's tool-call rows in render order.
pub fn run_list_to_representation<'a>(
    run: &'a AgentRun,
    pod: &'a PodMiniRow<'a>,
    tool_calls: &'a [AgentRunToolCall],
) -> AgentRunView<'a> {
    run_to_representation(run, pod, tool_calls, true)
}

/// Port of `AgentRunSerializer` for the per-run detail
/// (`serializers.py:260-312`): `error_diagnostic` runs
/// `classify_run_error(run.error)` (`serializers.py:272`).
pub fn run_detail_to_representation<'a>(
    run: &'a AgentRun,
    pod: &'a PodMiniRow<'a>,
    tool_calls: &'a [AgentRunToolCall],
) -> AgentRunView<'a> {
    run_to_representation(run, pod, tool_calls, false)
}

fn run_to_representation<'a>(
    run: &'a AgentRun,
    pod: &'a PodMiniRow<'a>,
    tool_calls: &'a [AgentRunToolCall],
    is_list: bool,
) -> AgentRunView<'a> {
    AgentRunView {
        id: uuid_string(&run.id),
        status: run.status.value(),
        executor_kind: run.executor_kind.value(),
        dispatch_attempts: run.dispatch_attempts,
        cancel_requested_at: drf_datetime_opt(&run.cancel_requested_at),
        cancel_reason: &run.cancel_reason,
        prompt: &run.prompt,
        thread_id: &run.thread_id,
        agent_metadata: &run.agent_metadata,
        runner: uuid_opt_string(&run.runner_id),
        work_item: uuid_opt_string(&run.work_item_id),
        pod: uuid_string(&run.pod_id),
        pod_detail: pod_mini_to_representation(pod),
        created_by: uuid_string(&run.created_by_id),
        owner: uuid_opt_string(&run.owner_id),
        created_at: drf_datetime(&run.created_at),
        assigned_at: drf_datetime_opt(&run.assigned_at),
        queue_position: run.queue_position,
        started_at: drf_datetime_opt(&run.started_at),
        ended_at: drf_datetime_opt(&run.ended_at),
        done_payload: run.done_payload.as_ref(),
        error: &run.error,
        error_code: &run.error_code,
        error_diagnostic: error_diagnostic(Some(run.error.as_str()), is_list),
        refusal_category: &run.refusal_category,
        llm_model: &run.llm_model,
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        usage: &run.usage,
        tool_plan: &run.tool_plan,
        tool_calls: tool_calls.iter().map(tool_call_to_representation).collect(),
    }
}

/// `AgentRunEventSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentRunEventView<'a> {
    pub id: i64,
    pub seq: i32,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub created_at: String,
}

/// Port of `AgentRunEventSerializer` (`serializers.py:313-318`).
pub fn run_event_to_representation(row: &AgentRunEvent) -> AgentRunEventView<'_> {
    AgentRunEventView {
        id: row.id,
        seq: row.seq,
        kind: &row.kind,
        payload: &row.payload,
        created_at: drf_datetime(&row.created_at),
    }
}

// ---------------------------------------------------------------------------
// Approval shapes
// ---------------------------------------------------------------------------

/// `ApprovalRequestSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApprovalView<'a> {
    pub id: String,
    pub agent_run: String,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub reason: &'a str,
    pub status: &'a str,
    pub decision_source: &'a str,
    pub requested_at: String,
    pub decided_at: Option<String>,
    pub expires_at: Option<String>,
}

/// Port of `ApprovalRequestSerializer` (`serializers.py:320-336`).
pub fn approval_to_representation(row: &ApprovalRequest) -> ApprovalView<'_> {
    ApprovalView {
        id: uuid_string(&row.id),
        agent_run: uuid_string(&row.agent_run_id),
        kind: row.kind.value(),
        payload: &row.payload,
        reason: &row.reason,
        status: row.status.value(),
        decision_source: &row.decision_source,
        requested_at: drf_datetime(&row.requested_at),
        decided_at: drf_datetime_opt(&row.decided_at),
        expires_at: drf_datetime_opt(&row.expires_at),
    }
}

// ---------------------------------------------------------------------------
// Chat shapes
// ---------------------------------------------------------------------------

/// `AgentChatSessionSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatSessionView<'a> {
    pub id: String,
    pub workspace: String,
    pub runner: String,
    pub runner_detail: RunnerView<'a>,
    pub created_by: String,
    pub pod: String,
    pub status: &'a str,
    pub agent_kind: &'a str,
    pub local_thread_id: &'a str,
    pub local_session_id: &'a str,
    pub cwd: &'a str,
    pub model: &'a str,
    pub active_turn_id: &'a str,
    pub active_message_id: Option<String>,
    pub close_requested: bool,
    pub last_message_at: Option<String>,
    pub closed_at: Option<String>,
    pub error: &'a str,
    pub created_at: String,
    pub updated_at: String,
}

/// Port of `AgentChatSessionSerializer` (`serializers.py:342-369`).
/// `runner_detail` embeds D-13's `RunnerSerializer` (`source="runner"`,
/// non-null FK) via the shared kernel.
pub fn chat_session_to_representation<'a>(
    session: &'a AgentChatSession,
    runner: &'a RunnerRow<'a>,
) -> ChatSessionView<'a> {
    ChatSessionView {
        id: uuid_string(&session.id),
        workspace: uuid_string(&session.workspace_id),
        runner: uuid_string(&session.runner_id),
        runner_detail: runner_to_representation(runner),
        created_by: uuid_string(&session.created_by_id),
        pod: uuid_string(&session.pod_id),
        status: session.status.value(),
        agent_kind: &session.agent_kind,
        local_thread_id: &session.local_thread_id,
        local_session_id: &session.local_session_id,
        cwd: &session.cwd,
        model: &session.model,
        active_turn_id: &session.active_turn_id,
        active_message_id: uuid_opt_string(&session.active_message_id),
        close_requested: session.close_requested,
        last_message_at: drf_datetime_opt(&session.last_message_at),
        closed_at: drf_datetime_opt(&session.closed_at),
        error: &session.error,
        created_at: drf_datetime(&session.created_at),
        updated_at: drf_datetime(&session.updated_at),
    }
}

/// `AgentChatMessageSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMessageView<'a> {
    pub id: String,
    pub session: String,
    pub role: &'a str,
    pub content: &'a str,
    pub content_parts: &'a Value,
    pub status: &'a str,
    pub local_item_id: &'a str,
    pub local_turn_id: &'a str,
    pub seq: i32,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Port of `AgentChatMessageSerializer` (`serializers.py:372-388`).
pub fn chat_message_to_representation(row: &AgentChatMessage) -> ChatMessageView<'_> {
    ChatMessageView {
        id: uuid_string(&row.id),
        session: uuid_string(&row.session_id),
        role: row.role.value(),
        content: &row.content,
        content_parts: &row.content_parts,
        status: row.status.value(),
        local_item_id: &row.local_item_id,
        local_turn_id: &row.local_turn_id,
        seq: row.seq,
        created_at: drf_datetime(&row.created_at),
        completed_at: drf_datetime_opt(&row.completed_at),
    }
}

/// `AgentChatEventSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatEventView<'a> {
    pub id: i64,
    pub session: String,
    pub message: Option<String>,
    pub seq: i32,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub created_at: String,
}

/// Port of `AgentChatEventSerializer` (`serializers.py:391-403`).
pub fn chat_event_to_representation(row: &AgentChatEvent) -> ChatEventView<'_> {
    ChatEventView {
        id: row.id,
        session: uuid_string(&row.session_id),
        message: uuid_opt_string(&row.message_id),
        seq: row.seq,
        kind: &row.kind,
        payload: &row.payload,
        created_at: drf_datetime(&row.created_at),
    }
}

/// `AgentChatApprovalRequestSerializer.to_representation` output, in wire
/// order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatApprovalView<'a> {
    pub id: String,
    pub session: String,
    pub local_approval_id: &'a str,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub reason: &'a str,
    pub status: &'a str,
    pub decision_source: &'a str,
    pub decided_by: Option<String>,
    pub requested_at: String,
    pub expires_at: Option<String>,
    pub decided_at: Option<String>,
}

/// A validated approval decision (`ApprovalDecisionSerializer`,
/// `serializers.py:338-340`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Accept,
    Decline,
}

impl ApprovalDecision {
    /// The wire value (`accept` / `decline`).
    pub fn value(&self) -> &'static str {
        match self {
            ApprovalDecision::Accept => "accept",
            ApprovalDecision::Decline => "decline",
        }
    }
}

/// Port of `ApprovalDecisionSerializer(data=body)` validation
/// (`serializers.py:338-340`): `decision` is a required
/// `ChoiceField(["accept", "decline"])`.
///
/// `body` is the already-parsed JSON request body (form bodies never
/// reach this kernel — content negotiation is the handler's job).
/// Failures return the exact DRF 3.15 error body as a `Value`, verified
/// against live DRF: missing → `required`, explicit `null` → `null`,
/// anything but the two choices → `invalid_choice` with the input
/// rendered by Python `str()`, non-object bodies → `invalid` (and a
/// missing body → `No data provided`).
pub fn validate_approval_decision(body: &Value) -> Result<ApprovalDecision, Value> {
    if body.is_null() {
        return Err(json!({"non_field_errors": ["No data provided"]}));
    }
    let Some(obj) = body.as_object() else {
        return Err(json!({"non_field_errors": [format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_datatype(body)
        )]}));
    };
    let Some(raw) = obj.get("decision") else {
        return Err(json!({"decision": ["This field is required."]}));
    };
    if raw.is_null() {
        return Err(json!({"decision": ["This field may not be null."]}));
    }
    if let Some(choice) = raw.as_str() {
        match choice {
            "accept" => return Ok(ApprovalDecision::Accept),
            "decline" => return Ok(ApprovalDecision::Decline),
            _ => {}
        }
    }
    Err(json!({"decision": [format!(
        "\"{}\" is not a valid choice.",
        py_str(raw)
    )]}))
}

/// DRF's `{datatype}` for the serializer-level `invalid` error: the
/// Python type name of the JSON value.
fn json_datatype(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python `str()` of a JSON-decoded value, for the `invalid_choice`
/// `input` slot: strings render bare, every other type renders its
/// `repr` (DRF formats the original object, `'"{}"'.format(input)`).
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => py_repr(value),
    }
}

/// Python `repr()` of a JSON-decoded value: `None` / `True` / `False`,
/// numbers in `str()` form, strings single-quoted (double-quoted when
/// they contain `'` but not `"`), containers with `", "` separators.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                py_float_repr(number.as_f64().expect("f64"))
            }
        }
        Value::String(text) => py_str_repr(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_str_repr(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a string: short escapes for whitespace, `\xXX` /
/// `\uXXXX` / `\UXXXXXXXX` for control characters, everything else
/// literal.
///
/// Known divergence: non-ASCII non-printables outside the `Cc` class
/// (format `Cf` / separator `Zl`/`Zp` characters such as the zero-width
/// space) pass through literally here while CPython escapes them. Only
/// reachable with such characters inside a `decision` value, which no
/// client sends; documented rather than tabled.
fn py_str_repr(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(quote);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < ' ' || c == '\u{7f}' => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if c.is_control() => {
                    let code = c as u32;
                    if code < 0x100 {
                        out.push_str(&format!("\\x{:02x}", code));
                    } else if code < 0x10000 {
                        out.push_str(&format!("\\u{:04x}", code));
                    } else {
                        out.push_str(&format!("\\U{:08x}", code));
                    }
                }
                c => out.push(c),
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a float: shortest round-trip digits (taken from
/// serde's ryu rendering, which implements the same shortest spec as
/// CPython's `float_repr_style short`) laid out by Python's rules —
/// fixed notation for decimal exponents `-3..=16` with a mandatory `.0`
/// on integral values, else `d[.ddd]e±XX` with a signed, ≥2-digit
/// exponent.
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let negative = value.is_sign_negative();
    let abs = value.abs();
    if abs == 0.0 {
        return if negative { "-0.0" } else { "0.0" }.to_string();
    }
    // Shortest digits: `1.5`, `100.0`, `1e20`, `1.5e-05` shapes.
    let ryu = serde_json::Number::from_f64(abs)
        .expect("finite")
        .to_string();
    let (mantissa, exp): (&str, i32) = match ryu.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse().expect("ryu exponent")),
        None => (ryu.as_str(), 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits[0] == '0' {
        digits.remove(0);
    }
    let after_point = mantissa.len() - point - usize::from(point < mantissa.len());
    // Value = digits × 10^(exp - after_point) = 0.digits × 10^dec_exp.
    let dec_exp = exp - after_point as i32 + digits.len() as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..=16).contains(&dec_exp) {
        // Fixed notation.
        if dec_exp <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-dec_exp) as usize));
            out.extend(digits);
        } else if dec_exp as usize >= digits.len() {
            let pad = dec_exp as usize - digits.len();
            out.extend(digits);
            out.push_str(&"0".repeat(pad));
            out.push_str(".0");
        } else {
            let split = dec_exp as usize;
            out.extend(digits[..split].iter());
            out.push('.');
            out.extend(digits[split..].iter());
        }
    } else {
        // Exponential notation.
        out.push(digits[0]);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter());
        }
        let exp10 = dec_exp - 1;
        out.push('e');
        out.push(if exp10 < 0 { '-' } else { '+' });
        let mag = exp10.unsigned_abs().to_string();
        if mag.len() < 2 {
            out.push('0');
        }
        out.push_str(&mag);
    }
    out
}

/// Port of `AgentChatApprovalRequestSerializer`
/// (`serializers.py:406-422`).
pub fn chat_approval_to_representation(row: &AgentChatApprovalRequest) -> ChatApprovalView<'_> {
    ChatApprovalView {
        id: uuid_string(&row.id),
        session: uuid_string(&row.session_id),
        local_approval_id: &row.local_approval_id,
        kind: row.kind.value(),
        payload: &row.payload,
        reason: &row.reason,
        status: row.status.value(),
        decision_source: &row.decision_source,
        decided_by: uuid_opt_string(&row.decided_by_id),
        requested_at: drf_datetime(&row.requested_at),
        expires_at: drf_datetime_opt(&row.expires_at),
        decided_at: drf_datetime_opt(&row.decided_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_enroll::serializers::shapes::{
        DevMachineMiniRow, LiveStateRow, RunnerRow, RUNNER_WIRE_FIELDS,
    };
    use pidash_types::runner_runs::{
        AgentChatSessionStatus, ApprovalKind, ApprovalStatus, ToolCallStatus,
    };
    use serde_json::json;

    static FX08: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
    static FX09: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-09-handlers-daemon.golden.json");

    fn fx08() -> Value {
        serde_json::from_str(FX08).expect("FX-RUN-08 parses")
    }

    fn fx09() -> Value {
        serde_json::from_str(FX09).expect("FX-RUN-09 parses")
    }

    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    fn dt(text: &str) -> DateTime<Utc> {
        text.parse().expect("fixture datetime")
    }

    fn dt_opt(value: &Value) -> Option<DateTime<Utc>> {
        match value {
            Value::Null => None,
            Value::String(text) => Some(dt(text)),
            _ => panic!("datetime or null"),
        }
    }

    fn uuid(text: &str) -> uuid::Uuid {
        text.parse().expect("fixture uuid")
    }

    fn uuid_opt(value: &Value) -> Option<uuid::Uuid> {
        match value {
            Value::Null => None,
            Value::String(text) => Some(uuid(text)),
            _ => panic!("uuid or null"),
        }
    }

    fn text(value: &Value) -> String {
        value.as_str().expect("string").to_owned()
    }

    /// Build the L2 run row behind a fixture run body (columns the
    /// serializer reads verbatim; serializer-ignored columns take
    /// defaults).
    fn run_row(body: &Value) -> AgentRun {
        AgentRun {
            id: uuid(body["id"].as_str().expect("id")),
            workspace_id: uuid("00000000-0000-4000-8000-000000000000"),
            owner_id: uuid_opt(&body["owner"]),
            created_by_id: uuid(body["created_by"].as_str().expect("created_by")),
            pod_id: uuid(body["pod"].as_str().expect("pod")),
            runner_id: uuid_opt(&body["runner"]),
            pinned_runner_id: None,
            work_item_id: uuid_opt(&body["work_item"]),
            scheduler_binding_id: None,
            parent_run_id: None,
            status: serde_json::from_value(body["status"].clone()).expect("status"),
            executor_kind: serde_json::from_value(body["executor_kind"].clone()).expect("executor"),
            dispatch_attempts: body["dispatch_attempts"].as_i64().expect("n") as i32,
            cancel_requested_at: dt_opt(&body["cancel_requested_at"]),
            cancel_reason: text(&body["cancel_reason"]),
            error_code: text(&body["error_code"]),
            tool_plan: body["tool_plan"].clone(),
            terminal_hooks_applied_at: None,
            terminal_capacity_released_at: None,
            prompt: text(&body["prompt"]),
            trigger: "direct".to_owned(),
            prompt_manifest: None,
            phase_kind: String::new(),
            run_config: json!({}),
            required_capabilities: json!([]),
            thread_id: text(&body["thread_id"]),
            agent_metadata: body["agent_metadata"].clone(),
            lease_expires_at: None,
            done_payload: match &body["done_payload"] {
                Value::Null => None,
                payload => Some(payload.clone()),
            },
            error: text(&body["error"]),
            refusal_category: text(&body["refusal_category"]),
            llm_model: text(&body["llm_model"]),
            usage: body["usage"].clone(),
            input_tokens: body["input_tokens"].as_i64(),
            output_tokens: body["output_tokens"].as_i64(),
            total_tokens: body["total_tokens"].as_i64(),
            created_at: dt(body["created_at"].as_str().expect("created_at")),
            assigned_at: dt_opt(&body["assigned_at"]),
            queue_position: body["queue_position"].as_i64().map(|n| n as i16),
            started_at: dt_opt(&body["started_at"]),
            ended_at: dt_opt(&body["ended_at"]),
        }
    }

    /// Owned storage for one [`PodMiniRow`]: uuids render once, then the
    /// row borrows the rendered strings.
    struct MiniStorage {
        id: String,
        name: String,
        project: String,
        project_identifier: String,
        is_default: bool,
    }

    impl MiniStorage {
        fn from_detail(detail: &Value) -> Self {
            Self {
                id: text(&detail["id"]),
                name: text(&detail["name"]),
                project: text(&detail["project"]),
                project_identifier: text(&detail["project_identifier"]),
                is_default: detail["is_default"].as_bool().expect("bool"),
            }
        }

        fn row(&self) -> PodMiniRow<'_> {
            PodMiniRow {
                id: &self.id,
                name: &self.name,
                is_default: self.is_default,
                project: &self.project,
                project_identifier: &self.project_identifier,
            }
        }
    }

    #[test]
    fn run_detail_matches_golden_byte_for_byte() {
        let gold = &fx08()["detail"]["ok"]["body"];
        assert!(gold.is_object(), "FX-08 detail.ok.body is an object");
        let run = run_row(gold);
        let mini = MiniStorage::from_detail(&gold["pod_detail"]);
        let mini_row = mini.row();
        let view = run_detail_to_representation(&run, &mini_row, &[]);
        let body = serde_json::to_value(&view).expect("serializes");
        assert_eq!(keys(&body), RUN_WIRE_FIELDS);
        assert_eq!(
            keys(&body["pod_detail"]),
            ["id", "name", "is_default", "project", "project_identifier"]
        );
        assert_eq!(body.to_string(), gold.to_string());
    }

    #[test]
    fn run_list_matches_golden_byte_for_byte() {
        let gold = &fx08()["list"]["golden_row"];
        let run = run_row(gold);
        let mini = MiniStorage::from_detail(&gold["pod_detail"]);
        let mini_row = mini.row();
        let view = run_list_to_representation(&run, &mini_row, &[]);
        let body = serde_json::to_value(&view).expect("serializes");
        assert_eq!(keys(&body), RUN_WIRE_FIELDS);
        // The list pins `error_diagnostic: null` even with `error` set…
        assert_eq!(body["error_diagnostic"], Value::Null);
        assert_eq!(body.to_string(), gold.to_string());
    }

    #[test]
    fn error_diagnostic_list_vs_detail_rule() {
        let gold = &fx08()["list"]["golden_row"];
        let mut run = run_row(gold);
        run.error = "boom x".to_string();
        let mini = MiniStorage::from_detail(&gold["pod_detail"]);
        let mini_row = mini.row();
        let list = serde_json::to_value(run_list_to_representation(&run, &mini_row, &[]))
            .expect("serializes");
        assert_eq!(list["error_diagnostic"], Value::Null);
        let detail = serde_json::to_value(run_detail_to_representation(&run, &mini_row, &[]))
            .expect("serializes");
        assert_eq!(
            detail["error_diagnostic"],
            json!({
                "source": "unknown",
                "source_label": "Unknown",
                "kind": "unknown",
                "summary": "boom x",
                "action": ""
            })
        );
        // …and the detail renders `None` for an empty error (the
        // classifier returns `None` there; the list rule is separate).
        run.error = String::new();
        let detail_empty = serde_json::to_value(run_detail_to_representation(&run, &mini_row, &[]))
            .expect("serializes");
        assert_eq!(detail_empty["error_diagnostic"], Value::Null);
    }

    fn tool_call_row() -> AgentRunToolCall {
        AgentRunToolCall {
            id: uuid("11111111-1111-4111-8111-111111111111"),
            agent_run_id: uuid("22222222-2222-4222-8222-222222222222"),
            tool_call_id: "call_1".to_string(),
            source: "agent".to_string(),
            server_key: "fs".to_string(),
            tool_name: "read".to_string(),
            risk: "low".to_string(),
            status: ToolCallStatus::Succeeded,
            request_fingerprint: String::new(),
            result_fingerprint: String::new(),
            idempotency_key_hash: String::new(),
            external_operation_id: String::new(),
            safe_replay_result: None,
            error_code: String::new(),
            prepared_at: dt("2026-10-02T22:59:51.789134Z"),
            submitted_at: Some(dt("2026-10-02T22:59:52Z")),
            completed_at: None,
        }
    }

    #[test]
    fn tool_call_shape_keys_and_values() {
        let body = serde_json::to_value(tool_call_to_representation(&tool_call_row()))
            .expect("serializes");
        assert_eq!(keys(&body), TOOL_CALL_WIRE_FIELDS);
        assert_eq!(
            body.to_string(),
            concat!(
                "{\"id\":\"11111111-1111-4111-8111-111111111111\",",
                "\"tool_call_id\":\"call_1\",\"source\":\"agent\",",
                "\"server_key\":\"fs\",\"tool_name\":\"read\",\"risk\":\"low\",",
                "\"status\":\"succeeded\",\"error_code\":\"\",",
                "\"prepared_at\":\"2026-10-02T22:59:51.789134Z\",",
                "\"submitted_at\":\"2026-10-02T22:59:52Z\",",
                "\"completed_at\":null}",
            )
        );
    }

    #[test]
    fn run_event_shape_keys_and_values() {
        let row = AgentRunEvent {
            id: 7,
            agent_run_id: uuid("22222222-2222-4222-8222-222222222222"),
            seq: 3,
            kind: "log".to_string(),
            payload: json!({"n": 1}),
            created_at: dt("2026-10-02T22:59:51.789134Z"),
        };
        let body = serde_json::to_value(run_event_to_representation(&row)).expect("serializes");
        assert_eq!(keys(&body), RUN_EVENT_WIRE_FIELDS);
        // Keys also pinned by the detail `include_events` golden.
        assert_eq!(
            keys(&body),
            fx08()["detail"]["events"]["event_keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().expect("str"))
                .collect::<Vec<_>>()
        );
        assert_eq!(body["id"], json!(7));
        assert_eq!(body["seq"], json!(3));
        assert_eq!(body["created_at"], json!("2026-10-02T22:59:51.789134Z"));
    }

    #[test]
    fn approval_shape_keys_match_fixture() {
        let row = ApprovalRequest {
            id: uuid("f4968397-a401-4b0e-b57c-bb64552c466b"),
            agent_run_id: uuid("22222222-2222-4222-8222-222222222222"),
            kind: ApprovalKind::CommandExecution,
            payload: json!({}),
            reason: "why".to_string(),
            status: ApprovalStatus::Pending,
            decision_source: String::new(),
            decided_by_id: None,
            requested_at: dt("2026-10-02T23:03:54.739663Z"),
            expires_at: None,
            decided_at: None,
        };
        let body = serde_json::to_value(approval_to_representation(&row)).expect("serializes");
        assert_eq!(keys(&body), APPROVAL_WIRE_FIELDS);
        assert_eq!(
            keys(&body),
            fx08()["approvals_list"]["ok"]["keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().expect("str"))
                .collect::<Vec<_>>()
        );
        assert_eq!(body["kind"], json!("command_execution"));
        assert_eq!(body["status"], json!("pending"));
        assert_eq!(body["requested_at"], json!("2026-10-02T23:03:54.739663Z"));
    }

    #[test]
    fn chat_message_matches_golden_byte_for_byte() {
        let gold = &fx09()["web_chat"]["msg_post_ok"]["msg"];
        let row = AgentChatMessage {
            id: uuid(gold["id"].as_str().expect("id")),
            session_id: uuid(gold["session"].as_str().expect("session")),
            role: serde_json::from_value(gold["role"].clone()).expect("role"),
            content: text(&gold["content"]),
            content_parts: gold["content_parts"].clone(),
            status: serde_json::from_value(gold["status"].clone()).expect("status"),
            local_item_id: text(&gold["local_item_id"]),
            local_turn_id: text(&gold["local_turn_id"]),
            seq: gold["seq"].as_i64().expect("seq") as i32,
            created_at: dt(gold["created_at"].as_str().expect("created_at")),
            completed_at: dt_opt(&gold["completed_at"]),
        };
        let body = serde_json::to_value(chat_message_to_representation(&row)).expect("serializes");
        assert_eq!(keys(&body), CHAT_MESSAGE_WIRE_FIELDS);
        assert_eq!(
            keys(&body),
            fx09()["web_chat"]["msg_list"]["keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().expect("str"))
                .collect::<Vec<_>>()
        );
        assert_eq!(body.to_string(), gold.to_string());
    }

    #[test]
    fn chat_approval_matches_golden_byte_for_byte() {
        let gold = &fx09()["web_chat"]["cappr_decide_ok"]["approval"];
        let row = AgentChatApprovalRequest {
            id: uuid(gold["id"].as_str().expect("id")),
            session_id: uuid(gold["session"].as_str().expect("session")),
            local_approval_id: text(&gold["local_approval_id"]),
            kind: serde_json::from_value(gold["kind"].clone()).expect("kind"),
            payload: gold["payload"].clone(),
            reason: text(&gold["reason"]),
            status: serde_json::from_value(gold["status"].clone()).expect("status"),
            decision_source: text(&gold["decision_source"]),
            decided_by_id: uuid_opt(&gold["decided_by"]),
            requested_at: dt(gold["requested_at"].as_str().expect("requested_at")),
            expires_at: dt_opt(&gold["expires_at"]),
            decided_at: dt_opt(&gold["decided_at"]),
        };
        let body = serde_json::to_value(chat_approval_to_representation(&row)).expect("serializes");
        assert_eq!(keys(&body), CHAT_APPROVAL_WIRE_FIELDS);
        assert_eq!(body.to_string(), gold.to_string());
    }

    #[test]
    fn chat_session_shape_keys_and_runner_detail() {
        let usage = json!({});
        let dev_metadata = json!({});
        let capabilities = json!({});
        let session = AgentChatSession {
            id: uuid("ba8f55bc-9306-4175-8ac9-839992652bd4"),
            workspace_id: uuid("00000000-0000-4000-8000-000000000000"),
            runner_id: uuid("e39ef475-5212-4de8-a5d0-e5fe8fd01f7b"),
            created_by_id: uuid("dae0b0de-c448-4a8c-bb4b-a63e382c196f"),
            pod_id: uuid("dd43f0eb-d975-431d-b661-8925300ae608"),
            status: AgentChatSessionStatus::Open,
            agent_kind: "claude".to_string(),
            local_thread_id: String::new(),
            local_session_id: String::new(),
            cwd: String::new(),
            model: "m".to_string(),
            active_turn_id: String::new(),
            active_message_id: None,
            close_requested: false,
            last_message_at: None,
            closed_at: None,
            error: String::new(),
            created_at: dt("2026-10-02T23:03:54.579295Z"),
            updated_at: dt("2026-10-02T23:03:54.579295Z"),
        };
        let mini = MiniStorage {
            id: "dd43f0eb-d975-431d-b661-8925300ae608".to_string(),
            name: "FX8B_pod_1".to_string(),
            project: "eb0e9859-36d1-45e3-bcc7-cb2f112c3974".to_string(),
            project_identifier: "FX8B".to_string(),
            is_default: true,
        };
        let mini_row = mini.row();
        let live = LiveStateRow {
            observed_run_id: None,
            last_event_at: None,
            last_event_kind: None,
            last_event_summary: None,
            agent_pid: None,
            agent_subprocess_alive: None,
            approvals_pending: None,
            usage: &usage,
            llm_model: None,
            turn_count: None,
            updated_at: "2026-10-02T23:03:54.579295Z",
        };
        let machine = DevMachineMiniRow {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            host_label: "h",
            label: "l",
        };
        let runner = RunnerRow {
            id: "e39ef475-5212-4de8-a5d0-e5fe8fd01f7b",
            name: "r1",
            status: "online",
            host_label: "h",
            provisioning: "manual",
            os: "linux",
            arch: "arm64",
            runner_version: "1.0",
            dev_metadata: &dev_metadata,
            protocol_version: 1,
            capabilities: &capabilities,
            last_heartbeat_at: None,
            owner: "dae0b0de-c448-4a8c-bb4b-a63e382c196f",
            dev_machine: Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            dev_machine_detail: Some(machine),
            visibility: 0,
            pod: "dd43f0eb-d975-431d-b661-8925300ae608",
            pod_detail: mini_row,
            live_state: Some(live),
            enrolled_at: None,
            revoked_at: None,
            revoked_reason: "",
            created_at: "2026-10-02T23:03:54.579295Z",
            updated_at: "2026-10-02T23:03:54.579295Z",
        };
        let body = serde_json::to_value(chat_session_to_representation(&session, &runner))
            .expect("serializes");
        assert_eq!(keys(&body), CHAT_SESSION_WIRE_FIELDS);
        assert_eq!(
            keys(&body),
            fx09()["web_chat"]["sess_list_owner"]["keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().expect("str"))
                .collect::<Vec<_>>()
        );
        // The embed is D-13's kernel: its wire order holds by construction.
        assert_eq!(keys(&body["runner_detail"]), RUNNER_WIRE_FIELDS);
        assert_eq!(body["close_requested"], json!(false));
        assert_eq!(body["active_message_id"], Value::Null);
    }

    #[test]
    fn chat_event_shape_keys_and_values() {
        let row = AgentChatEvent {
            id: 121,
            session_id: uuid("0a4b4a35-efae-4d39-a9e1-713211c11d74"),
            message_id: None,
            seq: 1,
            source_key: "k".to_string(),
            kind: "k1".to_string(),
            payload: json!({"n": 1}),
            created_at: dt("2026-10-02T23:10:04.602929Z"),
        };
        let body = serde_json::to_value(chat_event_to_representation(&row)).expect("serializes");
        assert_eq!(keys(&body), CHAT_EVENT_WIRE_FIELDS);
        assert_eq!(body["id"], json!(121));
        assert_eq!(body["message"], Value::Null);
        assert_eq!(body["created_at"], json!("2026-10-02T23:10:04.602929Z"));
        // Non-null message renders the uuid.
        let mut with_message = row;
        with_message.message_id = Some(uuid("21a4c91a-449a-4c61-a118-31db35c3fc11"));
        let body =
            serde_json::to_value(chat_event_to_representation(&with_message)).expect("serializes");
        assert_eq!(
            body["message"],
            json!("21a4c91a-449a-4c61-a118-31db35c3fc11")
        );
    }

    #[test]
    fn decision_validation_matches_drf() {
        // Fixture goldens first.
        assert_eq!(
            validate_approval_decision(&json!({"decision": "maybe"})).expect_err("400"),
            fx08()["decide"]["bad_choice"]["body"]
        );
        assert_eq!(
            validate_approval_decision(&json!({})).expect_err("400"),
            fx08()["decide"]["missing_field"]["body"]
        );
        assert_eq!(
            validate_approval_decision(&json!({"decision": "maybe"})).expect_err("400"),
            fx09()["web_chat"]["cappr_decide_bad"]["body"]
        );
        // Full DRF matrix, verified against live DRF 3.16 (messages match
        // the 3.15.2-recorded goldens).
        assert_eq!(
            validate_approval_decision(&json!({"decision": "accept"})),
            Ok(ApprovalDecision::Accept)
        );
        assert_eq!(
            validate_approval_decision(&json!({"decision": "decline"})),
            Ok(ApprovalDecision::Decline)
        );
        for (input, message) in [
            (Value::Null, "This field may not be null."),
            (json!(""), "\"\" is not a valid choice."),
            (json!("Accept"), "\"Accept\" is not a valid choice."),
            (json!(5), "\"5\" is not a valid choice."),
            (json!(true), "\"True\" is not a valid choice."),
            (json!(false), "\"False\" is not a valid choice."),
            (json!(["accept"]), "\"['accept']\" is not a valid choice."),
            (json!({"a": 1}), "\"{'a': 1}\" is not a valid choice."),
            (json!(1.5), "\"1.5\" is not a valid choice."),
            (json!(1.0), "\"1.0\" is not a valid choice."),
        ] {
            assert_eq!(
                validate_approval_decision(&json!({"decision": input})).expect_err("400"),
                json!({"decision": [message]}),
                "input {input}"
            );
        }
        assert_eq!(
            validate_approval_decision(&Value::Null).expect_err("400"),
            json!({"non_field_errors": ["No data provided"]})
        );
        for (input, datatype) in [
            (json!([1, 2]), "list"),
            (json!("x"), "str"),
            (json!(5), "int"),
            (json!(1.5), "float"),
            (json!(true), "bool"),
        ] {
            assert_eq!(
                validate_approval_decision(&input).expect_err("400"),
                json!({"non_field_errors": [format!(
                    "Invalid data. Expected a dictionary, but got {datatype}."
                )]}),
                "input {input}"
            );
        }
    }

    #[test]
    fn float_repr_matches_cpython() {
        // Expected values are CPython `repr()` output, generated 2026-10-03.
        let cases: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.0, "-1.0"),
            (1.5, "1.5"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (0.2, "0.2"),
            (0.3, "0.3"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (-1e16, "-1e+16"),
            (1e20, "1e+20"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (123.456, "123.456"),
            (100.0, "100.0"),
            (123456789.0, "123456789.0"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (2.2250738585072014e-308, "2.2250738585072014e-308"),
            (0.30000000000000004, "0.30000000000000004"),
            (1.1, "1.1"),
            (std::f64::consts::PI, "3.141592653589793"),
            (2.5e-7, "2.5e-07"),
            (1.2345678901234568, "1.2345678901234567"),
            (1e21, "1e+21"),
            (1e22, "1e+22"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (1e-4, "0.0001"),
            (1e-3, "0.001"),
            (0.001, "0.001"),
            (12.0, "12.0"),
            (1e10, "10000000000.0"),
            (1234567.891, "1234567.891"),
            (1.0000000000000002, "1.0000000000000002"),
        ];
        for (value, want) in cases {
            assert_eq!(&py_float_repr(*value), want, "repr({value})");
        }
        assert_eq!(py_float_repr(f64::NAN), "nan");
        assert_eq!(py_float_repr(f64::INFINITY), "inf");
        assert_eq!(py_float_repr(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn str_repr_matches_cpython() {
        // Expected values are CPython `repr()` output, generated 2026-10-03.
        // (`\u{2028}` is the documented Cf/Zl divergence and is excluded.)
        let cases: &[(&str, &str)] = &[
            ("accept", "'accept'"),
            ("", "''"),
            ("a\"b", "'a\"b'"),
            ("a'b", "\"a'b\""),
            ("a\\b", "'a\\\\b'"),
            ("l1\nl2", "'l1\\nl2'"),
            ("t\tt", "'t\\tt'"),
            ("r\rr", "'r\\rr'"),
            ("café", "'café'"),
            ("中文", "'中文'"),
            ("\u{0}", "'\\x00'"),
            ("\u{1b}", "'\\x1b'"),
            ("\u{7f}", "'\\x7f'"),
            ("\u{80}", "'\\x80'"),
            ("\u{85}", "'\\x85'"),
            ("\"quoted\"", "'\"quoted\"'"),
            ("'sq'", "\"'sq'\""),
            ("mix\"'q", "'mix\"\\'q'"),
        ];
        for (input, want) in cases {
            assert_eq!(&py_str_repr(input), want, "repr({input:?})");
        }
        assert_eq!(py_repr(&json!(["accept"])), "['accept']");
        assert_eq!(py_repr(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(
            py_repr(&json!([1, "x", Value::Null, true, 1.5])),
            "[1, 'x', None, True, 1.5]"
        );
        assert_eq!(py_repr(&json!({"k": [1.0, false]})), "{'k': [1.0, False]}");
    }
}
