//! Chat service closure (D-15 L5, stage 5).
//!
//! Ports `apps/api/pi_dash/runner/services/chat.py:1-497`:
//!
//! * `event_channel` (`:44-45`) and the `:40-41` constants are L1
//!   ([`pidash_types::runner_runs::consts`]), reused, not redefined.
//! * `can_read_chat` / `can_send_chat` / `can_decide_chat_approval`
//!   (`:48-68`) → [`can_read_chat`] / [`can_send_chat`] /
//!   [`can_decide_chat_approval`]. The L3 permission predicates
//!   (`services/permissions.py`, PIDASHCONV-529, unmerged) arrive as
//!   `FnOnce` seams so the Python short-circuit order is preserved
//!   and provable (the `admission.rs` seam precedent).
//! * `runner_has_active_chat` (`:71-79`) → [`runner_has_active_chat_sql`]
//!   over [`ACTIVE_TURN_PREDICATE_SQL`].
//! * `drain_tasks_after_chat_release` (`:82-109`) →
//!   [`drain_tasks_after_chat_release`], which queues
//!   [`ChatEffect::DrainTasks`]. The D-14 matcher callees
//!   (`drain_for_runner_by_id` / `drain_pod_by_id`, PIDASHCONV-552,
//!   unmerged) run in the executor (L6b/L8), which also owns the
//!   swallow-and-log wrapper.
//! * `normalize_cwd` (`:112-115`) → [`normalize_cwd`].
//! * `next_message_seq_locked` / `next_event_seq_locked` (`:118-125`) →
//!   [`next_message_seq_sql`] / [`next_event_seq_sql`] +
//!   [`next_seq_after_max`].
//! * `serialize_event` (`:128-137`) → [`serialize_event_json`].
//! * `publish_event` (`:140-147`) → [`publish_frame`]. The Redis PUBLISH
//!   itself runs in the executor over the foundation handle; a missing
//!   client is a no-op there, as in Python.
//! * `append_event_locked` (`:150-171`) → [`append_dedupe_check_sql`] +
//!   [`append_event_inputs`] + [`insert_event_sql`]. The caller inserts,
//!   then queues [`ChatEffect::PublishEvent`]; a dedupe hit returns the
//!   existing row and queues nothing.
//! * `record_dedupe` (`:180-190`) → [`record_dedupe_message_id`] +
//!   [`insert_dedupe_sql`]; the caller maps a unique violation to
//!   `false` from inside a savepoint.
//! * `enqueue_chat_message_after_commit` (`:193-224`) /
//!   `enqueue_chat_warm_after_commit` (`:227-253`) →
//!   [`ChatUserMessagePayload`] / [`ChatWarmPayload`] +
//!   [`enqueue_chat_message_after_commit`] /
//!   [`enqueue_chat_warm_after_commit`], which queue
//!   [`ChatEffect::SendChatMessage`] / [`ChatEffect::SendChatWarm`].
//!   The D-14 `send_to_runner` call (PIDASHCONV-553, unmerged) and the
//!   failure callbacks run in the executor.
//! * `mark_warm_dispatch_failed` (`:256-270`) → [`mark_warm_lock_sql`] +
//!   [`plan_mark_warm_dispatch_failed`].
//! * `mark_message_dispatch_failed` (`:272-302`),
//!   `sweep_active_turns` rows (`:403-448`),
//!   `release_active_chats_for_runner` rows (`:451-480`) → the shared
//!   [`plan_fail_active_turn`] core plus the [`plan_mark_message_dispatch_failed`],
//!   [`plan_sweep_active_row`] and [`plan_release_row`] wrappers.
//! * `create_assistant_message_locked` (`:305-319`) →
//!   [`create_assistant_inputs`] + [`insert_message_sql`].
//! * `active_assistant_message_locked` (`:322-332`) →
//!   [`assistant_turn_scoped_sql`] + [`assistant_fallback_sql`] +
//!   [`select_active_assistant`] (the fallback query stays lazy).
//! * `finalize_active_messages_locked` (`:335-356`) →
//!   [`finalize_target_message_id`] +
//!   [`finalize_explicit_lookup_sql`] + [`plan_finalize_active_messages`].
//! * `complete_active_turn_locked` (`:359-400`) →
//!   [`plan_complete_active_turn`] + [`complete_session_update_sql`].
//! * `sweep_active_turns` (`:403-448`) → [`sweep_active_ids_sql`] +
//!   [`session_lock_open_sql`] + [`plan_sweep_active_row`].
//! * `release_active_chats_for_runner` (`:451-480`) →
//!   [`release_ids_sql`] + [`session_lock_open_sql`] +
//!   [`release_row_applies`] + [`plan_release_row`].
//! * `sweep_empty_sessions` (`:483-497`) → [`sweep_empty_ids_sql`] +
//!   [`sweep_empty_close_sql`].
//!
//! Shape of the port: pure logic plus SQL text, no execution. The
//! services crate carries no `sqlx` handle, so every statement is a
//! `*_sql` builder in Django-compiler form (literals interpolated, as
//! `CaptureQueriesContext` records them); production executors bind
//! parameters instead — the builders pin the statement shape, they are
//! not the execution path. Post-commit registrations surface as
//! [`ChatEffect`] values pushed into a caller-provided sink (`&mut dyn
//! FnMut(ChatEffect)`, the `dispatch/admission.rs` precedent); dropping
//! the sink without running it is the rollback path.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-06-chat-service.golden.json`
//! (FX-RUN-06); the `#[cfg(test)]` suite replays it.
//!
//! Ported bugs and quirks (translated, not fixed; also listed in the PR):
//!
//! * `BUG-COMPLETE-UNSCOPED (chat.py:368)`: `complete_active_turn_locked`
//!   updates the active message by bare pk, unlike `finalize`, which
//!   scopes the lookup by session (`:345`). A stale cross-session
//!   `active_message_id` would finalize another session's message.
//!   [`complete_message_update_sql`] keeps the unscoped `WHERE`.
//! * `QUIRK-ERROR-TRUNCATION (:281,470,296,475)`: `mark_...` and
//!   `release_...` store `detail[:2000]` in `session.error` but copy
//!   the FULL detail into the `chat_failed` payload. Kept.
//! * `QUIRK-DRAIN-NONE (:95-96)`: the `runner_id is None` / `pod_id is
//!   None` guards are unreachable from database rows (both FKs are
//!   non-nullable, `models.py:1256,1262-1266`) but kept as `Option`
//!   inputs.
//! * `QUIRK-DEDUP-SAVEPOINT (:189)`: a duplicate `record_dedupe` insert
//!   raises `IntegrityError`, which aborts the Postgres transaction
//!   unless the caller holds a savepoint. Callers must create one.
//! * `QUIRK-EMPTY-ORDER (:485-491)`: the `sweep_empty` id select carries
//!   the default `ORDER BY` although only ids are consumed. Kept.

use chrono::{DateTime, Utc};
use pidash_db::runner_enroll::columns::enums::RUNNER_STATUS_OFFLINE;
use pidash_db::runner_enroll::columns::runner::TABLE as RUNNER_TABLE;
use pidash_db::runner_runs::{
    chat_dedupe, chat_event, chat_message, chat_session, AgentChatMessage, AgentChatSession,
};
use pidash_types::runner_runs::consts::{event_channel, CHAT_ACTIVE_TIMEOUT_SECS};
use pidash_types::runner_runs::{
    AgentChatMessageRole, AgentChatMessageStatus, AgentChatSessionStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Guards (`chat.py:48-68`)
// ---------------------------------------------------------------------------

/// `can_read_chat` (`chat.py:48-53`): member, then runner-visible, then
/// creator-or-admin. The L3 predicates arrive as seams, consulted in
/// Python order: a non-member never consults the runner check, and a
/// creator never consults the admin check.
pub fn can_read_chat(
    user_id: Uuid,
    created_by_id: Uuid,
    is_workspace_member: impl FnOnce() -> bool,
    can_view_runner: impl FnOnce() -> bool,
    is_workspace_admin: impl FnOnce() -> bool,
) -> bool {
    if !is_workspace_member() {
        return false;
    }
    if !can_view_runner() {
        return false;
    }
    created_by_id == user_id || is_workspace_admin()
}

/// `can_send_chat` (`chat.py:56-59`): member and creator-with-use. The
/// creator comparison runs before the runner check, as in Python, so a
/// non-creator never consults `can_use_runner`.
pub fn can_send_chat(
    user_id: Uuid,
    created_by_id: Uuid,
    is_workspace_member: impl FnOnce() -> bool,
    can_use_runner: impl FnOnce() -> bool,
) -> bool {
    if !is_workspace_member() {
        return false;
    }
    created_by_id == user_id && can_use_runner()
}

/// `can_decide_chat_approval` (`chat.py:62-68`): the `can_read_chat`
/// predicate evaluated over the approval's session. The caller resolves
/// `approval.session` (FK fetch) and passes its ids down.
pub fn can_decide_chat_approval(
    user_id: Uuid,
    created_by_id: Uuid,
    is_workspace_member: impl FnOnce() -> bool,
    can_view_runner: impl FnOnce() -> bool,
    is_workspace_admin: impl FnOnce() -> bool,
) -> bool {
    can_read_chat(
        user_id,
        created_by_id,
        is_workspace_member,
        can_view_runner,
        is_workspace_admin,
    )
}

// ---------------------------------------------------------------------------
// Post-commit effects (the three `transaction.on_commit` sites)
// ---------------------------------------------------------------------------

/// A side effect `chat.py` defers with `transaction.on_commit`, as data.
/// Executors (L6b tasks / L8 handlers) drain the sink after commit: event
/// publishes go through the foundation Redis handle, drains and runner
/// sends through the D-14 ports (PIDASHCONV-552 / PIDASHCONV-553).
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEffect {
    /// `append_event_locked.<locals>.<lambda>` (`chat.py:170`):
    /// re-read the event row by id and publish it
    /// (`_publish_event_by_id`, `:174-177`).
    PublishEvent {
        /// The new `AgentChatEvent` id.
        event_id: i64,
    },
    /// `drain_tasks_after_chat_release.<locals>._drain` (`chat.py:109`):
    /// run the D-14 matcher drains. The executor calls
    /// `drain_for_runner_by_id` first and `drain_pod_by_id` (when
    /// present) second inside ONE swallow-and-log guard, so a failure
    /// in the first skips the second, exactly as Python's single
    /// `try` (`chat.py:98-107`).
    DrainTasks {
        /// `session.runner_id` (always present; a `None` runner
        /// queues no effect).
        runner_id: Uuid,
        /// `session.pod_id` (drained only when present).
        pod_id: Option<Uuid>,
    },
    /// `enqueue_chat_message_after_commit.<locals>._send` (`chat.py:224`):
    /// deliver the payload via D-14 `send_to_runner`; on error the
    /// executor runs `mark_message_dispatch_failed`.
    SendChatMessage {
        /// Target runner.
        runner_id: Uuid,
        /// The `chat_user_message` frame.
        payload: ChatUserMessagePayload,
    },
    /// `enqueue_chat_warm_after_commit.<locals>._send` (`chat.py:253`):
    /// deliver the payload via D-14 `send_to_runner`; on error the
    /// executor logs and runs `mark_warm_dispatch_failed`.
    SendChatWarm {
        /// Target runner.
        runner_id: Uuid,
        /// The `chat_warm` frame.
        payload: ChatWarmPayload,
    },
}

/// `drain_tasks_after_chat_release` (`chat.py:82-109`): queue the drain
/// effect, unless the runner is missing (the `:95-96` early return).
/// `pod_id` rides along for the executor's second drain call.
pub fn drain_tasks_after_chat_release(
    runner_id: Option<Uuid>,
    pod_id: Option<Uuid>,
    effects: &mut dyn FnMut(ChatEffect),
) {
    let Some(runner_id) = runner_id else {
        return;
    };
    effects(ChatEffect::DrainTasks { runner_id, pod_id });
}

// ---------------------------------------------------------------------------
// SQL literal rendering (Django-compiler form, as recorded)
// ---------------------------------------------------------------------------

/// A UUID as Django interpolates it (`'<32 hex>'::uuid`, FX-RUN-06
/// `release_active.first_sql`).
fn lit_uuid(id: Uuid) -> String {
    format!("'{}'::uuid", id.as_simple())
}

/// A timestamptz as Django interpolates it
/// (`'YYYY-MM-DD HH:MM:SS.ffffff+00:00'::timestamptz`, FX-RUN-06
/// `sweep_empty.sql`).
fn lit_tstz(stamp: DateTime<Utc>) -> String {
    format!(
        "'{}'::timestamptz",
        stamp.format("%Y-%m-%d %H:%M:%S%.6f%:z")
    )
}

/// A string literal (`''`-doubled quoting).
fn lit_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The active-turn predicate shared by the EXISTS check and the sweep
/// id selects (`Q(active_message_id__isnull=False) |
/// ~Q(active_turn_id="")`, `chat.py:77,407,454`): the `NOT (...)` form
/// is Django's rendering of the negated `Q`, kept verbatim.
pub const ACTIVE_TURN_PREDICATE_SQL: &str = "\"agent_chat_session\".\"active_message_id\" IS NOT NULL OR NOT (\"agent_chat_session\".\"active_turn_id\" = '')";

/// `runner_has_active_chat` (`chat.py:71-79`): OPEN sessions for the
/// runner with an active turn, as an EXISTS probe.
pub fn runner_has_active_chat_sql(runner_id: Uuid) -> String {
    format!(
        "SELECT 1 AS \"a\" FROM \"agent_chat_session\" WHERE (\"agent_chat_session\".\"runner_id\" = {} AND \"agent_chat_session\".\"status\" = '{}' AND ({})) LIMIT 1",
        lit_uuid(runner_id),
        AgentChatSessionStatus::Open.value(),
        ACTIVE_TURN_PREDICATE_SQL,
    )
}

// ---------------------------------------------------------------------------
// `normalize_cwd` (`chat.py:112-115`)
// ---------------------------------------------------------------------------

/// The cloud ignores caller-selected cwd (`chat.py:112-115`); every
/// input maps to `""`. Generic over the input to mirror the `Any`
/// annotation (FX-RUN-06 passes paths, `None`, an int and a dict).
pub fn normalize_cwd<T>(_value: T) -> &'static str {
    ""
}

// ---------------------------------------------------------------------------
// Seq allocators (`chat.py:118-125`)
// ---------------------------------------------------------------------------

/// `next_message_seq_locked` (`chat.py:118-120`): `MAX(seq)` over the
/// session's messages. The caller adds one via [`next_seq_after_max`].
pub fn next_message_seq_sql(session_id: Uuid) -> String {
    format!(
        "SELECT MAX(\"{}\".\"seq\") AS \"seq__max\" FROM \"{}\" WHERE \"{}\".\"session_id\" = {}",
        chat_message::TABLE,
        chat_message::TABLE,
        chat_message::TABLE,
        lit_uuid(session_id),
    )
}

/// `next_event_seq_locked` (`chat.py:123-125`): `MAX(seq)` over the
/// session's events. The caller adds one via [`next_seq_after_max`].
pub fn next_event_seq_sql(session_id: Uuid) -> String {
    format!(
        "SELECT MAX(\"{}\".\"seq\") AS \"seq__max\" FROM \"{}\" WHERE \"{}\".\"session_id\" = {}",
        chat_event::TABLE,
        chat_event::TABLE,
        chat_event::TABLE,
        lit_uuid(session_id),
    )
}

/// `int(current) + 1` with `None or 0` (`chat.py:119,124`): `MAX` over
/// an empty set is `NULL`, so the first seq is 1; gaps are never
/// filled (FX-RUN-06 `seq_rule`).
pub fn next_seq_after_max(current_max: Option<i32>) -> i32 {
    current_max.unwrap_or(0) + 1
}

// ---------------------------------------------------------------------------
// `json.dumps` rendering (`serialize_event` / `publish_event`)
// ---------------------------------------------------------------------------

/// Render a string exactly like stdlib `json.dumps`
/// (`ensure_ascii=True`): short escapes for `\"`, `\\`, `\b`, `\f`,
/// `\n`, `\r`, `\t`; `\u00xx` (lowercase) for other controls and DEL;
/// `\uXXXX` (lowercase, surrogate pairs past the BMP) for everything
/// non-ASCII. Same rules as `db::assistant::event_queries`
/// (D-06, unexported there), repeated here because services cannot
/// reach into that private helper.
fn dumps_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{00}'..='\u{1f}' | '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            '\u{80}'..='\u{ffff}' => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            _ if ch as u32 > 0xffff => {
                let shifted = ch as u32 - 0x1_0000;
                out.push_str(&format!(
                    "\\u{:04x}\\u{:04x}",
                    0xd800 + (shifted >> 10),
                    0xdc00 + (shifted & 0x3ff)
                ));
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Render a float exactly like `json.dumps`: shortest-roundtrip
/// digits (as `serde_json` renders them) with Python's spelling —
/// an explicit exponent sign with at least two digits (`1e-07`,
/// `1e+300`), and scientific notation from `1e-5` down, where
/// `serde_json` still prints fixed (`0.00001` vs Python `1e-05`).
/// Non-finite values cannot occur (`serde_json` cannot hold them
/// and `jsonb` cannot store them).
fn dumps_float_text(rendered: &str) -> String {
    if let Some(pos) = rendered.find(['e', 'E']) {
        let (mantissa, exp_text) = rendered.split_at(pos);
        let exp: i32 = exp_text[1..].parse().unwrap_or(0);
        return format!("{mantissa}e{exp:+03}");
    }
    let (sign, digits) = match rendered.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", rendered),
    };
    if let Some(frac) = digits.strip_prefix("0.") {
        let zeros = frac.chars().take_while(|ch| *ch == '0').count();
        if zeros >= 4 {
            let significant: String = frac.chars().skip(zeros).collect();
            let mut chars = significant.chars();
            let first = chars.next().unwrap_or('0');
            let rest: String = chars.collect();
            let exp = -(zeros as i32) - 1;
            if rest.is_empty() {
                return format!("{sign}{first}e{exp:+03}");
            }
            return format!("{sign}{first}.{rest}e{exp:+03}");
        }
    }
    rendered.to_owned()
}

/// Render a number exactly like `json.dumps`: integers raw, floats
/// via [`dumps_float_text`].
fn dumps_number(value: &serde_json::Number) -> String {
    if let Some(int) = value.as_i64() {
        return int.to_string();
    }
    if let Some(int) = value.as_u64() {
        return int.to_string();
    }
    match value.as_f64() {
        Some(_) => dumps_float_text(&value.to_string()),
        None => "null".to_owned(),
    }
}

/// Render a value exactly like `json.dumps` with the default
/// `(', ', ': ')` separators. Object keys render in the map's
/// iteration order; callers pass database-read payloads, whose key
/// order is the `jsonb` normalization — identical on both backends.
/// Public so the publish executor (L8) renders event payloads with
/// the same bytes instead of forking this renderer.
pub fn dumps_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(true) => "true".to_owned(),
        Value::Bool(false) => "false".to_owned(),
        Value::Number(number) => dumps_number(number),
        Value::String(text) => dumps_string(text),
        Value::Array(items) => {
            let rendered: Vec<String> = items.iter().map(dumps_value).collect();
            format!("[{}]", rendered.join(", "))
        }
        Value::Object(map) => {
            let rendered: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", dumps_string(key), dumps_value(item)))
                .collect();
            format!("{{{}}}", rendered.join(", "))
        }
    }
}

/// Inputs to [`serialize_event_json`]. Ids and timestamps cross
/// pre-rendered (the `dtos.rs` convention): UUIDs as hyphenated
/// strings, `created_at` as the Python `datetime.isoformat()`
/// rendering, `payload` as already-serialized JSON.
pub struct EventParts<'a> {
    /// `event.id`.
    pub id: i64,
    /// `str(event.session_id)`, hyphenated.
    pub session_id: &'a str,
    /// `str(event.message_id)`, hyphenated, when set.
    pub message_id: Option<&'a str>,
    /// `event.seq`.
    pub seq: i32,
    /// `event.kind`.
    pub kind: &'a str,
    /// The payload rendered with [`dumps_value`] rules.
    pub payload_json: &'a str,
    /// `event.created_at.isoformat()`.
    pub created_at: &'a str,
}

/// Event wire shape (`chat.py:128-137`) rendered byte-for-byte like
/// `json.dumps(..., default=str)`: Python key order (`id, session,
/// message, seq, kind, payload, created_at`) with `(', ', ': ')`
/// separators. This is what `publish_event` puts on the channel and
/// what SSE subscribers receive verbatim.
pub fn serialize_event_json(parts: &EventParts<'_>) -> String {
    let message = match parts.message_id {
        Some(id) => dumps_string(id),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\": {}, \"session\": {}, \"message\": {}, \"seq\": {}, \"kind\": {}, \"payload\": {}, \"created_at\": {}}}",
        parts.id,
        dumps_string(parts.session_id),
        message,
        parts.seq,
        dumps_string(parts.kind),
        parts.payload_json,
        dumps_string(parts.created_at)
    )
}

/// `PUBLISH <channel> <serialize_event JSON>` frame
/// (`chat.py:140-147`). The channel reuses L1 [`event_channel`]; the
/// executor publishes this and swallows every Redis failure (log and
/// return, never raise): a missing client and a failed `publish` both
/// end the call silently.
pub fn publish_frame(session_id: Uuid, event_json: &str) -> (String, String) {
    (
        event_channel(&session_id.to_string()),
        event_json.to_owned(),
    )
}

/// Re-read for `_publish_event_by_id` (`chat.py:174-177`): the event
/// row by pk (a miss publishes nothing). Like
/// [`append_dedupe_check_sql`], the FK half of the default ordering
/// joins the session table for its `-last_message_at`,
/// `-created_at`.
pub fn publish_event_lookup_sql(event_id: i64) -> String {
    format!(
        "SELECT {} FROM \"{}\" INNER JOIN \"{}\" ON (\"{}\".\"session_id\" = \"{}\".\"id\") WHERE \"{}\".\"id\" = {} ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC, \"{}\".\"seq\" ASC LIMIT 1",
        quoted_columns(chat_event::TABLE, chat_event::COLUMNS),
        chat_event::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
        event_id,
        chat_session::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
    )
}

/// `"table"."col", ...` in the given order.
fn quoted_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Shared value rules
// ---------------------------------------------------------------------------

/// Truncate to a column bound the way Python `value[:bound]` does:
/// code points, never bytes, so the cut lands on a char boundary
/// (the `[:255]` semantic trap). Bounds come from L2.
pub fn truncate_chars(value: &str, bound: usize) -> &str {
    if value.chars().count() <= bound {
        return value;
    }
    let end = value
        .char_indices()
        .nth(bound)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}

/// Python truthiness over a JSON value, for the `payload or ...`
/// defaults (`chat.py:168,395`): `None`, `{}`, `[]`, `""`, `0` and
/// `false` all fall back.
fn is_python_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => {
            number.as_i64() == Some(0) || number.as_u64() == Some(0) || number.as_f64() == Some(0.0)
        }
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    }
}

/// `value or None` for the enqueue option fields (`chat.py:215-218,
/// 243-246`): an empty string sends JSON `null`.
fn or_none(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

// ---------------------------------------------------------------------------
// `append_event_locked` (`chat.py:150-171`)
// ---------------------------------------------------------------------------

/// A new event row's inputs: `source_key[:160]`, `kind[:64]`,
/// `payload or {}` (`chat.py:162-169`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEventInputs {
    /// Truncated to [`chat_event::SOURCE_KEY_MAX_LENGTH`].
    pub source_key: String,
    /// Truncated to [`chat_event::KIND_MAX_LENGTH`].
    pub kind: String,
    /// `payload or {}`.
    pub payload: Value,
    /// The attached message, if any.
    pub message_id: Option<Uuid>,
}

/// Build the insert inputs for `append_event_locked` (`chat.py:162-169`).
pub fn append_event_inputs(
    kind: &str,
    payload: Option<&Value>,
    message_id: Option<Uuid>,
    source_key: &str,
) -> NewEventInputs {
    let payload = match payload {
        Some(value) if !is_python_falsy(value) => value.clone(),
        _ => Value::Object(Default::default()),
    };
    NewEventInputs {
        source_key: truncate_chars(source_key, chat_event::SOURCE_KEY_MAX_LENGTH).to_owned(),
        kind: truncate_chars(kind, chat_event::KIND_MAX_LENGTH).to_owned(),
        payload,
        message_id,
    }
}

/// Whether the dedupe probe runs: only a non-empty `source_key`
/// dedupes (`chat.py:158`).
pub fn append_checks_dedupe(source_key: &str) -> bool {
    !source_key.is_empty()
}

/// The dedupe probe (`chat.py:159`): the session's event with this
/// `source_key`, default ordering, first row. The `ORDER BY session`
/// half of the model's `ordering = ["session", "seq"]` resolves
/// through the related model's own ordering, so Django joins
/// `agent_chat_session` and sorts by its `-last_message_at`,
/// `-created_at` before the event's `seq`.
pub fn append_dedupe_check_sql(session_id: Uuid, source_key: &str) -> String {
    format!(
        "SELECT {} FROM \"{}\" INNER JOIN \"{}\" ON (\"{}\".\"session_id\" = \"{}\".\"id\") WHERE (\"{}\".\"session_id\" = {} AND \"{}\".\"source_key\" = {}) ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC, \"{}\".\"seq\" ASC LIMIT 1",
        quoted_columns(chat_event::TABLE, chat_event::COLUMNS),
        chat_event::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
        lit_uuid(session_id),
        chat_event::TABLE,
        lit_str(source_key),
        chat_session::TABLE,
        chat_session::TABLE,
        chat_event::TABLE,
    )
}

/// The event insert (`chat.py:162-169`): every concrete column in
/// model field order (the `BigAutoField` pk takes `DEFAULT` and
/// comes back via `RETURNING`). `seq` is [`next_event_seq_sql`] + 1
/// under the session row lock; `created_at` is `auto_now_add`.
/// After a successful insert the caller queues
/// [`ChatEffect::PublishEvent`] with the returned id.
pub fn insert_event_sql(
    session_id: Uuid,
    seq: i32,
    inputs: &NewEventInputs,
    created_at: DateTime<Utc>,
) -> String {
    let message_id = match inputs.message_id {
        Some(id) => lit_uuid(id),
        None => "NULL".to_owned(),
    };
    let payload = format!("{}::jsonb", lit_str(&dumps_value(&inputs.payload)));
    format!(
        "INSERT INTO \"{}\" (\"session_id\", \"message_id\", \"seq\", \"source_key\", \"kind\", \"payload\", \"created_at\") VALUES ({}, {}, {}, {}, {}, {}, {}) RETURNING \"{}\".\"id\"",
        chat_event::TABLE,
        lit_uuid(session_id),
        message_id,
        seq,
        lit_str(&inputs.source_key),
        lit_str(&inputs.kind),
        payload,
        lit_tstz(created_at),
        chat_event::TABLE,
    )
}

// ---------------------------------------------------------------------------
// `record_dedupe` (`chat.py:180-190`)
// ---------------------------------------------------------------------------

/// The dedupe insert's `message_id`: an empty key inserts nothing
/// (`None`, the caller returns `true`), otherwise `key[:128]`
/// (`chat.py:182-187`).
pub fn record_dedupe_message_id(key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    Some(truncate_chars(key, chat_dedupe::MESSAGE_ID_MAX_LENGTH).to_owned())
}

/// The dedupe insert (`chat.py:184-187`). The caller maps a unique
/// violation on `chat_dedupe_unique` to `false`, and must hold a
/// savepoint: a bare duplicate aborts the Postgres transaction
/// (QUIRK-DEDUP-SAVEPOINT).
pub fn insert_dedupe_sql(session_id: Uuid, message_id: &str, created_at: DateTime<Utc>) -> String {
    format!(
        "INSERT INTO \"{}\" (\"session_id\", \"message_id\", \"created_at\") VALUES ({}, {}, {}) RETURNING \"{}\".\"id\"",
        chat_dedupe::TABLE,
        lit_uuid(session_id),
        lit_str(message_id),
        lit_tstz(created_at),
        chat_dedupe::TABLE,
    )
}

// ---------------------------------------------------------------------------
// Enqueue payloads (`chat.py:193-253`)
// ---------------------------------------------------------------------------

/// The `chat_user_message` frame (`chat.py:209-219`), fields in Python
/// dict order so the serialized bytes keep `type` first (the
/// `admission.rs` struct-not-map precedent). The option fields carry
/// `local_thread_id or None` and friends (`:215-218`); `content` is
/// never coerced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatUserMessagePayload {
    /// Always `"chat_user_message"`.
    #[serde(rename = "type")]
    pub message_type: String,
    /// `str(chat_session_id)`, hyphenated.
    pub chat_session_id: String,
    /// `str(message_id)`, hyphenated.
    pub message_id: String,
    /// Verbatim, even when empty.
    pub content: String,
    /// Verbatim.
    pub content_parts: Value,
    /// `local_thread_id or None`.
    pub local_thread_id: Option<String>,
    /// `local_session_id or None`.
    pub local_session_id: Option<String>,
    /// `cwd or None`.
    pub cwd: Option<String>,
    /// `model or None`.
    pub model: Option<String>,
}

impl ChatUserMessagePayload {
    /// Build the frame, applying the `or None` coercions.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chat_session_id: Uuid,
        message_id: Uuid,
        content: String,
        content_parts: Value,
        local_thread_id: &str,
        local_session_id: &str,
        cwd: &str,
        model: &str,
    ) -> Self {
        Self {
            message_type: "chat_user_message".to_owned(),
            chat_session_id: chat_session_id.to_string(),
            message_id: message_id.to_string(),
            content,
            content_parts,
            local_thread_id: or_none(local_thread_id),
            local_session_id: or_none(local_session_id),
            cwd: or_none(cwd),
            model: or_none(model),
        }
    }
}

/// The `chat_warm` frame (`chat.py:240-247`), same conventions as
/// [`ChatUserMessagePayload`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatWarmPayload {
    /// Always `"chat_warm"`.
    #[serde(rename = "type")]
    pub message_type: String,
    /// `str(chat_session_id)`, hyphenated.
    pub chat_session_id: String,
    /// `local_thread_id or None`.
    pub local_thread_id: Option<String>,
    /// `local_session_id or None`.
    pub local_session_id: Option<String>,
    /// `cwd or None`.
    pub cwd: Option<String>,
    /// `model or None`.
    pub model: Option<String>,
}

impl ChatWarmPayload {
    /// Build the frame, applying the `or None` coercions.
    pub fn new(
        chat_session_id: Uuid,
        local_thread_id: &str,
        local_session_id: &str,
        cwd: &str,
        model: &str,
    ) -> Self {
        Self {
            message_type: "chat_warm".to_owned(),
            chat_session_id: chat_session_id.to_string(),
            local_thread_id: or_none(local_thread_id),
            local_session_id: or_none(local_session_id),
            cwd: or_none(cwd),
            model: or_none(model),
        }
    }
}

/// Parameters of `enqueue_chat_message_after_commit` (`chat.py:193-204`).
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueueMessageParams {
    /// `chat_session_id`.
    pub chat_session_id: Uuid,
    /// `message_id`.
    pub message_id: Uuid,
    /// Verbatim.
    pub content: String,
    /// Verbatim.
    pub content_parts: Value,
    /// Coerced with `or None`.
    pub local_thread_id: String,
    /// Coerced with `or None`.
    pub local_session_id: String,
    /// Coerced with `or None`.
    pub cwd: String,
    /// Coerced with `or None`.
    pub model: String,
}

/// `enqueue_chat_message_after_commit` (`chat.py:193-224`): queue the
/// send effect. The executor delivers it after commit via D-14
/// `send_to_runner` and runs `mark_message_dispatch_failed` on error.
pub fn enqueue_chat_message_after_commit(
    runner_id: Uuid,
    params: EnqueueMessageParams,
    effects: &mut dyn FnMut(ChatEffect),
) {
    effects(ChatEffect::SendChatMessage {
        runner_id,
        payload: ChatUserMessagePayload::new(
            params.chat_session_id,
            params.message_id,
            params.content,
            params.content_parts,
            &params.local_thread_id,
            &params.local_session_id,
            &params.cwd,
            &params.model,
        ),
    });
}

/// Parameters of `enqueue_chat_warm_after_commit` (`chat.py:227-234`).
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueueWarmParams {
    /// `chat_session_id`.
    pub chat_session_id: Uuid,
    /// Coerced with `or None`.
    pub local_thread_id: String,
    /// Coerced with `or None`.
    pub local_session_id: String,
    /// Coerced with `or None`.
    pub cwd: String,
    /// Coerced with `or None`.
    pub model: String,
}

/// `enqueue_chat_warm_after_commit` (`chat.py:227-253`): queue the
/// send effect. The executor delivers it after commit via D-14
/// `send_to_runner`; on error it logs and runs
/// `mark_warm_dispatch_failed`.
pub fn enqueue_chat_warm_after_commit(
    runner_id: Uuid,
    params: EnqueueWarmParams,
    effects: &mut dyn FnMut(ChatEffect),
) {
    effects(ChatEffect::SendChatWarm {
        runner_id,
        payload: ChatWarmPayload::new(
            params.chat_session_id,
            &params.local_thread_id,
            &params.local_session_id,
            &params.cwd,
            &params.model,
        ),
    });
}

// ---------------------------------------------------------------------------
// Session row locks
// ---------------------------------------------------------------------------

/// Lock a session row by pk (`mark_message_dispatch_failed`,
/// `chat.py:274`): full row, `FOR UPDATE`, first match.
pub fn session_lock_by_pk_sql(session_id: Uuid) -> String {
    format!(
        "SELECT {} FROM \"{}\" WHERE \"{}\".\"id\" = {} ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        quoted_columns(chat_session::TABLE, chat_session::COLUMNS),
        chat_session::TABLE,
        chat_session::TABLE,
        lit_uuid(session_id),
        chat_session::TABLE,
        chat_session::TABLE,
    )
}

/// Lock an OPEN session row by pk (`mark_warm_dispatch_failed` `:259`,
/// sweep rows `:416`, release rows `:462`): the pk lock plus the
/// `status = 'open'` guard.
pub fn session_lock_open_sql(session_id: Uuid) -> String {
    format!(
        "SELECT {} FROM \"{}\" WHERE (\"{}\".\"id\" = {} AND \"{}\".\"status\" = '{}') ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        quoted_columns(chat_session::TABLE, chat_session::COLUMNS),
        chat_session::TABLE,
        chat_session::TABLE,
        lit_uuid(session_id),
        chat_session::TABLE,
        AgentChatSessionStatus::Open.value(),
        chat_session::TABLE,
        chat_session::TABLE,
    )
}

// ---------------------------------------------------------------------------
// `mark_warm_dispatch_failed` (`chat.py:256-270`)
// ---------------------------------------------------------------------------

/// Plan `mark_warm_dispatch_failed` over the locked row: `None`
/// (missing or not OPEN) is a no-op (`chat.py:263-264`); otherwise
/// one `chat_warm_failed` event with `{"code": "dispatch_failed"}`
/// (`:265-269`). The caller inserts the event and queues
/// [`ChatEffect::PublishEvent`].
pub fn plan_mark_warm_dispatch_failed(locked: Option<&AgentChatSession>) -> Option<NewEventInputs> {
    locked?;
    Some(append_event_inputs(
        "chat_warm_failed",
        Some(&serde_json::json!({"code": "dispatch_failed"})),
        None,
        "",
    ))
}

// ---------------------------------------------------------------------------
// `create_assistant_message_locked` (`chat.py:305-319`)
// ---------------------------------------------------------------------------

/// Inputs for `create_assistant_message_locked`: `local_turn_id[:128]`,
/// `local_item_id[:128]`, default status `STREAMING` (`chat.py:308-317`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewAssistantInputs {
    /// Truncated to [`chat_message::LOCAL_TURN_ID_MAX_LENGTH`].
    pub local_turn_id: String,
    /// Truncated to [`chat_message::LOCAL_ITEM_ID_MAX_LENGTH`].
    pub local_item_id: String,
    /// `STREAMING` unless the caller passes another status.
    pub status: AgentChatMessageStatus,
}

/// Build the assistant-message inputs (`chat.py:312-319`). The role is
/// always `ASSISTANT`; `seq` is [`next_message_seq_sql`] + 1.
pub fn create_assistant_inputs(
    local_turn_id: &str,
    local_item_id: &str,
    status: AgentChatMessageStatus,
) -> NewAssistantInputs {
    NewAssistantInputs {
        local_turn_id: truncate_chars(local_turn_id, chat_message::LOCAL_TURN_ID_MAX_LENGTH)
            .to_owned(),
        local_item_id: truncate_chars(local_item_id, chat_message::LOCAL_ITEM_ID_MAX_LENGTH)
            .to_owned(),
        status,
    }
}

/// The chat-message insert (`chat.py:312-319`): every concrete column
/// in model field order, including the client-side UUID pk (so no
/// `RETURNING`). `created_at` is `auto_now_add`; `content` /
/// `content_parts` take their field defaults when the call site does
/// not set them (`""` / `[]`).
#[allow(clippy::too_many_arguments)]
pub fn insert_message_sql(
    message_id: Uuid,
    session_id: Uuid,
    role: &AgentChatMessageRole,
    content: &str,
    content_parts: &Value,
    status: &AgentChatMessageStatus,
    local_item_id: &str,
    local_turn_id: &str,
    seq: i32,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
) -> String {
    let completed_at = match completed_at {
        Some(stamp) => lit_tstz(stamp),
        None => "NULL".to_owned(),
    };
    let content_parts = format!("{}::jsonb", lit_str(&dumps_value(content_parts)));
    format!(
        "INSERT INTO \"{}\" (\"id\", \"session_id\", \"role\", \"content\", \"content_parts\", \"status\", \"local_item_id\", \"local_turn_id\", \"seq\", \"created_at\", \"completed_at\") VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
        chat_message::TABLE,
        lit_uuid(message_id),
        lit_uuid(session_id),
        lit_str(role.value()),
        lit_str(content),
        content_parts,
        lit_str(status.value()),
        lit_str(local_item_id),
        lit_str(local_turn_id),
        seq,
        lit_tstz(created_at),
        completed_at,
    )
}

// ---------------------------------------------------------------------------
// `active_assistant_message_locked` (`chat.py:322-332`)
// ---------------------------------------------------------------------------

/// The turn-scoped assistant probe (`chat.py:329`): STREAMING
/// assistants of the session on this `local_turn_id`, newest first.
/// Django renders the one-`filter()` conditions alphabetically
/// (`role`, `session_id`, `status` — verified live, independent of
/// the Python kwarg order); the chained `.filter(local_turn_id)`
/// appends last.
pub fn assistant_turn_scoped_sql(session_id: Uuid, local_turn_id: &str) -> String {
    format!(
        "SELECT {} FROM \"{}\" WHERE (\"{}\".\"role\" = '{}' AND \"{}\".\"session_id\" = {} AND \"{}\".\"status\" = '{}' AND \"{}\".\"local_turn_id\" = {}) ORDER BY \"{}\".\"created_at\" DESC LIMIT 1",
        quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
        chat_message::TABLE,
        chat_message::TABLE,
        AgentChatMessageRole::Assistant.value(),
        chat_message::TABLE,
        lit_uuid(session_id),
        chat_message::TABLE,
        AgentChatMessageStatus::Streaming.value(),
        chat_message::TABLE,
        lit_str(local_turn_id),
        chat_message::TABLE,
    )
}

/// The unscoped fallback probe (`chat.py:332`): newest STREAMING
/// assistant of the session, whatever its turn. Same alphabetical
/// condition order as [`assistant_turn_scoped_sql`].
pub fn assistant_fallback_sql(session_id: Uuid) -> String {
    format!(
        "SELECT {} FROM \"{}\" WHERE (\"{}\".\"role\" = '{}' AND \"{}\".\"session_id\" = {} AND \"{}\".\"status\" = '{}') ORDER BY \"{}\".\"created_at\" DESC LIMIT 1",
        quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
        chat_message::TABLE,
        chat_message::TABLE,
        AgentChatMessageRole::Assistant.value(),
        chat_message::TABLE,
        lit_uuid(session_id),
        chat_message::TABLE,
        AgentChatMessageStatus::Streaming.value(),
        chat_message::TABLE,
    )
}

/// `active_assistant_message_locked` (`chat.py:322-332`): the
/// turn-scoped hit wins; otherwise the newest STREAMING assistant.
/// The fallback stays lazy — it runs only when the scoped probe
/// missed or the session has no active turn — so a scoped hit costs
/// one query, exactly as in Python.
pub fn select_active_assistant<'a>(
    active_turn_id: &str,
    turn_scoped: Option<&'a AgentChatMessage>,
    fallback: impl FnOnce() -> Option<&'a AgentChatMessage>,
) -> Option<&'a AgentChatMessage> {
    if !active_turn_id.is_empty() {
        if let Some(hit) = turn_scoped {
            return Some(hit);
        }
        return fallback();
    }
    fallback()
}

// ---------------------------------------------------------------------------
// `finalize_active_messages_locked` (`chat.py:335-356`)
// ---------------------------------------------------------------------------

/// A message finalized to a terminal status: `status` plus
/// `completed_at`, saved together (`chat.py:347-349,353-355`).
#[derive(Debug, Clone, PartialEq)]
pub struct MessageStatusUpdate {
    /// The message pk.
    pub id: Uuid,
    /// The final status.
    pub status: AgentChatMessageStatus,
    /// `completed_at`: the flow's timestamp.
    pub completed_at: DateTime<Utc>,
}

/// Resolve the explicit finalize target: `message_id or
/// session.active_message_id` (`chat.py:343`). `None` means no
/// explicit lookup runs at all.
pub fn finalize_target_message_id(
    message_id_override: Option<Uuid>,
    session_active_message_id: Option<Uuid>,
) -> Option<Uuid> {
    message_id_override.or(session_active_message_id)
}

/// The explicit lookup (`chat.py:345`): the target message scoped to
/// the session (a cross-session id silently matches nothing —
/// FX-RUN-06 `dispatch_failed_rule`). Like
/// [`append_dedupe_check_sql`], the FK half of the default ordering
/// joins the session table for its `-last_message_at`,
/// `-created_at`.
pub fn finalize_explicit_lookup_sql(session_id: Uuid, message_id: Uuid) -> String {
    format!(
        "SELECT {} FROM \"{}\" INNER JOIN \"{}\" ON (\"{}\".\"session_id\" = \"{}\".\"id\") WHERE (\"{}\".\"id\" = {} AND \"{}\".\"session_id\" = {}) ORDER BY \"{}\".\"last_message_at\" DESC, \"{}\".\"created_at\" DESC, \"{}\".\"seq\" ASC LIMIT 1",
        quoted_columns(chat_message::TABLE, chat_message::COLUMNS),
        chat_message::TABLE,
        chat_session::TABLE,
        chat_message::TABLE,
        chat_session::TABLE,
        chat_message::TABLE,
        lit_uuid(message_id),
        chat_message::TABLE,
        lit_uuid(session_id),
        chat_session::TABLE,
        chat_session::TABLE,
        chat_message::TABLE,
    )
}

/// The outcome of [`plan_finalize_active_messages`].
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizePlan {
    /// Status updates: the explicit message (when found) first,
    /// then the streaming assistant (when found).
    pub updates: Vec<MessageStatusUpdate>,
    /// `assistant or message` (`chat.py:356`): the assistant wins
    /// when both finalized; `None` when neither did.
    pub returned_message_id: Option<Uuid>,
}

/// `finalize_active_messages_locked` (`chat.py:335-356`): the
/// explicit message and the streaming assistant both move to
/// `final_status` with `completed_at` set. `now` is the flow's
/// single timestamp (Python's one `timezone.now()`, `:341`).
pub fn plan_finalize_active_messages(
    explicit_message: Option<&AgentChatMessage>,
    assistant: Option<&AgentChatMessage>,
    final_status: AgentChatMessageStatus,
    now: DateTime<Utc>,
) -> FinalizePlan {
    let mut updates = Vec::with_capacity(2);
    if let Some(message) = explicit_message {
        updates.push(MessageStatusUpdate {
            id: message.id,
            status: final_status,
            completed_at: now,
        });
    }
    if let Some(assistant) = assistant {
        updates.push(MessageStatusUpdate {
            id: assistant.id,
            status: final_status,
            completed_at: now,
        });
    }
    let returned_message_id = assistant
        .map(|message| message.id)
        .or_else(|| explicit_message.map(|message| message.id));
    FinalizePlan {
        updates,
        returned_message_id,
    }
}

/// The message status write shared by `finalize` (`save`,
/// `chat.py:349,355`) and `complete_active_turn_locked` (queryset
/// `update`, `:368-371`): `SET status, completed_at WHERE pk`. The
/// `WHERE` is pk-only in both paths — the session scoping, if any,
/// happened at fetch time (BUG-COMPLETE-UNSCOPED for the `complete`
/// path, which never scopes).
pub fn message_status_update_sql(update: &MessageStatusUpdate) -> String {
    format!(
        "UPDATE \"{}\" SET \"status\" = {}, \"completed_at\" = {} WHERE \"{}\".\"id\" = {}",
        chat_message::TABLE,
        lit_str(update.status.value()),
        lit_tstz(update.completed_at),
        chat_message::TABLE,
        lit_uuid(update.id),
    )
}

/// Alias naming the `complete_active_turn_locked` call site
/// (`chat.py:368-371`): same statement as
/// [`message_status_update_sql`], deliberately unscoped by session
/// (BUG-COMPLETE-UNSCOPED).
pub fn complete_message_update_sql(update: &MessageStatusUpdate) -> String {
    message_status_update_sql(update)
}

// ---------------------------------------------------------------------------
// Fail-active-turn core (`mark_message_dispatch_failed`, sweep and
// release rows)
// ---------------------------------------------------------------------------

/// When the flow queues the drain effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainRule {
    /// `mark_...` (`chat.py:301`) and sweep rows (`:445`): only
    /// when the session had an active turn.
    IfWasActive,
    /// Release rows (`chat.py:478`): unconditionally (idle rows are
    /// skipped before planning, [`release_row_applies`]).
    Always,
}

/// Inputs varying between the three fail-active-turn flows.
#[derive(Debug, Clone, PartialEq)]
pub struct FailTurnParams {
    /// Explicit finalize target (`mark_...` passes its `message_id`,
    /// `chat.py:278`; sweep/release pass `None`).
    pub message_id_override: Option<Uuid>,
    /// The raw detail text; the planner stores `error[:2000]`
    /// (`:281,425,470`).
    pub error: String,
    /// The `chat_failed` event code: `dispatch_failed`,
    /// `active_turn_timeout` or `runner_session_reopened`.
    pub event_code: &'static str,
    /// The `chat_failed` payload `detail`: copied FULL, never
    /// truncated, when present (`:296,475` — QUIRK-ERROR-TRUNCATION).
    /// `None` omits the key (sweep rows, `:440`).
    pub event_detail: Option<String>,
    /// Whether `close_requested` closes the session (`mark_...`
    /// `:288-291`, sweep `:432-435`; release never closes).
    pub apply_close: bool,
    /// When to queue the drain effect.
    pub drain: DrainRule,
}

/// The planned fail-active-turn write set.
#[derive(Debug, Clone, PartialEq)]
pub struct FailTurnPlan {
    /// Message finalizations (explicit, then assistant).
    pub finalize: FinalizePlan,
    /// The stored error: `error[:2000]`, code points.
    pub session_error: String,
    /// Whether the session closes (`close_requested` + `apply_close`).
    pub close_session: bool,
    /// `closed_at` (the second clock read, only when closing).
    pub closed_at: Option<DateTime<Utc>>,
    /// `chat_failed`, plus `chat_closed` when closing.
    pub events: Vec<NewEventInputs>,
    /// Whether to queue [`ChatEffect::DrainTasks`].
    pub queue_drain: bool,
}

/// The session `save(update_fields)` columns shared by the three
/// fail flows (`chat.py:282-287,426-431,471`).
pub const FAIL_SESSION_UPDATE_FIELDS: [&str; 4] =
    ["active_message_id", "active_turn_id", "error", "updated_at"];
/// The extra columns when the fail flow closes the session
/// (`:291,435`).
pub const FAIL_SESSION_CLOSE_FIELDS: [&str; 2] = ["status", "closed_at"];

/// `was_active`: `bool(session.active_message_id or
/// session.active_turn_id)` (`chat.py:277,421`).
pub fn turn_was_active(session: &AgentChatSession) -> bool {
    session.active_message_id.is_some() || !session.active_turn_id.is_empty()
}

/// `mark_message_dispatch_failed` (`chat.py:272-302`), one
/// `sweep_active_turns` row (`:411-447`) and one
/// `release_active_chats_for_runner` row (`:458-479`): finalize to
/// FAILED, clear the active turn, store the error, emit
/// `chat_failed` (plus `chat_closed` when closing), maybe drain.
/// The clock mirrors Python's reads: once for the finalize stamps,
/// again for `closed_at` only when the session closes.
pub fn plan_fail_active_turn(
    session: &AgentChatSession,
    explicit_message: Option<&AgentChatMessage>,
    assistant: Option<&AgentChatMessage>,
    params: FailTurnParams,
    clock: &mut impl FnMut() -> DateTime<Utc>,
) -> FailTurnPlan {
    let was_active = turn_was_active(session);
    let finalize = plan_finalize_active_messages(
        explicit_message,
        assistant,
        AgentChatMessageStatus::Failed,
        clock(),
    );
    let close_session = params.apply_close && session.close_requested;
    let closed_at = if close_session { Some(clock()) } else { None };
    let mut payload = serde_json::Map::with_capacity(2);
    payload.insert(
        "code".to_owned(),
        Value::String(params.event_code.to_owned()),
    );
    if let Some(detail) = params.event_detail {
        payload.insert("detail".to_owned(), Value::String(detail));
    }
    let mut events = Vec::with_capacity(2);
    events.push(append_event_inputs(
        "chat_failed",
        Some(&Value::Object(payload)),
        finalize.returned_message_id,
        "",
    ));
    if close_session {
        events.push(append_event_inputs(
            "chat_closed",
            Some(&serde_json::json!({"reason": "close_requested"})),
            None,
            "",
        ));
    }
    let queue_drain = match params.drain {
        DrainRule::IfWasActive => was_active,
        DrainRule::Always => true,
    };
    FailTurnPlan {
        finalize,
        session_error: truncate_chars(&params.error, 2000).to_owned(),
        close_session,
        closed_at,
        events,
        queue_drain,
    }
}

/// `plan_fail_active_turn` as `mark_message_dispatch_failed`
/// (`chat.py:272-302`): explicit `message_id`, `error=detail[:2000]`,
/// `chat_failed{"dispatch_failed", FULL detail}`, close on
/// `close_requested`, drain iff was active.
pub fn plan_mark_message_dispatch_failed(
    session: &AgentChatSession,
    explicit_message: Option<&AgentChatMessage>,
    assistant: Option<&AgentChatMessage>,
    message_id: Option<Uuid>,
    detail: &str,
    clock: &mut impl FnMut() -> DateTime<Utc>,
) -> FailTurnPlan {
    plan_fail_active_turn(
        session,
        explicit_message,
        assistant,
        FailTurnParams {
            message_id_override: message_id,
            error: detail.to_owned(),
            event_code: "dispatch_failed",
            event_detail: Some(detail.to_owned()),
            apply_close: true,
            drain: DrainRule::IfWasActive,
        },
        clock,
    )
}

/// `plan_fail_active_turn` as one `sweep_active_turns` row
/// (`chat.py:411-447`): no override, `error="active chat turn timed
/// out"`, `chat_failed{"active_turn_timeout"}` with NO detail key,
/// close on `close_requested`, drain iff was active.
pub fn plan_sweep_active_row(
    session: &AgentChatSession,
    explicit_message: Option<&AgentChatMessage>,
    assistant: Option<&AgentChatMessage>,
    clock: &mut impl FnMut() -> DateTime<Utc>,
) -> FailTurnPlan {
    plan_fail_active_turn(
        session,
        explicit_message,
        assistant,
        FailTurnParams {
            message_id_override: None,
            error: "active chat turn timed out".to_owned(),
            event_code: "active_turn_timeout",
            event_detail: None,
            apply_close: true,
            drain: DrainRule::IfWasActive,
        },
        clock,
    )
}

/// `plan_fail_active_turn` as one `release_active_chats_for_runner`
/// row (`chat.py:458-479`): no override, `error=detail[:2000]`,
/// `chat_failed{"runner_session_reopened", FULL detail}`, never
/// closes, always drains. The caller skips idle rows first
/// ([`release_row_applies`]).
pub fn plan_release_row(
    session: &AgentChatSession,
    explicit_message: Option<&AgentChatMessage>,
    assistant: Option<&AgentChatMessage>,
    detail: &str,
    clock: &mut impl FnMut() -> DateTime<Utc>,
) -> FailTurnPlan {
    plan_fail_active_turn(
        session,
        explicit_message,
        assistant,
        FailTurnParams {
            message_id_override: None,
            error: detail.to_owned(),
            event_code: "runner_session_reopened",
            event_detail: Some(detail.to_owned()),
            apply_close: false,
            drain: DrainRule::Always,
        },
        clock,
    )
}

/// The release-row skip (`chat.py:465`): idle rows are left
/// untouched and uncounted.
pub fn release_row_applies(session: &AgentChatSession) -> bool {
    turn_was_active(session)
}

/// The fail-flow session write (`save(update_fields)`,
/// `chat.py:292,436,471`): clears the active turn, stores the
/// planned error, closes when planned. `updated_at` is `auto_now`.
/// `save()` renders `SET` in model field order, not `update_fields`
/// order (verified live): `active_turn_id` before
/// `active_message_id`, and `status`/`closed_at` slotted by their
/// field positions when closing.
pub fn fail_session_update_sql(
    session_id: Uuid,
    plan: &FailTurnPlan,
    updated_at: DateTime<Utc>,
) -> String {
    let mut sets = format!(
        "\"active_turn_id\" = '', \"active_message_id\" = NULL, \"error\" = {}, \"updated_at\" = {}",
        lit_str(&plan.session_error),
        lit_tstz(updated_at),
    );
    if plan.close_session {
        let closed_at = plan.closed_at.expect("closed_at when closing");
        sets = format!(
            "\"status\" = '{}', \"active_turn_id\" = '', \"active_message_id\" = NULL, \"closed_at\" = {}, \"error\" = {}, \"updated_at\" = {}",
            AgentChatSessionStatus::Closed.value(),
            lit_tstz(closed_at),
            lit_str(&plan.session_error),
            lit_tstz(updated_at),
        );
    }
    format!(
        "UPDATE \"{}\" SET {} WHERE \"{}\".\"id\" = {}",
        chat_session::TABLE,
        sets,
        chat_session::TABLE,
        lit_uuid(session_id),
    )
}

// ---------------------------------------------------------------------------
// `complete_active_turn_locked` (`chat.py:359-400`)
// ---------------------------------------------------------------------------

/// The session `save(update_fields)` columns for the complete flow
/// (`chat.py:382-391`): always all six, even when the session stays
/// open.
pub const COMPLETE_SESSION_UPDATE_FIELDS: [&str; 6] = [
    "active_message_id",
    "active_turn_id",
    "last_message_at",
    "status",
    "closed_at",
    "updated_at",
];

/// The planned complete-turn write set.
#[derive(Debug, Clone, PartialEq)]
pub struct CompleteTurnPlan {
    /// The active-message status write (`None` when the session has
    /// no active message). Unscoped by session
    /// (BUG-COMPLETE-UNSCOPED).
    pub message_update: Option<MessageStatusUpdate>,
    /// `last_message_at`: the flow's single timestamp.
    pub last_message_at: DateTime<Utc>,
    /// Whether the session closes: `close_requested` with a final
    /// status in `{completed, cancelled, failed}` (`chat.py:375-379`).
    pub close_session: bool,
    /// `closed_at` (same single timestamp, only when closing).
    pub closed_at: Option<DateTime<Utc>>,
    /// `turn_completed`, plus `chat_closed` when closing.
    pub events: Vec<NewEventInputs>,
    /// Whether to queue [`ChatEffect::DrainTasks`] (`was_active`).
    pub queue_drain: bool,
}

/// The `turn_completed` payload: `payload or {"status": final_status}`
/// (`chat.py:392-396`).
pub fn complete_event_payload(
    payload: Option<&Value>,
    final_status: &AgentChatMessageStatus,
) -> Value {
    match payload {
        Some(value) if !is_python_falsy(value) => value.clone(),
        _ => serde_json::json!({"status": final_status.value()}),
    }
}

/// `complete_active_turn_locked` (`chat.py:359-400`): the active
/// message moves to `final_status`, the turn clears, `turn_completed`
/// fires (plus `chat_closed` when `close_requested` closes the
/// session), and the drain queues iff the turn was active. Python
/// reads the clock ONCE (`:365`); the single `now` stamps the
/// message, `last_message_at` and `closed_at` alike.
pub fn plan_complete_active_turn(
    session: &AgentChatSession,
    final_status: AgentChatMessageStatus,
    payload: Option<&Value>,
    now: DateTime<Utc>,
) -> CompleteTurnPlan {
    let was_active = turn_was_active(session);
    let message_update = session.active_message_id.map(|id| MessageStatusUpdate {
        id,
        status: final_status,
        completed_at: now,
    });
    let close_session = session.close_requested
        && matches!(
            final_status,
            AgentChatMessageStatus::Completed
                | AgentChatMessageStatus::Cancelled
                | AgentChatMessageStatus::Failed
        );
    let mut events = Vec::with_capacity(2);
    let turn_payload = complete_event_payload(payload, &final_status);
    events.push(append_event_inputs(
        "turn_completed",
        Some(&turn_payload),
        None,
        "",
    ));
    if close_session {
        events.push(append_event_inputs(
            "chat_closed",
            Some(&serde_json::json!({"reason": "close_requested"})),
            None,
            "",
        ));
    }
    CompleteTurnPlan {
        message_update,
        last_message_at: now,
        close_session,
        closed_at: if close_session { Some(now) } else { None },
        events,
        queue_drain: was_active,
    }
}

/// The complete-flow session write (`chat.py:382-391`): clears the
/// active turn, stamps `last_message_at`, closes when planned.
/// `status`/`closed_at` are written unconditionally (Python's six
/// `update_fields`); `updated_at` is `auto_now`. Like
/// [`fail_session_update_sql`], `SET` follows model field order
/// (verified live).
pub fn complete_session_update_sql(
    session: &AgentChatSession,
    plan: &CompleteTurnPlan,
    updated_at: DateTime<Utc>,
) -> String {
    let status = if plan.close_session {
        AgentChatSessionStatus::Closed.value()
    } else {
        session.status.value()
    };
    let closed_at = match plan.closed_at {
        Some(stamp) => lit_tstz(stamp),
        None => match session.closed_at {
            Some(stamp) => lit_tstz(stamp),
            None => "NULL".to_owned(),
        },
    };
    format!(
        "UPDATE \"{}\" SET \"status\" = '{}', \"active_turn_id\" = '', \"active_message_id\" = NULL, \"last_message_at\" = {}, \"closed_at\" = {}, \"updated_at\" = {} WHERE \"{}\".\"id\" = {}",
        chat_session::TABLE,
        status,
        lit_tstz(plan.last_message_at),
        closed_at,
        lit_tstz(updated_at),
        chat_session::TABLE,
        lit_uuid(session.id),
    )
}

// ---------------------------------------------------------------------------
// Sweeps (`chat.py:403-497`)
// ---------------------------------------------------------------------------

/// The staleness cutoff for [`sweep_active_ids_sql`]:
/// `now - CHAT_ACTIVE_TIMEOUT_SECS` (`chat.py:404`). Takes `now` as a
/// parameter (the caller reads the clock); the L1 constant is the
/// 1800s span.
pub fn sweep_active_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - chrono::Duration::seconds(CHAT_ACTIVE_TIMEOUT_SECS)
}

/// `sweep_active_turns` id select (`chat.py:405-410`): OPEN sessions
/// with an active turn that are stale (`updated_at < cutoff`) or
/// whose runner is OFFLINE (hence the `INNER JOIN`).
pub fn sweep_active_ids_sql(cutoff: DateTime<Utc>) -> String {
    format!(
        "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" INNER JOIN \"{}\" ON (\"agent_chat_session\".\"runner_id\" = \"{}\".\"id\") WHERE (\"agent_chat_session\".\"status\" = '{}' AND ({}) AND (\"agent_chat_session\".\"updated_at\" < {} OR \"{}\".\"status\" = '{}')) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC",
        RUNNER_TABLE,
        RUNNER_TABLE,
        AgentChatSessionStatus::Open.value(),
        ACTIVE_TURN_PREDICATE_SQL,
        lit_tstz(cutoff),
        RUNNER_TABLE,
        RUNNER_STATUS_OFFLINE,
    )
}

/// `release_active_chats_for_runner` id select (`chat.py:452-456`):
/// the runner's OPEN sessions with an active turn (no staleness
/// filter).
pub fn release_ids_sql(runner_id: Uuid) -> String {
    format!(
        "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" WHERE (\"agent_chat_session\".\"runner_id\" = {} AND \"agent_chat_session\".\"status\" = '{}' AND ({})) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC",
        lit_uuid(runner_id),
        AgentChatSessionStatus::Open.value(),
        ACTIVE_TURN_PREDICATE_SQL,
    )
}

/// The emptiness cutoff for [`sweep_empty_ids_sql`]: `now - 24h`
/// (`chat.py:484`).
pub fn sweep_empty_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - chrono::Duration::hours(24)
}

/// `sweep_empty_sessions` id select (`chat.py:485-491`): OPEN
/// sessions created over 24h ago with zero messages (the `LEFT
/// OUTER JOIN` + `IS NULL` is `messages__isnull=True`). The
/// `ORDER BY` is pointless for an id list but ported
/// (QUIRK-EMPTY-ORDER).
pub fn sweep_empty_ids_sql(cutoff: DateTime<Utc>) -> String {
    format!(
        "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" LEFT OUTER JOIN \"agent_chat_message\" ON (\"agent_chat_session\".\"id\" = \"agent_chat_message\".\"session_id\") WHERE (\"agent_chat_session\".\"created_at\" < {} AND \"agent_chat_message\".\"id\" IS NULL AND \"agent_chat_session\".\"status\" = '{}') ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC",
        lit_tstz(cutoff),
        AgentChatSessionStatus::Open.value(),
    )
}

/// `sweep_empty_sessions` bulk close (`chat.py:494-497`): one
/// `UPDATE` over the collected ids; the rowcount is the return
/// value. The caller skips the call when `ids` is empty
/// (`chat.py:492-493`).
pub fn sweep_empty_close_sql(ids: &[Uuid], closed_at: DateTime<Utc>) -> String {
    let ids = ids
        .iter()
        .map(|id| lit_uuid(*id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"agent_chat_session\" SET \"status\" = '{}', \"closed_at\" = {} WHERE \"agent_chat_session\".\"id\" IN ({})",
        AgentChatSessionStatus::Closed.value(),
        lit_tstz(closed_at),
        ids,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-06-chat-service.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn uuid(text: &str) -> Uuid {
        text.parse().expect("uuid parses")
    }

    fn stamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 2, 22, 52, 2).unwrap()
    }

    fn test_session() -> AgentChatSession {
        AgentChatSession {
            id: uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
            workspace_id: uuid("11111111-1111-1111-1111-111111111111"),
            runner_id: uuid("cd871d43-1c13-4f30-a838-2495c1c4baac"),
            created_by_id: uuid("22222222-2222-2222-2222-222222222222"),
            pod_id: uuid("33333333-3333-3333-3333-333333333333"),
            status: AgentChatSessionStatus::Open,
            agent_kind: String::new(),
            local_thread_id: String::new(),
            local_session_id: String::new(),
            cwd: String::new(),
            model: String::new(),
            active_turn_id: String::new(),
            active_message_id: None,
            close_requested: false,
            last_message_at: None,
            closed_at: None,
            error: String::new(),
            created_at: stamp(),
            updated_at: stamp(),
        }
    }

    fn test_message(id: &str, session_id: Uuid) -> AgentChatMessage {
        AgentChatMessage {
            id: uuid(id),
            session_id,
            role: AgentChatMessageRole::Assistant,
            content: String::new(),
            content_parts: Value::Array(Vec::new()),
            status: AgentChatMessageStatus::Streaming,
            local_item_id: String::new(),
            local_turn_id: String::new(),
            seq: 1,
            created_at: stamp(),
            completed_at: None,
        }
    }

    // -- guards ---------------------------------------------------------

    #[test]
    fn read_guard_truth_table_and_short_circuit() {
        let user = uuid("22222222-2222-2222-2222-222222222222");
        let creator = user;
        let other = uuid("44444444-4444-4444-4444-444444444444");
        // Non-member: runner/admin never consulted.
        let calls = std::cell::Cell::new(0);
        let bump = || {
            calls.set(calls.get() + 1);
            true
        };
        assert!(!can_read_chat(
            user,
            creator,
            || false,
            bump,
            || {
                calls.set(calls.get() + 1);
                true
            }
        ));
        assert_eq!(calls.get(), 0);
        // Member without runner view: admin never consulted.
        let mut calls = 0;
        assert!(!can_read_chat(
            user,
            creator,
            || true,
            || false,
            || {
                calls += 1;
                true
            },
        ));
        assert_eq!(calls, 0);
        // Creator with view: admin never consulted.
        let mut calls = 0;
        assert!(can_read_chat(
            user,
            creator,
            || true,
            || true,
            || {
                calls += 1;
                false
            },
        ));
        assert_eq!(calls, 0);
        // Non-creator needs admin.
        assert!(can_read_chat(user, other, || true, || true, || true));
        assert!(!can_read_chat(user, other, || true, || true, || false));
    }

    #[test]
    fn send_guard_truth_table_and_short_circuit() {
        let user = uuid("22222222-2222-2222-2222-222222222222");
        let other = uuid("44444444-4444-4444-4444-444444444444");
        let mut calls = 0;
        assert!(!can_send_chat(
            user,
            user,
            || false,
            || {
                calls += 1;
                true
            }
        ));
        assert_eq!(calls, 0);
        // Non-creator never consults can_use_runner (creator check first).
        let mut calls = 0;
        assert!(!can_send_chat(
            user,
            other,
            || true,
            || {
                calls += 1;
                true
            }
        ));
        assert_eq!(calls, 0);
        assert!(can_send_chat(user, user, || true, || true));
        assert!(!can_send_chat(user, user, || true, || false));
    }

    #[test]
    fn decide_guard_matches_read_guard() {
        let user = uuid("22222222-2222-2222-2222-222222222222");
        for (created_by, member, view, admin, expected) in [
            (user, true, true, false, true),
            (user, true, false, true, false),
            (user, false, true, true, false),
        ] {
            assert_eq!(
                can_decide_chat_approval(user, created_by, || member, || view, || admin),
                expected,
            );
        }
        let other = uuid("44444444-4444-4444-4444-444444444444");
        assert!(can_decide_chat_approval(
            user,
            other,
            || true,
            || true,
            || true
        ));
        assert!(!can_decide_chat_approval(
            user,
            other,
            || true,
            || true,
            || false
        ));
    }

    // -- normalize_cwd --------------------------------------------------

    #[test]
    fn normalize_cwd_ignores_everything() {
        // FX-RUN-06 `normalize_cwd` vectors (Python reprs on the left).
        assert_eq!(normalize_cwd("/tmp"), "");
        assert_eq!(normalize_cwd(""), "");
        assert_eq!(normalize_cwd(None::<&str>), "");
        assert_eq!(normalize_cwd("/a/b"), "");
        assert_eq!(normalize_cwd(123), "");
        assert_eq!(normalize_cwd([("x", 1)]), "");
        for vector in fixture()["normalize_cwd"].as_array().expect("vectors") {
            assert_eq!(vector["out"].as_str(), Some(""));
        }
    }

    // -- seq allocators -------------------------------------------------

    #[test]
    fn next_seq_vectors_match_fixture() {
        let fx = &fixture()["seq_allocators"];
        assert_eq!(
            next_seq_after_max(None),
            fx["empty_message"].as_i64().unwrap() as i32
        );
        assert_eq!(
            next_seq_after_max(None),
            fx["empty_event"].as_i64().unwrap() as i32
        );
        assert_eq!(
            next_seq_after_max(Some(5)),
            fx["after_rows_message"].as_i64().unwrap() as i32
        );
        assert_eq!(
            next_seq_after_max(Some(2)),
            fx["after_rows_event"].as_i64().unwrap() as i32
        );
        // Gap not filled: deleting seq 5 leaves max 1, next is 2.
        assert_eq!(
            next_seq_after_max(Some(1)),
            fx["after_gap_message"].as_i64().unwrap() as i32
        );
    }

    #[test]
    fn seq_sql_shapes() {
        let session = uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d");
        assert_eq!(
            next_message_seq_sql(session),
            "SELECT MAX(\"agent_chat_message\".\"seq\") AS \"seq__max\" FROM \"agent_chat_message\" WHERE \"agent_chat_message\".\"session_id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid"
        );
        assert_eq!(
            next_event_seq_sql(session),
            "SELECT MAX(\"agent_chat_event\".\"seq\") AS \"seq__max\" FROM \"agent_chat_event\" WHERE \"agent_chat_event\".\"session_id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid"
        );
    }

    // -- serialize / dumps / publish ------------------------------------

    #[test]
    fn serialize_event_vector_is_byte_exact() {
        let vector = &fixture()["serialize_event"];
        let parts = EventParts {
            id: vector["id"].as_i64().expect("id"),
            session_id: vector["session"].as_str().expect("session"),
            message_id: vector["message"].as_str(),
            seq: vector["seq"].as_i64().expect("seq") as i32,
            kind: vector["kind"].as_str().expect("kind"),
            payload_json: "{}",
            created_at: vector["created_at"].as_str().expect("created_at"),
        };
        assert_eq!(
            serialize_event_json(&parts),
            "{\"id\": 46, \"session\": \"9fe1b18d-4602-48c7-9352-0bee23df1f9d\", \"message\": null, \"seq\": 2, \"kind\": \"k\", \"payload\": {}, \"created_at\": \"2026-10-02T22:52:02.740477+00:00\"}"
        );
    }

    #[test]
    fn dumps_string_matches_json_dumps() {
        // (input, `json.dumps` output), probed live for D-06; same rules.
        const STRINGS: &[(&str, &str)] = &[
            ("hi", "\"hi\""),
            ("a\"q\\b", "\"a\\\"q\\\\b\""),
            ("line\nbreak", "\"line\\nbreak\""),
            ("tab\there", "\"tab\\there\""),
            ("cr\rr", "\"cr\\rr\""),
            ("back\u{08}form\u{0c}f", "\"back\\bform\\ff\""),
            ("ctrl\u{01}\u{7f}", "\"ctrl\\u0001\\u007f\""),
            ("\u{e9}\u{2603}", "\"\\u00e9\\u2603\""),
            ("emoji\u{1f600}", "\"emoji\\ud83d\\ude00\""),
            ("sep\u{2028}end", "\"sep\\u2028end\""),
            ("sep\u{2029}end", "\"sep\\u2029end\""),
            ("quote\"and\u{27}", "\"quote\\\"and'\""),
            ("mix \u{e9}\n\0", "\"mix \\u00e9\\n\\u0000\""),
        ];
        for (input, expected) in STRINGS {
            assert_eq!(&dumps_string(input), expected, "{input:?}");
        }
    }

    #[test]
    fn dumps_value_shapes_match_json_dumps() {
        assert_eq!(dumps_value(&Value::Null), "null");
        assert_eq!(dumps_value(&serde_json::json!(true)), "true");
        assert_eq!(dumps_value(&serde_json::json!(7)), "7");
        assert_eq!(dumps_value(&serde_json::json!(1.5)), "1.5");
        assert_eq!(dumps_value(&serde_json::json!({"t": 1})), "{\"t\": 1}");
        assert_eq!(
            dumps_value(&serde_json::json!([{"t": 1}, "x"])),
            "[{\"t\": 1}, \"x\"]"
        );
        assert_eq!(dumps_value(&serde_json::json!("\u{e9}")), "\"\\u00e9\"");
        // Floats: Python spelling, verified against `repr`/`json.dumps`.
        for (value, expected) in [
            (1e-7, "1e-07"),
            (1e300, "1e+300"),
            (1.5, "1.5"),
            (0.1, "0.1"),
            (123456.0, "123456.0"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1e21, "1e+21"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (-2.5e-10, "-2.5e-10"),
            (0.0, "0.0"),
            (100.0, "100.0"),
        ] {
            let number = Value::Number(serde_json::Number::from_f64(value).expect("finite"));
            assert_eq!(dumps_value(&number), expected, "{value:?}");
        }
        assert_eq!(dumps_value(&serde_json::json!(-0.0)), "-0.0");
    }

    #[test]
    fn publish_frame_vector_matches_fixture() {
        let published = &fixture()["publish_event"]["published"][0];
        let body = &published["body"];
        let parts = EventParts {
            id: body["id"].as_i64().expect("id"),
            session_id: body["session"].as_str().expect("session"),
            message_id: body["message"].as_str(),
            seq: body["seq"].as_i64().expect("seq") as i32,
            kind: body["kind"].as_str().expect("kind"),
            payload_json: "{\"a\": 1}",
            created_at: body["created_at"].as_str().expect("created_at"),
        };
        let session = uuid(body["session"].as_str().expect("session"));
        let (channel, event_json) = publish_frame(session, &serialize_event_json(&parts));
        assert_eq!(channel, published["channel"].as_str().expect("channel"));
        assert_eq!(
            event_json,
            "{\"id\": 47, \"session\": \"9fe1b18d-4602-48c7-9352-0bee23df1f9d\", \"message\": null, \"seq\": 3, \"kind\": \"chat_started\", \"payload\": {\"a\": 1}, \"created_at\": \"2026-10-02T22:52:02.822934+00:00\"}"
        );
        // A missing client is a no-op in the executor (no frame queued).
        assert_eq!(
            fixture()["publish_event"]["none_client"].as_str(),
            Some("no-op, no publish")
        );
    }

    // -- append ---------------------------------------------------------

    #[test]
    fn append_inputs_truncate_and_default() {
        let fx = &fixture()["append_event_locked"];
        let inputs = append_event_inputs(
            "chat_started",
            Some(&serde_json::json!({"a": 1})),
            None,
            "sk-1",
        );
        assert_eq!(inputs.kind, fx["first"]["kind"].as_str().unwrap());
        assert_eq!(inputs.payload, fx["first"]["payload"].clone());
        assert_eq!(inputs.source_key, "sk-1");
        // Truncation bounds (char counts from the fixture).
        let long_key = "k".repeat(200);
        let long_kind = "k".repeat(100);
        let cut = append_event_inputs(&long_kind, None, None, &long_key);
        assert_eq!(
            cut.source_key.chars().count(),
            fx["truncated_source_key_len"].as_u64().unwrap() as usize
        );
        assert_eq!(
            cut.kind.chars().count(),
            fx["truncated_kind_len"].as_u64().unwrap() as usize
        );
        // Defaults: payload {} and no message.
        let defaulted = append_event_inputs("k", None, None, "");
        assert_eq!(
            defaulted.payload,
            fx["default_payload_message"]["payload"].clone()
        );
        assert_eq!(defaulted.message_id, None);
        // Char-wise cut, never mid-UTF-8.
        let wide = append_event_inputs(&"☃".repeat(70), None, None, "");
        assert_eq!(wide.kind.chars().count(), 64);
        // Only non-empty source keys dedupe.
        assert!(append_checks_dedupe("sk-1"));
        assert!(!append_checks_dedupe(""));
    }

    #[test]
    fn append_dedupe_check_sql_shape() {
        let sql = append_dedupe_check_sql(uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"), "sk-1");
        assert!(sql.starts_with(
            "SELECT \"agent_chat_event\".\"id\", \"agent_chat_event\".\"session_id\""
        ));
        assert!(sql.contains(
            "FROM \"agent_chat_event\" INNER JOIN \"agent_chat_session\" ON (\"agent_chat_event\".\"session_id\" = \"agent_chat_session\".\"id\") WHERE (\"agent_chat_event\".\"session_id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid AND \"agent_chat_event\".\"source_key\" = 'sk-1')"
        ));
        assert!(sql.ends_with(
            "ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_event\".\"seq\" ASC LIMIT 1"
        ));
    }

    #[test]
    fn insert_event_sql_shape() {
        let inputs = append_event_inputs(
            "chat_started",
            Some(&serde_json::json!({"a": 1})),
            None,
            "sk-1",
        );
        let created: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-02T22:52:02.822934+00:00")
                .unwrap()
                .with_timezone(&Utc);
        assert_eq!(
            insert_event_sql(
                uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
                3,
                &inputs,
                created,
            ),
            "INSERT INTO \"agent_chat_event\" (\"session_id\", \"message_id\", \"seq\", \"source_key\", \"kind\", \"payload\", \"created_at\") VALUES ('9fe1b18d460248c793520bee23df1f9d'::uuid, NULL, 3, 'sk-1', 'chat_started', '{\"a\": 1}'::jsonb, '2026-10-02 22:52:02.822934+00:00'::timestamptz) RETURNING \"agent_chat_event\".\"id\""
        );
    }

    // -- record_dedupe --------------------------------------------------

    #[test]
    fn record_dedupe_vectors_match_fixture() {
        let fx = &fixture()["record_dedupe"];
        // Empty key: no insert, returns true.
        assert_eq!(record_dedupe_message_id(""), None);
        assert!(fx["empty_key"].as_bool().unwrap());
        assert_eq!(fx["empty_created_rows"].as_u64().unwrap(), 0);
        // First insert / dup outcomes are the executor's (true / false
        // on unique violation).
        assert!(fx["first"].as_bool().unwrap());
        assert!(!fx["dup"].as_bool().unwrap());
        assert!(fx["long"].as_bool().unwrap());
        // key[:128].
        let stored = record_dedupe_message_id(&"m".repeat(200)).expect("some");
        assert_eq!(
            stored.chars().count(),
            fx["stored_len"].as_u64().unwrap() as usize
        );
        assert_eq!(record_dedupe_message_id("k"), Some("k".to_owned()));
    }

    #[test]
    fn insert_dedupe_sql_shape() {
        let created: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-02T22:52:02.822934+00:00")
                .unwrap()
                .with_timezone(&Utc);
        assert_eq!(
            insert_dedupe_sql(
                uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
                "k",
                created,
            ),
            "INSERT INTO \"chat_message_dedupe\" (\"session_id\", \"message_id\", \"created_at\") VALUES ('9fe1b18d460248c793520bee23df1f9d'::uuid, 'k', '2026-10-02 22:52:02.822934+00:00'::timestamptz) RETURNING \"chat_message_dedupe\".\"id\""
        );
    }

    // -- enqueue ---------------------------------------------------------

    #[test]
    fn enqueue_message_payload_matches_fixture() {
        let sent = &fixture()["enqueue"]["sent"][0];
        let payload = ChatUserMessagePayload::new(
            uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
            uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9"),
            "hi".to_owned(),
            serde_json::json!([{"t": 1}]),
            "",
            "ls",
            "",
            "",
        );
        assert_eq!(
            serde_json::to_value(&payload).expect("serializes"),
            sent["payload"].clone(),
        );
        // Key order pins the struct field order (Python dict order).
        assert_eq!(
            serde_json::to_string(&payload).expect("renders"),
            "{\"type\":\"chat_user_message\",\"chat_session_id\":\"9fe1b18d-4602-48c7-9352-0bee23df1f9d\",\"message_id\":\"9d469513-f80c-4f0c-ab20-e5827f95c3f9\",\"content\":\"hi\",\"content_parts\":[{\"t\":1}],\"local_thread_id\":null,\"local_session_id\":\"ls\",\"cwd\":null,\"model\":null}"
        );
        assert_eq!(
            sent["runner_id"].as_str(),
            Some("cd871d43-1c13-4f30-a838-2495c1c4baac")
        );
    }

    #[test]
    fn enqueue_warm_payload_matches_fixture() {
        let sent = &fixture()["enqueue"]["sent"][1];
        let payload = ChatWarmPayload::new(
            uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
            "lt",
            "",
            "/x",
            "m",
        );
        assert_eq!(
            serde_json::to_value(&payload).expect("serializes"),
            sent["payload"].clone(),
        );
        assert_eq!(
            serde_json::to_string(&payload).expect("renders"),
            "{\"type\":\"chat_warm\",\"chat_session_id\":\"9fe1b18d-4602-48c7-9352-0bee23df1f9d\",\"local_thread_id\":\"lt\",\"local_session_id\":null,\"cwd\":\"/x\",\"model\":\"m\"}"
        );
    }

    #[test]
    fn enqueue_registrars_queue_one_send_each() {
        let fx = &fixture()["enqueue"]["on_commit"];
        assert_eq!(fx.as_array().expect("names").len(), 2);
        let mut effects = Vec::new();
        let mut sink = |effect: ChatEffect| effects.push(effect);
        enqueue_chat_message_after_commit(
            uuid("cd871d43-1c13-4f30-a838-2495c1c4baac"),
            EnqueueMessageParams {
                chat_session_id: uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
                message_id: uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9"),
                content: "hi".to_owned(),
                content_parts: serde_json::json!([{"t": 1}]),
                local_thread_id: String::new(),
                local_session_id: "ls".to_owned(),
                cwd: String::new(),
                model: String::new(),
            },
            &mut sink,
        );
        enqueue_chat_warm_after_commit(
            uuid("cd871d43-1c13-4f30-a838-2495c1c4baac"),
            EnqueueWarmParams {
                chat_session_id: uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
                local_thread_id: "lt".to_owned(),
                local_session_id: String::new(),
                cwd: "/x".to_owned(),
                model: "m".to_owned(),
            },
            &mut sink,
        );
        assert_eq!(effects.len(), 2);
        assert!(matches!(
            &effects[0],
            ChatEffect::SendChatMessage { runner_id, payload }
            if *runner_id == uuid("cd871d43-1c13-4f30-a838-2495c1c4baac")
                && payload.message_type == "chat_user_message"
        ));
        assert!(matches!(
            &effects[1],
            ChatEffect::SendChatWarm { runner_id, payload }
            if *runner_id == uuid("cd871d43-1c13-4f30-a838-2495c1c4baac")
                && payload.message_type == "chat_warm"
        ));
    }

    // -- drain -----------------------------------------------------------

    #[test]
    fn drain_queues_only_with_a_runner() {
        let effects = std::cell::RefCell::new(Vec::new());
        let mut sink = |effect: ChatEffect| effects.borrow_mut().push(effect);
        drain_tasks_after_chat_release(
            None,
            Some(uuid("33333333-3333-3333-3333-333333333333")),
            &mut sink,
        );
        assert!(effects.borrow().is_empty());
        drain_tasks_after_chat_release(
            Some(uuid("cd871d43-1c13-4f30-a838-2495c1c4baac")),
            Some(uuid("33333333-3333-3333-3333-333333333333")),
            &mut sink,
        );
        drain_tasks_after_chat_release(
            Some(uuid("cd871d43-1c13-4f30-a838-2495c1c4baac")),
            None,
            &mut sink,
        );
        assert_eq!(
            *effects.borrow(),
            vec![
                ChatEffect::DrainTasks {
                    runner_id: uuid("cd871d43-1c13-4f30-a838-2495c1c4baac"),
                    pod_id: Some(uuid("33333333-3333-3333-3333-333333333333")),
                },
                ChatEffect::DrainTasks {
                    runner_id: uuid("cd871d43-1c13-4f30-a838-2495c1c4baac"),
                    pod_id: None,
                },
            ]
        );
    }

    // -- mark_warm --------------------------------------------------------

    #[test]
    fn mark_warm_plans_event_on_open_only() {
        let fx = &fixture()["dispatch_failed"];
        let session = test_session();
        let planned = plan_mark_warm_dispatch_failed(Some(&session)).expect("event");
        assert_eq!(planned.kind, "chat_warm_failed");
        assert_eq!(
            planned.payload,
            serde_json::json!({"code": "dispatch_failed"})
        );
        // Missing (or non-OPEN, filtered by the lock query) rows: no-op.
        assert_eq!(plan_mark_warm_dispatch_failed(None), None);
        assert_eq!(fx["warm_closed_session_events"].as_u64(), Some(0));
    }

    // -- assistant selection + finalize -----------------------------------

    #[test]
    fn assistant_selection_prefers_turn_scoped_and_stays_lazy() {
        let session_id = uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d");
        let scoped = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session_id);
        let fallback = test_message("f7220594-2481-4d71-aae2-f64a321983c8", session_id);
        // Scoped hit: fallback never runs (one query).
        let mut ran = false;
        let picked = select_active_assistant("t1", Some(&scoped), || {
            ran = true;
            Some(&fallback)
        });
        assert_eq!(picked.map(|message| message.id), Some(scoped.id));
        assert!(!ran);
        // Scoped miss: fallback runs (two queries).
        let picked = select_active_assistant("t1", None, || Some(&fallback));
        assert_eq!(picked.map(|message| message.id), Some(fallback.id));
        // No active turn: straight to the fallback.
        let mut scoped_consulted = false;
        let picked = select_active_assistant("", None, || {
            scoped_consulted = true;
            Some(&fallback)
        });
        assert_eq!(picked.map(|message| message.id), Some(fallback.id));
        assert!(scoped_consulted);
        // FX-RUN-06 `active_assistant` rule pins the same order.
        let fx = &fixture()["active_assistant"];
        assert!(fx["turn_scoped"].as_bool().unwrap());
        assert_eq!(
            fx["fallback_latest"].as_str(),
            Some("f7220594-2481-4d71-aae2-f64a321983c8")
        );
    }

    #[test]
    fn finalize_target_resolution() {
        let override_id = uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9");
        let session_id = uuid("f7220594-2481-4d71-aae2-f64a321983c8");
        assert_eq!(
            finalize_target_message_id(Some(override_id), Some(session_id)),
            Some(override_id)
        );
        assert_eq!(
            finalize_target_message_id(None, Some(session_id)),
            Some(session_id)
        );
        assert_eq!(finalize_target_message_id(None, None), None);
    }

    #[test]
    fn finalize_plan_updates_both_and_returns_assistant() {
        let fx = &fixture()["finalize_active"];
        let session_id = uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d");
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session_id);
        let assistant = test_message("f7220594-2481-4d71-aae2-f64a321983c8", session_id);
        let now = stamp();
        let plan = plan_finalize_active_messages(
            Some(&explicit),
            Some(&assistant),
            AgentChatMessageStatus::Completed,
            now,
        );
        assert_eq!(plan.updates.len(), 2);
        assert_eq!(plan.updates[0].id, explicit.id);
        assert_eq!(plan.updates[1].id, assistant.id);
        for update in &plan.updates {
            assert_eq!(
                update.status.value(),
                fx["explicit_status"].as_str().unwrap()
            );
            assert_eq!(update.completed_at, now);
        }
        assert!(fx["explicit_completed_set"].as_bool().unwrap());
        assert_eq!(plan.returned_message_id, Some(assistant.id));
        assert!(fx["returned_is_assistant"].as_bool().unwrap());
        // Explicit only: returned is the message.
        let plan = plan_finalize_active_messages(
            Some(&explicit),
            None,
            AgentChatMessageStatus::Failed,
            now,
        );
        assert_eq!(plan.updates.len(), 1);
        assert_eq!(plan.returned_message_id, Some(explicit.id));
        // Neither: no updates, returns None.
        let plan = plan_finalize_active_messages(None, None, AgentChatMessageStatus::Failed, now);
        assert!(plan.updates.is_empty());
        assert_eq!(plan.returned_message_id, None);
    }

    // -- fail flows ---------------------------------------------------------

    fn active_session() -> AgentChatSession {
        let mut session = test_session();
        session.active_turn_id = "t1".to_owned();
        session.active_message_id = Some(uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9"));
        session
    }

    #[test]
    fn mark_message_plan_matches_fixture() {
        let fx = &fixture()["dispatch_failed"]["message"];
        let session = active_session();
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session.id);
        let assistant = test_message("f7220594-2481-4d71-aae2-f64a321983c8", session.id);
        let now = stamp();
        let mut clock_calls = 0;
        let mut clock = || {
            clock_calls += 1;
            now
        };
        let plan = plan_mark_message_dispatch_failed(
            &session,
            Some(&explicit),
            Some(&assistant),
            Some(explicit.id),
            "boom-offline",
            &mut clock,
        );
        assert_eq!(plan.session_error, fx["session"]["error"].as_str().unwrap());
        assert!(!plan.close_session);
        assert_eq!(plan.closed_at, None);
        assert_eq!(clock_calls, 1);
        assert_eq!(plan.finalize.updates.len(), 2);
        for update in &plan.finalize.updates {
            assert_eq!(
                update.status.value(),
                fx["message_status"].as_str().unwrap()
            );
            assert_eq!(update.completed_at, now);
        }
        assert!(fx["message_completed_set"].as_bool().unwrap());
        assert_eq!(plan.events.len(), fx["events"].as_array().unwrap().len());
        assert_eq!(
            plan.events[0].kind,
            fx["events"][0]["kind"].as_str().unwrap()
        );
        assert_eq!(plan.events[0].payload, fx["events"][0]["payload"].clone());
        assert_eq!(plan.events[0].message_id, Some(assistant.id));
        assert!(plan.queue_drain);
        assert!(fx["drain_registered"].as_bool().unwrap());
        assert_eq!(plan.finalize.returned_message_id, Some(assistant.id));
    }

    #[test]
    fn mark_message_close_requested_closes_and_reads_clock_twice() {
        let mut session = active_session();
        session.close_requested = true;
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session.id);
        let base = stamp();
        let mut tick = 0;
        let mut clock = || {
            tick += 1;
            base + chrono::Duration::microseconds(tick)
        };
        let plan = plan_mark_message_dispatch_failed(
            &session,
            Some(&explicit),
            None,
            Some(explicit.id),
            "boom",
            &mut clock,
        );
        assert!(plan.close_session);
        assert_eq!(tick, 2);
        assert_eq!(
            plan.closed_at,
            Some(base + chrono::Duration::microseconds(2))
        );
        assert_eq!(
            plan.finalize.updates[0].completed_at,
            base + chrono::Duration::microseconds(1)
        );
        assert_eq!(plan.events.len(), 2);
        assert_eq!(plan.events[1].kind, "chat_closed");
        assert_eq!(
            plan.events[1].payload,
            serde_json::json!({"reason": "close_requested"})
        );
    }

    #[test]
    fn cross_session_message_id_is_silently_ignored() {
        // The executor's session-scoped lookup finds nothing for a
        // foreign id, so only the assistant finalizes (FX-RUN-06
        // `dispatch_failed.message.cross_session_message_status` stays
        // "sent").
        let session = active_session();
        let assistant = test_message("f7220594-2481-4d71-aae2-f64a321983c8", session.id);
        let mut clock = || stamp();
        let plan = plan_mark_message_dispatch_failed(
            &session,
            None,
            Some(&assistant),
            Some(uuid("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            "boom",
            &mut clock,
        );
        assert_eq!(plan.finalize.updates.len(), 1);
        assert_eq!(plan.finalize.updates[0].id, assistant.id);
        assert_eq!(plan.finalize.returned_message_id, Some(assistant.id));
        assert_eq!(
            fixture()["dispatch_failed"]["message"]["cross_session_message_status"].as_str(),
            Some("sent")
        );
    }

    #[test]
    fn sweep_row_plan_matches_fixture() {
        let rows = &fixture()["sweep_active_turns"]["rows"];
        let session = active_session();
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session.id);
        let mut clock = || stamp();
        let plan = plan_sweep_active_row(&session, Some(&explicit), None, &mut clock);
        assert_eq!(plan.session_error, rows["stale"]["error"].as_str().unwrap());
        assert!(!plan.close_session);
        assert_eq!(plan.events.len(), 1);
        assert_eq!(
            plan.events[0].kind,
            rows["stale"]["events"][0]["kind"].as_str().unwrap()
        );
        assert_eq!(
            plan.events[0].payload,
            rows["stale"]["events"][0]["payload"].clone()
        );
        assert!(plan.queue_drain);
        // No detail key on the sweep event (unlike mark/release).
        assert!(plan.events[0].payload.get("detail").is_none());
        // close_requested rows close with chat_closed.
        let mut closing = active_session();
        closing.close_requested = true;
        let plan = plan_sweep_active_row(&closing, Some(&explicit), None, &mut clock);
        assert!(plan.close_session);
        assert_eq!(
            plan.events.len(),
            rows["close_req"]["events"].as_array().unwrap().len()
        );
        assert_eq!(plan.events[1].kind, "chat_closed");
    }

    #[test]
    fn release_row_plan_truncates_error_but_not_event_detail() {
        let rows = &fixture()["release_active"]["rows"];
        let session = active_session();
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session.id);
        let detail = format!("session reopened detail {}", "D".repeat(3000));
        let mut clock = || stamp();
        let plan = plan_release_row(&session, Some(&explicit), None, &detail, &mut clock);
        // QUIRK-ERROR-TRUNCATION: stored error cut to 2000 chars, event
        // detail kept whole.
        assert_eq!(
            plan.session_error.chars().count(),
            rows["active"]["error_len"].as_u64().unwrap() as usize
        );
        assert_eq!(
            plan.events[0].payload["detail"].as_str(),
            Some(detail.as_str())
        );
        assert_eq!(
            plan.events[0].kind,
            rows["active"]["events"][0]["kind"].as_str().unwrap()
        );
        assert_eq!(
            plan.events[0].payload["code"].as_str(),
            rows["active"]["events"][0]["payload"]["code"].as_str()
        );
        assert!(plan.queue_drain);
        // Release never closes, even when close was requested.
        let mut closing = active_session();
        closing.close_requested = true;
        let plan = plan_release_row(&closing, Some(&explicit), None, &detail, &mut clock);
        assert!(!plan.close_session);
        assert_eq!(plan.events.len(), 1);
        // Idle rows are skipped before planning.
        assert!(!release_row_applies(&test_session()));
        assert!(release_row_applies(&session));
    }

    // -- complete ------------------------------------------------------------

    fn complete_case(name: &str) -> Value {
        fixture()["complete_turn"]
            .as_array()
            .expect("cases")
            .iter()
            .find(|case| case["case"].as_str() == Some(name))
            .expect("case")
            .clone()
    }

    #[test]
    fn complete_close_cases_match_fixture() {
        for (name, status) in [
            ("completed-close", AgentChatMessageStatus::Completed),
            ("cancelled-close", AgentChatMessageStatus::Cancelled),
            ("failed-close", AgentChatMessageStatus::Failed),
        ] {
            let case = complete_case(name);
            let mut session = active_session();
            session.close_requested = true;
            let now = stamp();
            let plan = plan_complete_active_turn(&session, status, None, now);
            assert!(plan.close_session, "{name}");
            assert_eq!(plan.closed_at, Some(now), "{name}");
            assert_eq!(plan.last_message_at, now, "{name}");
            assert_eq!(
                plan.message_update.as_ref().expect("update").completed_at,
                now
            );
            let message_status = &plan.message_update.as_ref().expect("update").status;
            assert_eq!(
                message_status.value(),
                case["message_status"].as_str().unwrap()
            );
            assert_eq!(plan.events.len(), case["events"].as_array().unwrap().len());
            assert_eq!(
                plan.events[0].kind,
                case["events"][0]["kind"].as_str().unwrap()
            );
            assert_eq!(plan.events[0].payload, case["events"][0]["payload"].clone());
            assert_eq!(plan.events[1].kind, "chat_closed");
            assert!(plan.queue_drain, "{name}");
            // Effect count: one publish per event plus the drain.
            assert_eq!(case["on_commit"].as_array().unwrap().len(), 3);
        }
    }

    #[test]
    fn complete_open_and_idle_cases_match_fixture() {
        // Streaming never closes, even when close was requested.
        let case = complete_case("streaming-close");
        let mut session = active_session();
        session.close_requested = true;
        let plan =
            plan_complete_active_turn(&session, AgentChatMessageStatus::Streaming, None, stamp());
        assert!(!plan.close_session);
        assert_eq!(plan.closed_at, None);
        assert_eq!(plan.events.len(), 1);
        assert_eq!(plan.events[0].payload, case["events"][0]["payload"].clone());
        assert!(plan.queue_drain);
        // Open sessions stay open.
        let case = complete_case("completed-open");
        let session = active_session();
        let plan =
            plan_complete_active_turn(&session, AgentChatMessageStatus::Completed, None, stamp());
        assert!(!plan.close_session);
        assert_eq!(plan.events.len(), 1);
        assert_eq!(plan.events[0].payload, case["events"][0]["payload"].clone());
        assert!(plan.queue_drain);
        // Idle turns queue no drain: one effect total.
        let case = complete_case("idle-no-drain");
        let plan = plan_complete_active_turn(
            &test_session(),
            AgentChatMessageStatus::Completed,
            None,
            stamp(),
        );
        assert!(!plan.queue_drain);
        assert_eq!(plan.message_update, None);
        assert_eq!(plan.events.len(), 1);
        assert_eq!(case["on_commit_count"].as_u64(), Some(1));
    }

    #[test]
    fn complete_payload_falls_back_when_falsy() {
        let default = serde_json::json!({"status": "completed"});
        assert_eq!(
            complete_event_payload(None, &AgentChatMessageStatus::Completed),
            default
        );
        assert_eq!(
            complete_event_payload(
                Some(&serde_json::json!({})),
                &AgentChatMessageStatus::Completed
            ),
            default
        );
        let custom = serde_json::json!({"status": "completed", "extra": 1});
        assert_eq!(
            complete_event_payload(Some(&custom), &AgentChatMessageStatus::Completed),
            custom
        );
    }

    // -- create_assistant -----------------------------------------------------

    #[test]
    fn create_assistant_vectors_match_fixture() {
        let fx = &fixture()["create_assistant"];
        let inputs = create_assistant_inputs("t1", "i1", AgentChatMessageStatus::Streaming);
        assert_eq!(
            inputs.local_turn_id,
            fx["default"]["local_turn_id"].as_str().unwrap()
        );
        assert_eq!(
            inputs.local_item_id,
            fx["default"]["local_item_id"].as_str().unwrap()
        );
        assert_eq!(
            inputs.status.value(),
            fx["default"]["status"].as_str().unwrap()
        );
        let cut = create_assistant_inputs(
            &"t".repeat(200),
            &"i".repeat(200),
            AgentChatMessageStatus::Streaming,
        );
        assert_eq!(
            cut.local_turn_id.chars().count(),
            fx["truncated_turn_len"].as_u64().unwrap() as usize
        );
        assert_eq!(
            cut.local_item_id.chars().count(),
            fx["truncated_item_len"].as_u64().unwrap() as usize
        );
    }

    // -- sweeps ---------------------------------------------------------------

    #[test]
    fn sweep_empty_sql_is_byte_exact() {
        let fx = &fixture()["sweep_empty"];
        let cutoff: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-01T22:52:02.927778+00:00")
                .unwrap()
                .with_timezone(&Utc);
        assert_eq!(sweep_empty_ids_sql(cutoff), fx["sql"][0].as_str().unwrap());
        let closed: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-02T22:52:02.928857+00:00")
                .unwrap()
                .with_timezone(&Utc);
        assert_eq!(
            sweep_empty_close_sql(&[uuid("1d271194-9131-4bfc-bc3f-cb6b36dfa173")], closed),
            fx["sql"][1].as_str().unwrap()
        );
        assert_eq!(fx["returned"].as_u64(), Some(1));
    }

    #[test]
    fn sweep_active_ids_prefix_matches_fixture() {
        // The fixture truncates the statement at 200 chars; pin the
        // recorded prefix (minus the cut-off token tail).
        let recorded = fixture()["sweep_active_turns"]["first_sql"][0]
            .as_str()
            .expect("sql")
            .to_owned();
        let cutoff: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-02T22:22:02.927778+00:00")
                .unwrap()
                .with_timezone(&Utc);
        let sql = sweep_active_ids_sql(cutoff);
        assert!(sql.starts_with(&recorded[..recorded.len() - 4]), "{sql}");
        assert!(sql.contains(
            "INNER JOIN \"runner\" ON (\"agent_chat_session\".\"runner_id\" = \"runner\".\"id\")"
        ));
        assert!(sql.contains("\"agent_chat_session\".\"status\" = 'open'"));
        assert!(sql.contains(ACTIVE_TURN_PREDICATE_SQL));
        assert!(sql.contains("\"runner\".\"status\" = 'offline'"));
        assert!(sql.ends_with("ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC"));
        assert_eq!(CHAT_ACTIVE_TIMEOUT_SECS, 1800);
        assert_eq!(
            sweep_active_cutoff(stamp()),
            stamp() - chrono::Duration::seconds(1800)
        );
    }

    #[test]
    fn release_ids_prefix_matches_fixture() {
        let recorded = fixture()["release_active"]["first_sql"][0]
            .as_str()
            .expect("sql")
            .to_owned();
        let sql = release_ids_sql(uuid("a6007fab-1d06-459f-9062-3c6bbb794a83"));
        assert!(sql.starts_with(&recorded[..recorded.len() - 4]), "{sql}");
        assert!(sql.contains("\"agent_chat_session\".\"status\" = 'open'"));
        assert!(sql.contains(ACTIVE_TURN_PREDICATE_SQL));
        assert!(sql.ends_with("ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC"));
    }

    // -- remaining SQL shapes --------------------------------------------------

    #[test]
    fn runner_has_active_chat_sql_shape() {
        assert_eq!(
            runner_has_active_chat_sql(uuid("cd871d43-1c13-4f30-a838-2495c1c4baac")),
            "SELECT 1 AS \"a\" FROM \"agent_chat_session\" WHERE (\"agent_chat_session\".\"runner_id\" = 'cd871d431c134f30a8382495c1c4baac'::uuid AND \"agent_chat_session\".\"status\" = 'open' AND (\"agent_chat_session\".\"active_message_id\" IS NOT NULL OR NOT (\"agent_chat_session\".\"active_turn_id\" = ''))) LIMIT 1"
        );
    }

    #[test]
    fn lock_sql_shapes() {
        let by_pk = session_lock_by_pk_sql(uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"));
        assert!(by_pk.contains("WHERE \"agent_chat_session\".\"id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid ORDER BY"));
        assert!(!by_pk.contains("\"status\" = 'open'"));
        assert!(by_pk.ends_with("LIMIT 1 FOR UPDATE"));
        let open = session_lock_open_sql(uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"));
        assert!(open.contains(
            "WHERE (\"agent_chat_session\".\"id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid AND \"agent_chat_session\".\"status\" = 'open')"
        ));
        assert!(open.ends_with("LIMIT 1 FOR UPDATE"));
        // Full row in both locks.
        for sql in [&by_pk, &open] {
            assert!(sql.starts_with(
                "SELECT \"agent_chat_session\".\"id\", \"agent_chat_session\".\"workspace_id\""
            ));
        }
    }

    #[test]
    fn lookup_sql_shapes() {
        let session = uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d");
        let message = uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9");
        let lookup = finalize_explicit_lookup_sql(session, message);
        assert!(lookup.contains(&format!(
            "FROM \"agent_chat_message\" INNER JOIN \"agent_chat_session\" ON (\"agent_chat_message\".\"session_id\" = \"agent_chat_session\".\"id\") WHERE (\"agent_chat_message\".\"id\" = {} AND \"agent_chat_message\".\"session_id\" = {})",
            "'9d469513f80c4f0cab20e5827f95c3f9'::uuid",
            "'9fe1b18d460248c793520bee23df1f9d'::uuid",
        )));
        assert!(lookup.ends_with("ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_message\".\"seq\" ASC LIMIT 1"));
        let scoped = assistant_turn_scoped_sql(session, "t1");
        assert!(scoped.contains(
            "WHERE (\"agent_chat_message\".\"role\" = 'assistant' AND \"agent_chat_message\".\"session_id\" = '9fe1b18d460248c793520bee23df1f9d'::uuid AND \"agent_chat_message\".\"status\" = 'streaming' AND \"agent_chat_message\".\"local_turn_id\" = 't1')"
        ));
        assert!(scoped.ends_with("ORDER BY \"agent_chat_message\".\"created_at\" DESC LIMIT 1"));
        let fallback = assistant_fallback_sql(session);
        assert!(!fallback.contains("local_turn_id\" ="));
        assert!(fallback.ends_with("ORDER BY \"agent_chat_message\".\"created_at\" DESC LIMIT 1"));
        let publish = publish_event_lookup_sql(47);
        assert!(publish.contains(
            "FROM \"agent_chat_event\" INNER JOIN \"agent_chat_session\" ON (\"agent_chat_event\".\"session_id\" = \"agent_chat_session\".\"id\") WHERE \"agent_chat_event\".\"id\" = 47"
        ));
        assert!(publish.ends_with("ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_event\".\"seq\" ASC LIMIT 1"));
    }

    #[test]
    fn update_sql_shapes() {
        // Fail update without close: four SET columns.
        let session = active_session();
        let explicit = test_message("9d469513-f80c-4f0c-ab20-e5827f95c3f9", session.id);
        let mut clock = || stamp();
        let plan = plan_sweep_active_row(&session, Some(&explicit), None, &mut clock);
        let sql = fail_session_update_sql(session.id, &plan, stamp());
        assert!(sql.starts_with("UPDATE \"agent_chat_session\" SET \"active_turn_id\" = '', \"active_message_id\" = NULL, \"error\" = 'active chat turn timed out', \"updated_at\" = "));
        assert!(!sql.contains("\"status\" ="));
        assert!(sql.ends_with(&format!(
            "WHERE \"agent_chat_session\".\"id\" = {}",
            "'9fe1b18d460248c793520bee23df1f9d'::uuid"
        )));
        // Fail update with close: six SET columns.
        let mut closing = active_session();
        closing.close_requested = true;
        let plan = plan_sweep_active_row(&closing, Some(&explicit), None, &mut clock);
        let sql = fail_session_update_sql(closing.id, &plan, stamp());
        assert!(sql.contains("SET \"status\" = 'closed', \"active_turn_id\" = '', \"active_message_id\" = NULL, \"closed_at\" = '2026-10-02 22:52:02.000000+00:00'::timestamptz, \"error\" = 'active chat turn timed out', \"updated_at\" ="));
        // Message status write (shared finalize/complete shape).
        let update = MessageStatusUpdate {
            id: explicit.id,
            status: AgentChatMessageStatus::Failed,
            completed_at: stamp(),
        };
        assert_eq!(
            message_status_update_sql(&update),
            complete_message_update_sql(&update),
        );
        assert_eq!(
            message_status_update_sql(&update),
            "UPDATE \"agent_chat_message\" SET \"status\" = 'failed', \"completed_at\" = '2026-10-02 22:52:02.000000+00:00'::timestamptz WHERE \"agent_chat_message\".\"id\" = '9d469513f80c4f0cab20e5827f95c3f9'::uuid"
        );
        // Complete update always writes all six columns in model field order.
        let plan =
            plan_complete_active_turn(&session, AgentChatMessageStatus::Completed, None, stamp());
        let sql = complete_session_update_sql(&session, &plan, stamp());
        assert!(sql.contains("SET \"status\" = 'open', \"active_turn_id\" = '', \"active_message_id\" = NULL, \"last_message_at\" = '2026-10-02 22:52:02.000000+00:00'::timestamptz, \"closed_at\" = NULL, \"updated_at\" ="));
    }

    #[test]
    fn insert_message_sql_shape() {
        let created: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2026-10-02T22:52:02.822934+00:00")
                .unwrap()
                .with_timezone(&Utc);
        let sql = insert_message_sql(
            uuid("9d469513-f80c-4f0c-ab20-e5827f95c3f9"),
            uuid("9fe1b18d-4602-48c7-9352-0bee23df1f9d"),
            &AgentChatMessageRole::Assistant,
            "",
            &Value::Array(Vec::new()),
            &AgentChatMessageStatus::Streaming,
            "i1",
            "t1",
            1,
            created,
            None,
        );
        assert_eq!(
            sql,
            "INSERT INTO \"agent_chat_message\" (\"id\", \"session_id\", \"role\", \"content\", \"content_parts\", \"status\", \"local_item_id\", \"local_turn_id\", \"seq\", \"created_at\", \"completed_at\") VALUES ('9d469513f80c4f0cab20e5827f95c3f9'::uuid, '9fe1b18d460248c793520bee23df1f9d'::uuid, 'assistant', '', '[]'::jsonb, 'streaming', 'i1', 't1', 1, '2026-10-02 22:52:02.822934+00:00'::timestamptz, NULL)"
        );
    }

    #[test]
    fn update_field_consts_match_python_order() {
        assert_eq!(
            FAIL_SESSION_UPDATE_FIELDS,
            ["active_message_id", "active_turn_id", "error", "updated_at"]
        );
        assert_eq!(FAIL_SESSION_CLOSE_FIELDS, ["status", "closed_at"]);
        assert_eq!(
            COMPLETE_SESSION_UPDATE_FIELDS,
            [
                "active_message_id",
                "active_turn_id",
                "last_message_at",
                "status",
                "closed_at",
                "updated_at",
            ]
        );
        assert_eq!(
            sweep_empty_cutoff(stamp()),
            stamp() - chrono::Duration::hours(24)
        );
    }
}
