#![forbid(unsafe_code)]

//! Run-lifecycle handlers (D-15, stage 5).
//!
//! Pure port of `apps/api/pi_dash/runner/services/run_lifecycle.py:1-463`:
//! `_usage_updates` (`:80-100`), `apply_run_paused` (`:158-244`),
//! `apply_run_resume_unavailable` (`:247-295`),
//! `apply_assign_rejected_busy` (`:298-322`), `_post_failure_comment`
//! (`:341-399`) and `finalize_run_terminal` (`:402-463`), plus the
//! shared normalizers (`:56-64`). `TERMINAL_RUN_STATUSES` is reused
//! from L1, `merge_usage` from L1, `enrich_run_error` from L1, and
//! `merge_done_payload` from the sibling [`super::finalization`]
//! module (the source's circular import, cut at the module edge).
//!
//! Every entry point takes pre-fetched facts and returns a plan: typed
//! updates, the exact SQL text (`$N` placeholders) and ordered
//! [`LifecycleEffect`]s. Transaction boundaries per entry point:
//!
//! * Pause: phase 1 (lock + update) runs in one transaction; phase 2
//!   re-reads the run and inserts the question comment in autocommit;
//!   phase 3 (`PauseAndDrain`: orchestration, then runner drain) is
//!   registered with `on_commit` — deferred to the request commit at
//!   every real call site, since the run endpoints wrap the call in
//!   `transaction.atomic()`.
//! * Requeue pair: the caller holds the transaction (the endpoints
//!   pass an already-locked row); the pod drain is `on_commit`.
//! * Terminal: the updates plan feeds `finalize_agent_run`, whose own
//!   transaction + `on_commit` publish live in [`super::finalization`].
//!
//! Fixture: FX-RUN-05 (`apply_run_paused`, `resume_unavailable`,
//! `assign_rejected_busy`, `finalize_run_terminal`,
//! `post_failure_comment` sections).

use pidash_db::app_pages::strip::ml_strip_tags;
use pidash_db::runner_runs::agent_run;
use pidash_types::runner_runs::{
    enrich_run_error, merge_usage, AgentRunStatus, RefusalCategory, RunnerInfo,
};
use serde_json::Value;
use uuid::Uuid;

use super::finalization::{merge_done_payload, plan_finalize_values, FinalizeValues};
use super::{
    py_str, py_strip, py_truthy, qualified_columns, truncate_chars, LifecycleEffect, SetValue,
    ISSUE_COLUMNS, PROJECT_COLUMNS, STATE_COLUMNS,
};

/// `PROJECT_MOVE_HANDOFF_CONFIG_KEY`
/// (`orchestration/service.py:57`): the `run_config` key whose
/// truthy value marks a source-project run stopped for a move.
pub const PROJECT_MOVE_HANDOFF_CONFIG_KEY: &str = "_project_move_handoff";

/// `error` truncation in `finalize_run_terminal` (`[:16000]`, chars).
pub const RUN_ERROR_MAX_CHARS: usize = 16_000;

/// `llm_model` truncation (`[:128]`, chars).
pub const LLM_MODEL_MAX_CHARS: usize = 128;

/// Detail prefixes that suppress the failure comment
/// (`run_lifecycle.py:335-338`), in tuple order.
pub const INFRA_FAILURE_DETAIL_PREFIXES: [&str; 2] = [
    "daemon shutdown requested",
    "agent stalled: no events for >",
];

/// `fire_tick` Celery wire name (`bgtasks/agent_ticker.py:77`).
pub const FIRE_TICK_TASK: &str = "pi_dash.bgtasks.agent_ticker.fire_tick";

/// `runner_live_state` columns in Django `_meta` order (captured from
/// `RunnerLiveState._meta.concrete_fields`): the pause/terminal paths
/// fetch the full row, not the L2 read subset. D-14 owns the write
/// path and extends the L2 module; this list pins the `SELECT` text.
const LIVE_STATE_COLUMNS: &[&str] = &[
    "runner_id",
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "usage",
    "llm_model",
    "turn_count",
    "updated_at",
];

/// `issue_comments` columns in Django `_meta` order (captured from
/// `IssueComment._meta.concrete_fields`): the comment `INSERT` lists
/// all 24.
const COMMENT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "comment_stripped",
    "comment_json",
    "comment_html",
    "description_id",
    "attachments",
    "labels",
    "issue_id",
    "actor_id",
    "access",
    "external_source",
    "external_id",
    "speaker_type",
    "speaker_label",
    "speaker_agent_run_id",
    "edited_at",
    "parent_id",
];

/// `descriptions` columns in Django `_meta` order (captured from
/// `Description._meta.concrete_fields`).
const DESCRIPTION_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "workspace_id",
    "project_id",
    "description_json",
    "description_html",
    "description_binary",
    "description_stripped",
];

/// How a lifecycle plan fails. Each variant is a Python exception the
/// source raises for the same input; the executing layer maps them to
/// the same 500 the Django handler renders.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LifecycleError {
    /// `payload.get` on a truthy non-dict pause payload (`AttributeError`).
    #[error("pause payload must be an object")]
    PausePayloadNotObject,
    /// `(autonomy or {}).get` on a truthy non-dict `autonomy` (`AttributeError`).
    #[error("pause payload autonomy must be an object")]
    PauseAutonomyNotObject,
    /// `.strip()` / `[:16000]` on a truthy non-string terminal detail
    /// (`AttributeError` / `TypeError`).
    #[error("terminal error detail must be a string")]
    TerminalErrorDetailNotString,
    /// `finalize_agent_run` with a non-terminal status (`ValueError`,
    /// message kept verbatim).
    #[error("new_status must be terminal")]
    NonTerminalStatus,
}

/// `_has_project_move_handoff` (`run_lifecycle.py:50-53`):
/// `bool((run_config or {}).get(KEY))`. A truthy non-object config
/// raises `AttributeError` in Django; that shape is unreachable from
/// real writes (the column is dict-defaulted), so it reads as absent.
pub fn has_project_move_handoff(run_config: &Value) -> bool {
    match run_config.as_object() {
        Some(map) => map
            .get(PROJECT_MOVE_HANDOFF_CONFIG_KEY)
            .map(py_truthy)
            .unwrap_or(false),
        None => false,
    }
}

/// `_normalize_model` (`run_lifecycle.py:56-57`):
/// `str(raw or "").strip()[:128]` (chars, never splitting UTF-8).
pub fn normalize_model(raw: &Value) -> String {
    if !py_truthy(raw) {
        return String::new();
    }
    truncate_chars(py_strip(&py_str(raw)), LLM_MODEL_MAX_CHARS)
}

/// `_normalize_refusal_category` (`run_lifecycle.py:60-64`): lowercased
/// membership against the known values, `Unknown` for anything else.
pub fn normalize_refusal_category(raw: &Value) -> RefusalCategory {
    if !py_truthy(raw) {
        return RefusalCategory::Unknown;
    }
    let lowered = py_strip(&py_str(raw)).to_lowercase();
    RefusalCategory::from_value(&lowered).unwrap_or(RefusalCategory::Unknown)
}

/// `_payload_usage` (`run_lifecycle.py:67-70`): the done payload's
/// `usage`, or null when the payload is not a dict or has none.
pub fn payload_usage(payload: &Value) -> Value {
    payload
        .as_object()
        .and_then(|map| map.get("usage"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The live-state snapshot `_usage_updates` reads: `usage` plus the
/// nullable `llm_model`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveStateUsageFacts {
    pub usage: Value,
    pub llm_model: Option<String>,
}

/// The `AgentRun` usage/model writes (`usage` only when at least one
/// source reported something; `llm_model` only when the snapshot has
/// a non-empty one, truncated to 128 chars).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageUpdates {
    pub usage: Option<Value>,
    pub llm_model: Option<String>,
}

/// `_usage_updates` (`run_lifecycle.py:80-100`): three-source merge —
/// live-state snapshot, done payload `usage`, terminal frame `tokens`
/// — fresher winning per counter via L1, plus the snapshot model.
pub fn usage_updates(
    state: Option<&LiveStateUsageFacts>,
    payload_usage: &Value,
    tokens: &Value,
) -> UsageUpdates {
    let state_usage = state
        .map(|facts| facts.usage.clone())
        .unwrap_or(Value::Null);
    let merged = merge_usage(&[state_usage, payload_usage.clone(), tokens.clone()]);
    UsageUpdates {
        usage: if py_truthy(&merged) {
            Some(merged)
        } else {
            None
        },
        llm_model: state
            .and_then(|facts| facts.llm_model.clone())
            .filter(|model| !model.is_empty())
            .map(|model| truncate_chars(&model, LLM_MODEL_MAX_CHARS)),
    }
}

/// Pause-lock predicate (`apply_run_paused:180-186`): the row must be
/// neither terminal nor `CANCEL_REQUESTED`. Id/runner equality is the
/// SQL's job; this pins the status matrix for unit tests.
pub fn pause_lock_passes(row_status: AgentRunStatus) -> bool {
    !row_status.is_terminal() && row_status != AgentRunStatus::CancelRequested
}

/// Requeue-lock predicate (`apply_run_resume_unavailable:264-269`):
/// the row must be non-terminal.
pub fn requeue_lock_passes(row_status: AgentRunStatus) -> bool {
    !row_status.is_terminal()
}

/// Pause lock (`apply_run_paused:180-186`): full `agent_run` row by
/// id + runner, excluding terminal states and `CANCEL_REQUESTED`,
/// `ORDER BY created_at DESC LIMIT 1 FOR UPDATE`. Params: `$1` run
/// id, `$2` runner id, `$3..$7` the terminal values in tuple order,
/// `$8` `cancel_requested`.
pub fn lock_run_for_pause_sql() -> String {
    format!(
        "SELECT {} FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \
         \"agent_run\".\"runner_id\" = $2 AND NOT (\"agent_run\".\"status\" IN \
         ($3, $4, $5, $6, $7)) AND NOT (\"agent_run\".\"status\" = $8)) \
         ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        qualified_columns("agent_run", agent_run::COLUMNS)
    )
}

/// Requeue fallback lock (`apply_run_resume_unavailable:264-269`):
/// same shape without the `CANCEL_REQUESTED` exclusion. Params: `$1`
/// run id, `$2` runner id, `$3..$7` terminal values.
pub fn lock_run_for_requeue_sql() -> String {
    format!(
        "SELECT {} FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \
         \"agent_run\".\"runner_id\" = $2 AND NOT (\"agent_run\".\"status\" IN \
         ($3, $4, $5, $6, $7))) ORDER BY \"agent_run\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified_columns("agent_run", agent_run::COLUMNS)
    )
}

/// Live-state read (`_matching_live_state`, `run_lifecycle.py:73-77`):
/// full row by runner + observed run id. No `ORDER BY` (the model has
/// no `Meta.ordering`). Params follow Django's empirical `WHERE`
/// order: `$1` observed run id, `$2` runner id.
pub fn live_state_by_runner_sql() -> String {
    format!(
        "SELECT {} FROM \"runner_live_state\" WHERE \
         (\"runner_live_state\".\"observed_run_id\" = $1 AND \
         \"runner_live_state\".\"runner_id\" = $2) LIMIT 1",
        qualified_columns("runner_live_state", LIVE_STATE_COLUMNS)
    )
}

/// Pause phase-2 re-read (`apply_run_paused:197`):
/// `select_related("work_item").get(id)` — 41 + 34 columns, `.get()`
/// so no `LIMIT`. A missing row returns from the whole function
/// *before* the `on_commit` registration, skipping the drain too.
/// Param: `$1` run id.
pub fn pause_reread_sql() -> String {
    format!(
        "SELECT {}, {} FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
         (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") WHERE \
         \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC",
        qualified_columns("agent_run", agent_run::COLUMNS),
        qualified_columns("issues", ISSUE_COLUMNS)
    )
}

/// `_pause_and_drain` re-read (`apply_run_paused:231-239`):
/// `select_related("work_item", "work_item__state",
/// "work_item__project").filter(pk).first()` — 41 + 34 + 46 + 18
/// columns. Join order follows Django's field-definition order
/// (projects before states), not the argument order. Param: `$1` run id.
pub fn pause_drain_reread_sql() -> String {
    format!(
        "SELECT {}, {}, {}, {} FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
         (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") LEFT OUTER JOIN \
         \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \
         \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") WHERE \
         \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1",
        qualified_columns("agent_run", agent_run::COLUMNS),
        qualified_columns("issues", ISSUE_COLUMNS),
        qualified_columns("projects", PROJECT_COLUMNS),
        qualified_columns("states", STATE_COLUMNS)
    )
}

/// Failure-comment re-read (`_post_failure_comment:367`):
/// `select_related("work_item", "work_item__project").filter(pk).first()` —
/// 41 + 34 + 46 columns. Param: `$1` run id.
pub fn failure_reread_sql() -> String {
    format!(
        "SELECT {}, {}, {} FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
         (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") LEFT OUTER JOIN \
         \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") WHERE \
         \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1",
        qualified_columns("agent_run", agent_run::COLUMNS),
        qualified_columns("issues", ISSUE_COLUMNS),
        qualified_columns("projects", PROJECT_COLUMNS)
    )
}

/// Pending-entry ticker lookup (`_fire_pending_entry:148-152`):
/// `filter(issue_id, enabled, pending_entry).values_list("id").first()`.
/// The default manager adds `deleted_at IS NULL`; the two booleans
/// render as bare columns; no `ORDER BY`. Param: `$1` issue id.
pub fn ticker_pending_entry_sql() -> &'static str {
    "SELECT \"issue_agent_ticker\".\"id\" FROM \"issue_agent_ticker\" WHERE \
     (\"issue_agent_ticker\".\"deleted_at\" IS NULL AND \
     \"issue_agent_ticker\".\"enabled\" AND \"issue_agent_ticker\".\"issue_id\" = $1 AND \
     \"issue_agent_ticker\".\"pending_entry\") LIMIT 1"
}

/// Failure-comment dedupe check (`_post_failure_comment:370-375`):
/// `(issue, speaker_agent_run_id, SYSTEM)` existence, soft-deleted
/// rows excluded by the default manager. Params: `$1` issue id,
/// `$2` speaker run id, `$3` speaker type (`system`).
pub fn comment_dedupe_exists_sql() -> &'static str {
    "SELECT 1 AS \"a\" FROM \"issue_comments\" WHERE \
     (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"issue_id\" = $1 AND \
     \"issue_comments\".\"speaker_agent_run_id\" = $2 AND \
     \"issue_comments\".\"speaker_type\" = $3) LIMIT 1"
}

/// `IssueComment` speaker (`db/models/issue.py:551-554`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentSpeaker {
    Human,
    Agent,
    System,
    Integration,
}

impl CommentSpeaker {
    pub fn value(&self) -> &'static str {
        match self {
            CommentSpeaker::Human => "human",
            CommentSpeaker::Agent => "agent",
            CommentSpeaker::System => "system",
            CommentSpeaker::Integration => "integration",
        }
    }
}

impl std::fmt::Display for CommentSpeaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// A comment's derived content: the `comment_html` plus everything
/// `IssueComment.save` / `Description.save` derive from it.
/// `comment_stripped` uses `MLStripper` (entities decoded);
/// `description_stripped` uses Django's `strip_tags` (entities
/// verbatim) — `Description.save` recomputes it, discarding the
/// value `IssueComment.save` passes. `comment_json` is always `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPlan {
    pub comment_html: String,
    pub comment_stripped: String,
    pub description_stripped: String,
    pub speaker: CommentSpeaker,
    pub speaker_label: String,
    pub speaker_agent_run_id: Option<Uuid>,
}

impl CommentPlan {
    /// Pause comment (`apply_run_paused:220-226`): speaker fields left
    /// at their defaults (`human` / `""` / null).
    pub fn pause(comment_html: String) -> Self {
        Self {
            comment_stripped: ml_strip_tags(&comment_html),
            description_stripped: django_strip_tags(&comment_html),
            comment_html,
            speaker: CommentSpeaker::Human,
            speaker_label: String::new(),
            speaker_agent_run_id: None,
        }
    }

    /// Failure comment (`_post_failure_comment:390-399`): `SYSTEM` /
    /// `"Pi Dash"` / run id.
    pub fn failure(comment_html: String, run_id: Uuid) -> Self {
        Self {
            comment_stripped: ml_strip_tags(&comment_html),
            description_stripped: django_strip_tags(&comment_html),
            comment_html,
            speaker: CommentSpeaker::System,
            speaker_label: "Pi Dash".to_owned(),
            speaker_agent_run_id: Some(run_id),
        }
    }
}

/// Bindings for the comment `INSERT`: the fetched ids plus the plan.
/// `created_at`/`updated_at` each bind their own `now()` (two
/// `pre_save` calls); `created_by/updated_by` bind the ambient
/// request user, `None` on lifecycle paths (daemon endpoints and
/// workers carry no `crum` user).
#[derive(Debug, Clone, PartialEq)]
pub struct CommentInsert {
    pub comment_id: Uuid,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub plan: CommentPlan,
}

/// Comment `INSERT` (`IssueComment.objects.create`): all 24 columns in
/// `_meta` order, values bound positionally — `$1`/`$2` now,
/// `$3`/`$4` ambient user, `$5` null, `$6` comment id, `$7`/`$8`
/// project/workspace, `$9` stripped, `$10` `{}`, `$11` html, `$12`
/// null (linked after the description insert), `$13`/`$14` `[]`,
/// `$15` issue, `$16` actor, `$17` `"INTERNAL"`, `$18`/`$19` null,
/// `$20` speaker, `$21` label, `$22` speaker run, `$23`/`$24` null.
/// No `RETURNING` (client-assigned UUID key).
pub fn comment_insert_sql() -> String {
    let columns = COMMENT_COLUMNS
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let params = (1..=COMMENT_COLUMNS.len())
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"issue_comments\" ({columns}) VALUES ({params})")
}

/// Description `INSERT` (`IssueComment.save`): all 12 columns — `$1`/`$2`
/// now, `$3`/`$4` ambient user (the passed defaults are discarded by
/// `BaseModel.save` — a ported quirk), `$5` null, `$6` new id,
/// `$7`/`$8` workspace/project, `$9` `{}`, `$10` html, `$11` null,
/// `$12` Django-stripped text.
pub fn description_insert_sql() -> String {
    let columns = DESCRIPTION_COLUMNS
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let params = (1..=DESCRIPTION_COLUMNS.len())
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"descriptions\" ({columns}) VALUES ({params})")
}

/// Description link-back (`IssueComment.save`): single-column update —
/// `update_fields=["description_id"]` writes nothing else (auto_now is
/// *not* force-included). Params: `$1` description id, `$2` comment id.
/// Zero rows raise `DatabaseError` in Django; the executing layer maps
/// that to its row-missing error.
pub fn comment_description_link_sql() -> &'static str {
    "UPDATE \"issue_comments\" SET \"description_id\" = $1 WHERE \"issue_comments\".\"id\" = $2"
}

/// `html.escape(str, quote=True)`: what `format_html` applies to each
/// argument (`&`, `<`, `>`, `"`, `'`).
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Django's `django.utils.html.strip_tags` (what `Description.save`
/// recomputes with): loop `_strip_once` while both `<` and `>` remain,
/// stopping when an iteration stops removing `<`. Entities pass
/// through verbatim. Mirrors the `api::space::sanitize::strip_tags`
/// twin, which this crate cannot import (crate graph runs
/// `types` → `db` → `services` → `api`); the depth-guard
/// `SuspiciousOperation` arms are omitted there and here — generated
/// comment HTML can never trigger them.
fn django_strip_tags(value: &str) -> String {
    let mut current = value.to_owned();
    loop {
        if !(current.contains('<') && current.contains('>')) {
            break;
        }
        let next = django_strip_once(&current);
        if next.matches('<').count() == current.matches('<').count() {
            break;
        }
        current = next;
    }
    current
}

/// One Django `_strip_once` pass: collect text and verbatim entity
/// references, skip tags (quote-aware), comments and declarations.
fn django_strip_once(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            // `<` is ASCII, so byte detection never splits a
            // multi-byte sequence; entities pass through verbatim.
            let ch = value[index..].chars().next().expect("char boundary");
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        if value[index..].starts_with("<!--") {
            if let Some(end) = value[index..].find("-->") {
                index += end + 3;
            } else {
                // Unterminated comment: bogus-comments to EOF.
                break;
            }
            continue;
        }
        if value[index..].starts_with("<!") || value[index..].starts_with("<?") {
            if let Some(end) = find_tag_end(&value[index..]) {
                index += end;
            } else {
                out.push('<');
                index += 1;
            }
            continue;
        }
        // A `<` followed by ASCII alpha or `/` opens a tag; anything
        // else is literal text.
        let opener = value[index + 1..].chars().next().unwrap_or('\0');
        if opener.is_ascii_alphabetic() || opener == '/' {
            if let Some(end) = find_tag_end(&value[index..]) {
                index += end;
                continue;
            }
        }
        out.push('<');
        index += 1;
    }
    out
}

/// Byte length of `<…>` at `text[0] == '<'`, respecting
/// single/double quotes; `None` when unterminated.
fn find_tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(q) = quote {
            if byte == q {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

/// Pause phase-1 writes (`apply_run_paused:191-195`): status becomes
/// `PAUSED_AWAITING_INPUT`, `done_payload` merges over the locked
/// row's (keeping an earlier `yield`), usage/model follow
/// [`usage_updates`] with the frame model winning.
#[derive(Debug, Clone, PartialEq)]
pub struct PauseUpdatePlan {
    pub done_payload: Value,
    pub usage: Option<Value>,
    pub llm_model: Option<String>,
}

/// Plan the pause row update. `stored_done_payload` is the locked
/// row's `done_payload`; `payload` the pause frame; `tokens`/`model`
/// the frame's usage/model; `live` the live-state snapshot, if any.
/// Total — even a non-dict payload plans here; only the phase-2
/// comment can fail (after this update commits).
pub fn plan_pause_update(
    stored_done_payload: &Value,
    payload: &Value,
    tokens: &Value,
    model: &Value,
    live: Option<&LiveStateUsageFacts>,
) -> PauseUpdatePlan {
    let updates = usage_updates(live, &payload_usage(payload), tokens);
    let mut llm_model = updates.llm_model;
    let model_value = normalize_model(model);
    if !model_value.is_empty() {
        llm_model = Some(model_value);
    }
    PauseUpdatePlan {
        done_payload: merge_done_payload(stored_done_payload, payload),
        usage: updates.usage,
        llm_model,
    }
}

/// Pause `UPDATE` (`apply_run_paused:191-195`): `SET` order is
/// `status`, `done_payload`, then `usage`/`llm_model` when the plan
/// carries them. Params: `$1` `paused_awaiting_input`, `$2` merged
/// payload, `$N` usage jsonb / model text, `$N` run id.
pub fn pause_update_sql(plan: &PauseUpdatePlan) -> String {
    let mut set = vec![
        "\"status\" = $1".to_owned(),
        "\"done_payload\" = $2".to_owned(),
    ];
    let mut next = 3;
    for column in ["usage", "llm_model"] {
        let present = match column {
            "usage" => plan.usage.is_some(),
            _ => plan.llm_model.is_some(),
        };
        if present {
            set.push(format!("\"{column}\" = ${next}"));
            next += 1;
        }
    }
    format!(
        "UPDATE \"agent_run\" SET {} WHERE \"agent_run\".\"id\" = ${next}",
        set.join(", ")
    )
}

/// Pause question/summary HTML (`apply_run_paused:207-218`): the
/// question block, then the summary block, joined with no separator.
/// `None` when both are absent (no comment row). Arguments are
/// escaped exactly like `format_html`.
pub fn pause_comment_html(question: Option<&str>, summary: Option<&str>) -> Option<String> {
    let mut parts = String::new();
    if let Some(question) = question {
        parts.push_str(&format!(
            "<p><strong>Agent paused — question:</strong></p><p>{}</p>",
            escape_html(question)
        ));
    }
    if let Some(summary) = summary {
        parts.push_str(&format!(
            "<p><em>Summary so far:</em> {}</p>",
            escape_html(summary)
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

/// Plan the pause comment from the frame (`apply_run_paused:201-226`).
/// The question is `(payload.autonomy or {}).question_for_human`, the
/// summary `payload.summary`, each kept when truthy (raw) and
/// stringified for the template. `Ok(None)` means no comment.
/// `Err` is the source's `AttributeError` for a truthy non-dict
/// payload or autonomy value.
pub fn plan_pause_comment(payload: &Value) -> Result<Option<CommentPlan>, LifecycleError> {
    let fields = payload
        .as_object()
        .ok_or(LifecycleError::PausePayloadNotObject)?;
    let question = match fields.get("autonomy") {
        None => None,
        Some(value) if !py_truthy(value) => None,
        Some(Value::Object(autonomy)) => autonomy
            .get("question_for_human")
            .filter(|q| py_truthy(q))
            .map(py_str),
        Some(_) => return Err(LifecycleError::PauseAutonomyNotObject),
    };
    let summary = fields.get("summary").filter(|s| py_truthy(s)).map(py_str);
    match pause_comment_html(question.as_deref(), summary.as_deref()) {
        None => Ok(None),
        Some(html) => Ok(Some(CommentPlan::pause(html))),
    }
}

/// The locked row `apply_run_resume_unavailable` resets. `parent_thread_id`
/// is `Some` exactly when a parent row exists; Django raises
/// `DoesNotExist` when `parent_run_id` is set but the row is missing,
/// so the executing layer must fail the same way instead of passing
/// `None` for a dangling id (impossible under the FK constraint).
#[derive(Debug, Clone, PartialEq)]
pub struct RequeueRunFacts {
    pub id: Uuid,
    pub pod_id: Option<Uuid>,
    pub parent_run_id: Option<Uuid>,
    pub parent_thread_id: Option<String>,
}

/// Requeue writes (`apply_run_resume_unavailable:272-295`): the run
/// row back to `QUEUED` with runner/pin/assigned/queue cleared, the
/// parent thread cleared when set (saved *before* the run row, both
/// inside the caller's transaction), and the pod drain `on_commit`.
#[derive(Debug, Clone, PartialEq)]
pub struct RequeuePlan {
    pub run_id: Uuid,
    pub parent_thread_clear: Option<Uuid>,
    pub after_commit: Option<LifecycleEffect>,
}

/// Plan the requeue from the locked row. `None` from the fallback
/// lock means no-op (no plan, no effects).
pub fn plan_requeue_from_locked(run: &RequeueRunFacts) -> RequeuePlan {
    debug_assert!(
        run.parent_run_id.is_none() || run.parent_thread_id.is_some(),
        "dangling parent_run_id raises DoesNotExist in Django",
    );
    let parent_thread_clear = match (&run.parent_run_id, &run.parent_thread_id) {
        (Some(parent_id), Some(thread)) if !thread.is_empty() => Some(*parent_id),
        _ => None,
    };
    RequeuePlan {
        run_id: run.id,
        parent_thread_clear,
        after_commit: run
            .pod_id
            .map(|pod_id| LifecycleEffect::DrainPod { pod_id }),
    }
}

/// Requeue `UPDATE` (`apply_run_resume_unavailable:282-290`):
/// `save(update_fields=["status", "runner", "pinned_runner",
/// "assigned_at", "queue_position"])`, in that order. Params: `$1`
/// `queued`, `$2..$5` null, `$6` run id. Zero rows raise
/// `DatabaseError`.
pub fn requeue_update_sql() -> &'static str {
    "UPDATE \"agent_run\" SET \"status\" = $1, \"runner_id\" = $2, \
     \"pinned_runner_id\" = $3, \"assigned_at\" = $4, \"queue_position\" = $5 \
     WHERE \"agent_run\".\"id\" = $6"
}

/// Parent thread clear (`apply_run_resume_unavailable:279-281`):
/// single-column `UPDATE` to `""`. Params: `$1` `""`, `$2` parent id.
pub fn parent_thread_clear_sql() -> &'static str {
    "UPDATE \"agent_run\" SET \"thread_id\" = $1 WHERE \"agent_run\".\"id\" = $2"
}

/// The busy rejection (`apply_assign_rejected_busy:321-322`): flip the
/// runner to `BUSY` first — so the pod drain cannot instantly
/// re-assign the run to the runner that just NACKed it — then the
/// shared requeue.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignRejectedBusyPlan {
    pub busy_flip_runner_id: Uuid,
    pub requeue: RequeuePlan,
}

/// Plan the busy rejection: the flip plus [`plan_requeue_from_locked`].
pub fn plan_assign_rejected_busy(runner_id: Uuid, run: &RequeueRunFacts) -> AssignRejectedBusyPlan {
    AssignRejectedBusyPlan {
        busy_flip_runner_id: runner_id,
        requeue: plan_requeue_from_locked(run),
    }
}

/// Runner `BUSY` flip (`apply_assign_rejected_busy:321`): single-column
/// update, no timestamp touch. Params: `$1` `busy`, `$2` runner id.
pub fn runner_busy_update_sql() -> &'static str {
    "UPDATE \"runner\" SET \"status\" = $1 WHERE \"runner\".\"id\" = $2"
}

/// Inputs to `finalize_run_terminal` (`run_lifecycle.py:402-412`).
/// `done_payload`/`tokens`/`model`/`refusal_category` are raw frame
/// values; `error_detail` the raw `detail` (stringified-and-checked,
/// since Django crashes on truthy non-strings here); `live` the
/// live-state snapshot; `runner` the enrich signals for `FAILED`.
/// (`RunnerInfo` has no `PartialEq`, so neither does this struct.)
#[derive(Debug, Clone)]
pub struct TerminalUpdateInputs<'a> {
    pub status: AgentRunStatus,
    pub done_payload: &'a Value,
    pub error_detail: &'a Value,
    pub refusal_category: &'a Value,
    pub tokens: &'a Value,
    pub model: &'a Value,
    pub live: Option<&'a LiveStateUsageFacts>,
    pub runner: Option<&'a RunnerInfo<'a>>,
}

/// Per-status terminal writes (`finalize_run_terminal:426-449`).
/// `status`/`ended_at`/`queue_position` are always written (base of
/// the finalize values); `done_payload` only on `COMPLETED` (verbatim,
/// pre-merge — the merge runs in the finalize step against the locked
/// row); `error` on `COMPLETED` (`""`), `FAILED` (enriched, only when
/// the detail is non-empty) and `REFUSED` (raw, same gate);
/// `refusal_category` always on `REFUSED`. Usage/model follow
/// [`usage_updates`] with the frame model winning.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalUpdates {
    pub status: AgentRunStatus,
    pub done_payload: Option<Value>,
    pub error: Option<String>,
    pub refusal_category: Option<RefusalCategory>,
    pub usage: Option<Value>,
    pub llm_model: Option<String>,
}

/// Plan the terminal writes. A late call for an already-closed run is
/// *not* decided here — first-writer-wins lives in the finalize lock;
/// the executing layer logs `run_lifecycle: ignoring late terminal
/// transition for closed run %s` (info, `pi_dash.runner.services.run_lifecycle`)
/// when the finalize step reports no row.
pub fn plan_terminal_updates(
    inputs: &TerminalUpdateInputs,
) -> Result<TerminalUpdates, LifecycleError> {
    let mut updates = TerminalUpdates {
        status: inputs.status,
        done_payload: None,
        error: None,
        refusal_category: None,
        usage: None,
        llm_model: None,
    };
    if inputs.status == AgentRunStatus::Completed {
        updates.done_payload = Some(inputs.done_payload.clone());
        updates.error = Some(String::new());
    }
    if inputs.status == AgentRunStatus::Failed && py_truthy(inputs.error_detail) {
        let detail = terminal_detail_str(inputs.error_detail)?;
        let model_owned = if py_truthy(inputs.model) {
            py_str(inputs.model)
        } else {
            String::new()
        };
        let enriched = enrich_run_error(Some(detail), inputs.runner, Some(&model_owned));
        updates.error = Some(truncate_chars(&enriched, RUN_ERROR_MAX_CHARS));
    }
    if inputs.status == AgentRunStatus::Refused {
        updates.refusal_category = Some(normalize_refusal_category(inputs.refusal_category));
        if py_truthy(inputs.error_detail) {
            let detail = terminal_detail_str(inputs.error_detail)?;
            updates.error = Some(truncate_chars(detail, RUN_ERROR_MAX_CHARS));
        }
    }
    let merged = usage_updates(
        inputs.live,
        &payload_usage(inputs.done_payload),
        inputs.tokens,
    );
    updates.usage = merged.usage;
    updates.llm_model = merged.llm_model;
    let model_value = normalize_model(inputs.model);
    if !model_value.is_empty() {
        updates.llm_model = Some(model_value);
    }
    Ok(updates)
}

/// The truthy terminal detail as a string, or the source's crash for a
/// truthy non-string (`AttributeError` on `.strip`, `TypeError` on
/// `[:16000]`).
fn terminal_detail_str(detail: &Value) -> Result<&str, LifecycleError> {
    detail
        .as_str()
        .ok_or(LifecycleError::TerminalErrorDetailNotString)
}

/// Caller-`updates` extras for the finalize values, in exact dict
/// insertion order: per-status keys first (`done_payload`, `error` /
/// `refusal_category`, `error` / `error`), then `usage`, then
/// `llm_model`. `status`/`ended_at`/`queue_position` are *not* extras —
/// they overwrite the finalize base in place.
pub fn terminal_extras(updates: &TerminalUpdates) -> Vec<(&'static str, SetValue)> {
    let mut extras = Vec::new();
    match updates.status {
        AgentRunStatus::Completed => {
            if let Some(payload) = &updates.done_payload {
                extras.push(("done_payload", SetValue::Json(payload.clone())));
            }
            if let Some(error) = &updates.error {
                extras.push(("error", SetValue::Text(error.clone())));
            }
        }
        AgentRunStatus::Refused => {
            if let Some(category) = &updates.refusal_category {
                extras.push((
                    "refusal_category",
                    SetValue::Text(category.value().to_owned()),
                ));
            }
            if let Some(error) = &updates.error {
                extras.push(("error", SetValue::Text(error.clone())));
            }
        }
        _ => {
            if let Some(error) = &updates.error {
                extras.push(("error", SetValue::Text(error.clone())));
            }
        }
    }
    if let Some(usage) = &updates.usage {
        extras.push(("usage", SetValue::Json(usage.clone())));
    }
    if let Some(model) = &updates.llm_model {
        extras.push(("llm_model", SetValue::Text(model.clone())));
    }
    extras
}

/// Composed terminal path (`finalize_run_terminal:450-457`): the updates
/// plan plus the finalize values (expected runner bound by the
/// executing layer in the lock). Non-terminal statuses fail here, as
/// `finalize_agent_run` raises `ValueError` in the source.
pub fn plan_terminal_finalize(
    inputs: &TerminalUpdateInputs,
) -> Result<FinalizeValues, LifecycleError> {
    let updates = plan_terminal_updates(inputs)?;
    let extras = terminal_extras(&updates);
    plan_finalize_values(updates.status, &extras).map_err(|_| LifecycleError::NonTerminalStatus)
}

/// Infra-flavored detail check (`_post_failure_comment:363-365`):
/// case-sensitive prefix match on the *stripped* detail.
pub fn is_infra_failure_detail(stripped_detail: &str) -> bool {
    INFRA_FAILURE_DETAIL_PREFIXES
        .iter()
        .any(|prefix| stripped_detail.starts_with(prefix))
}

/// Plan the failure comment (`_post_failure_comment:341-399`): strip,
/// suppress infra noise, then render — the `<pre>` detail block, or
/// `(no diagnostic detail)` when empty. `None` means suppressed; the
/// executing layer still runs the row re-read and the dedupe check
/// (a hit or a missing work item also means silence).
pub fn plan_failure_comment(error: &str, run_id: Uuid) -> Option<CommentPlan> {
    let detail = py_strip(error);
    if is_infra_failure_detail(detail) {
        return None;
    }
    let html = if detail.is_empty() {
        format!(
            "<p><strong>Run failed.</strong> {}</p>",
            escape_html("(no diagnostic detail)")
        )
    } else {
        format!(
            "<p><strong>Run failed.</strong></p><pre>{}</pre>",
            escape_html(detail)
        )
    };
    Some(CommentPlan::failure(html, run_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::runner_runs::TERMINAL_RUN_STATUSES;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-05-lifecycle.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn json_value(text: &str) -> Value {
        serde_json::from_str(text).expect("json parses")
    }

    // ------------------------------------------------------------------
    // normalizers
    // ------------------------------------------------------------------

    #[test]
    fn terminal_in_list_matches_l1_tuple_order() {
        // The order every lock statement binds its `IN` list in.
        let l1: Vec<&str> = TERMINAL_RUN_STATUSES.iter().map(|s| s.value()).collect();
        assert_eq!(
            l1,
            ["completed", "failed", "cancelled", "blocked", "refused"]
        );
    }

    #[test]
    fn normalize_model_matches_python_battery() {
        // Differential oracle: /tmp/dj534diff.py `model:*` (real `str(raw or "").strip()[:128]`).
        let cases: Vec<(Value, &str)> = vec![
            (Value::Null, ""),
            (json!(true), "True"),
            (json!(false), ""),
            (json!(5), "5"),
            (json!(-3), "-3"),
            (json!(5.0), "5.0"),
            (json!(0.1), "0.1"),
            (json!(1e16), "1e+16"),
            (json!(1e-5), "1e-05"),
            (json!("abc"), "abc"),
            (json!("  \t x \n "), "x"),
            (json!("\u{1c}full\u{1c}"), "full"),
            (json!([]), ""),
            (json!({}), ""),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_model(&input), expected, "input {input}");
        }
        // Beyond u64 the digits are already f64: renders like the
        // equivalent float (see `py_num_str`).
        let big = json_value("1000000000000000000000000000000");
        assert_eq!(normalize_model(&big), "1e+30");
        // Containers stringify in repr form (key order follows the map).
        assert_eq!(normalize_model(&json!([1, "a"])), "[1, 'a']");
        assert_eq!(normalize_model(&json!({"a": 1})), "{'a': 1}");
        // 128 chars, never splitting UTF-8.
        assert_eq!(normalize_model(&json!("y".repeat(200))).len(), 128);
        let wide = normalize_model(&json!("—".repeat(200)));
        assert_eq!(wide.chars().count(), 128);
        assert_eq!(wide, "—".repeat(128));
    }

    #[test]
    fn normalize_refusal_category_matches_python_battery() {
        // Differential oracle: /tmp/dj534diff.py `ref:*`.
        let cases: Vec<(Value, RefusalCategory)> = vec![
            (json!("cyber"), RefusalCategory::Cyber),
            (json!("CYBER"), RefusalCategory::Cyber),
            (json!(" Cyber "), RefusalCategory::Cyber),
            (json!("bio"), RefusalCategory::Bio),
            (
                json!("reasoning_extraction"),
                RefusalCategory::ReasoningExtraction,
            ),
            (json!("bogus"), RefusalCategory::Unknown),
            (json!(""), RefusalCategory::Unknown),
            (Value::Null, RefusalCategory::Unknown),
            (json!(0), RefusalCategory::Unknown),
            (json!(5), RefusalCategory::Unknown),
            (json!(true), RefusalCategory::Unknown),
            (json!("UNKNOWN"), RefusalCategory::Unknown),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_refusal_category(&input),
                expected,
                "input {input}"
            );
        }
    }

    #[test]
    fn has_project_move_handoff_reads_config_key() {
        assert_eq!(PROJECT_MOVE_HANDOFF_CONFIG_KEY, "_project_move_handoff");
        assert!(!has_project_move_handoff(&json!({})));
        assert!(!has_project_move_handoff(&Value::Null));
        assert!(!has_project_move_handoff(&json!({"other": 1})));
        assert!(!has_project_move_handoff(
            &json!({"_project_move_handoff": {}})
        ));
        assert!(!has_project_move_handoff(
            &json!({"_project_move_handoff": ""})
        ));
        assert!(has_project_move_handoff(
            &json!({"_project_move_handoff": {"target": "p"}})
        ));
        assert!(has_project_move_handoff(
            &json!({"_project_move_handoff": "x"})
        ));
        // Unreachable shape (dict-defaulted column): reads as absent.
        assert!(!has_project_move_handoff(&json!("nope")));
    }

    #[test]
    fn payload_usage_extracts_usage_or_null() {
        assert_eq!(
            payload_usage(&json!({"usage": {"input": 1}})),
            json!({"input": 1})
        );
        assert_eq!(payload_usage(&json!({"conclusion": "x"})), Value::Null);
        assert_eq!(payload_usage(&Value::Null), Value::Null);
        assert_eq!(payload_usage(&json!([1])), Value::Null);
    }

    // ------------------------------------------------------------------
    // usage updates
    // ------------------------------------------------------------------

    fn live_state(usage: Value, llm_model: Option<&str>) -> LiveStateUsageFacts {
        LiveStateUsageFacts {
            usage,
            llm_model: llm_model.map(str::to_owned),
        }
    }

    #[test]
    fn usage_updates_merge_three_sources_fresher_wins() {
        let live = live_state(json!({"input_tokens": 1}), Some("live-model"));
        let updates = usage_updates(
            Some(&live),
            &json!({"input_tokens": 12}),
            &json!({"input": 20, "output": 5, "total": 15}),
        );
        let usage = updates.usage.expect("usage reported");
        // Provider-shape payload normalizes; canonical tokens win per key.
        assert_eq!(usage.get("input").and_then(Value::as_i64), Some(20));
        assert_eq!(usage.get("output").and_then(Value::as_i64), Some(5));
        assert_eq!(usage.get("total").and_then(Value::as_i64), Some(15));
        assert_eq!(updates.llm_model.as_deref(), Some("live-model"));
    }

    #[test]
    fn usage_updates_empty_when_nothing_reported() {
        assert_eq!(
            usage_updates(None, &Value::Null, &Value::Null),
            UsageUpdates {
                usage: None,
                llm_model: None,
            }
        );
        let live = live_state(json!({}), None);
        let updates = usage_updates(Some(&live), &Value::Null, &Value::Null);
        assert_eq!(updates.usage, None);
        assert_eq!(updates.llm_model, None);
    }

    #[test]
    fn usage_updates_truncates_snapshot_model() {
        let live = live_state(json!({"input": 1}), Some(&"m".repeat(200)));
        let updates = usage_updates(Some(&live), &Value::Null, &Value::Null);
        assert_eq!(updates.llm_model.as_deref().map(str::len), Some(128));
        let live = live_state(json!({"input": 1}), Some(""));
        assert_eq!(
            usage_updates(Some(&live), &Value::Null, &Value::Null).llm_model,
            None
        );
    }

    // ------------------------------------------------------------------
    // pause
    // ------------------------------------------------------------------

    #[test]
    fn pause_lock_matrix() {
        use AgentRunStatus::*;
        for status in [
            Queued,
            Assigned,
            WaitingForWorktree,
            Running,
            AwaitingApproval,
            AwaitingReauth,
            PausedAwaitingInput,
        ] {
            assert!(pause_lock_passes(status), "{status} locks");
        }
        for status in [
            CancelRequested,
            Completed,
            Failed,
            Cancelled,
            Blocked,
            Refused,
        ] {
            assert!(!pause_lock_passes(status), "{status} no-ops");
        }
        let fx = fixture();
        let noops = fx.get("apply_run_paused_noops").expect("noops");
        assert_eq!(
            noops.get("terminal_still").and_then(Value::as_str),
            Some("completed")
        );
        assert_eq!(
            noops.get("cancel_requested_still").and_then(Value::as_str),
            Some("cancel_requested")
        );
        assert_eq!(
            noops.get("no_work_item_status").and_then(Value::as_str),
            Some("paused_awaiting_input")
        );
    }

    #[test]
    fn requeue_lock_matrix() {
        for status in TERMINAL_RUN_STATUSES {
            assert!(!requeue_lock_passes(status), "{status} no-ops");
        }
        assert!(requeue_lock_passes(AgentRunStatus::Running));
        assert!(requeue_lock_passes(AgentRunStatus::CancelRequested));
        assert!(requeue_lock_passes(AgentRunStatus::Assigned));
        let fx = fixture();
        assert_eq!(
            fx["resume_unavailable_extra"]
                .get("terminal_still")
                .and_then(Value::as_str),
            Some("failed")
        );
    }

    #[test]
    fn pause_update_merges_payload_and_overrides_model() {
        let stored = json!({"status": "progressed", "note": "n", "yielded_at": "t"});
        let payload = json!({"summary": "did things"});
        let live = live_state(json!({"input": 1}), Some("live-model"));
        let plan = plan_pause_update(
            &stored,
            &payload,
            &Value::Null,
            &json!("frame-model"),
            Some(&live),
        );
        // Yield keys survive onto the incoming payload.
        assert_eq!(
            plan.done_payload.get("summary").and_then(Value::as_str),
            Some("did things")
        );
        assert_eq!(
            plan.done_payload.get("status").and_then(Value::as_str),
            Some("progressed")
        );
        assert_eq!(
            plan.done_payload.get("yielded_at").and_then(Value::as_str),
            Some("t")
        );
        // Frame model beats the snapshot.
        assert_eq!(plan.llm_model.as_deref(), Some("frame-model"));
        assert!(plan.usage.is_some());
        // No model anywhere: snapshot-absent stays absent.
        let plan = plan_pause_update(&stored, &payload, &Value::Null, &Value::Null, None);
        assert_eq!(plan.llm_model, None);
        // Empty-string model does not override either.
        let plan = plan_pause_update(&stored, &payload, &Value::Null, &json!(""), Some(&live));
        assert_eq!(plan.llm_model.as_deref(), Some("live-model"));
    }

    #[test]
    fn pause_comment_html_matches_fixture() {
        let fx = fixture();
        let html = fx["apply_run_paused"]["comments"][0]
            .get("comment_html")
            .and_then(Value::as_str)
            .expect("comment html");
        assert_eq!(
            pause_comment_html(Some("Which DB?"), Some("did things")).as_deref(),
            Some(html)
        );
        assert_eq!(
            pause_comment_html(Some("Which DB?"), None).as_deref(),
            Some("<p><strong>Agent paused — question:</strong></p><p>Which DB?</p>")
        );
        assert_eq!(
            pause_comment_html(None, Some("did things")).as_deref(),
            Some("<p><em>Summary so far:</em> did things</p>")
        );
        assert_eq!(pause_comment_html(None, None), None);
    }

    #[test]
    fn pause_comment_escapes_like_format_html() {
        // Differential oracle: /tmp/dj534diff.py `pause_escape`.
        assert_eq!(
            pause_comment_html(None, Some("a&b<\"c'>")).as_deref(),
            Some("<p><em>Summary so far:</em> a&amp;b&lt;&quot;c&#x27;&gt;</p>")
        );
        assert_eq!(
            pause_comment_html(Some("cost <5€ & >3¥"), None).as_deref(),
            Some(
                "<p><strong>Agent paused — question:</strong></p>\
                 <p>cost &lt;5€ &amp; &gt;3¥</p>"
            )
        );
    }

    #[test]
    fn plan_pause_comment_extracts_and_validates() {
        let fx = fixture();
        let html = fx["apply_run_paused"]["comments"][0]
            .get("comment_html")
            .and_then(Value::as_str)
            .expect("comment html");
        let payload = json!({
            "summary": "did things",
            "autonomy": {"question_for_human": "Which DB?"},
        });
        let plan = plan_pause_comment(&payload)
            .expect("plans")
            .expect("comment");
        assert_eq!(plan.comment_html, html);
        assert_eq!(plan.speaker, CommentSpeaker::Human);
        assert_eq!(plan.speaker_label, "");
        assert_eq!(plan.speaker_agent_run_id, None);
        // No question/summary: no comment.
        assert_eq!(
            plan_pause_comment(&json!({"other": 1})).expect("plans"),
            None
        );
        assert_eq!(
            plan_pause_comment(&json!({"autonomy": Value::Null})).expect("plans"),
            None
        );
        // Non-string truthy values stringify for the template.
        let plan = plan_pause_comment(&json!({"summary": 5}))
            .expect("plans")
            .expect("comment");
        assert!(plan
            .comment_html
            .ends_with("<p><em>Summary so far:</em> 5</p>"));
        // Truthy non-dict shapes are the source's AttributeError.
        assert_eq!(
            plan_pause_comment(&json!([1])),
            Err(LifecycleError::PausePayloadNotObject)
        );
        assert_eq!(
            plan_pause_comment(&json!({"autonomy": "x"})),
            Err(LifecycleError::PauseAutonomyNotObject)
        );
    }

    #[test]
    fn comment_strips_decode_and_verbatim() {
        // Differential oracle: /tmp/dj534diff.py `ml:*` / `dj:*`.
        let plain = "<p><strong>Agent paused — question:</strong></p>\
             <p>Which DB?</p><p><em>Summary so far:</em> did things</p>";
        let plan = CommentPlan::pause(plain.to_owned());
        assert_eq!(
            plan.comment_stripped,
            "Agent paused — question:Which DB?Summary so far: did things"
        );
        assert_eq!(
            plan.description_stripped,
            "Agent paused — question:Which DB?Summary so far: did things"
        );
        // Entities: MLStripper decodes, Django keeps verbatim.
        let escaped = "<p><em>Summary so far:</em> a&amp;b&lt;&quot;c&#x27;&gt;</p>";
        let plan = CommentPlan::pause(escaped.to_owned());
        assert_eq!(plan.comment_stripped, "Summary so far: a&b<\"c'>");
        assert_eq!(
            plan.description_stripped,
            "Summary so far: a&amp;b&lt;&quot;c&#x27;&gt;"
        );
        let failure = CommentPlan::failure(
            "<p><strong>Run failed.</strong></p><pre>boom detail</pre>".to_owned(),
            Uuid::nil(),
        );
        assert_eq!(failure.comment_stripped, "Run failed.boom detail");
        assert_eq!(failure.description_stripped, "Run failed.boom detail");
        assert_eq!(failure.speaker, CommentSpeaker::System);
        assert_eq!(failure.speaker_label, "Pi Dash");
        assert_eq!(failure.speaker_agent_run_id, Some(Uuid::nil()));
    }

    #[test]
    fn django_strip_twin_matches_basics() {
        assert_eq!(django_strip_tags("<p>why</p>"), "why");
        assert_eq!(django_strip_tags("plain"), "plain");
        assert_eq!(django_strip_tags(""), "");
        assert_eq!(django_strip_tags("<p>a</p><p>b</p>"), "ab");
        assert_eq!(django_strip_tags("a &amp; b"), "a &amp; b");
        assert_eq!(django_strip_tags("a < b"), "a < b");
        assert_eq!(django_strip_tags("a < b > c"), "a < b > c");
    }

    // ------------------------------------------------------------------
    // requeue
    // ------------------------------------------------------------------

    fn requeue_facts() -> RequeueRunFacts {
        RequeueRunFacts {
            id: Uuid::nil(),
            pod_id: Some(Uuid::nil()),
            parent_run_id: Some(Uuid::nil()),
            parent_thread_id: Some("thread-1".to_owned()),
        }
    }

    #[test]
    fn requeue_plan_resets_and_drains_pod() {
        let fx = fixture();
        let row = fx.get("resume_unavailable").expect("resume section")["row"].clone();
        assert_eq!(row.get("status").and_then(Value::as_str), Some("queued"));
        assert!(row.get("runner").unwrap().is_null());
        assert!(row.get("pinned").unwrap().is_null());
        assert!(row.get("assigned_at").unwrap().is_null());
        assert!(row.get("queue_position").unwrap().is_null());
        assert_eq!(
            fx["resume_unavailable"]
                .get("parent_thread_id")
                .and_then(Value::as_str),
            Some("''")
        );

        let plan = plan_requeue_from_locked(&requeue_facts());
        assert_eq!(plan.run_id, Uuid::nil());
        assert_eq!(plan.parent_thread_clear, Some(Uuid::nil()));
        assert_eq!(
            plan.after_commit,
            Some(LifecycleEffect::DrainPod {
                pod_id: Uuid::nil()
            })
        );
        // No parent / empty thread: no clear.
        let mut facts = requeue_facts();
        facts.parent_thread_id = Some(String::new());
        assert_eq!(plan_requeue_from_locked(&facts).parent_thread_clear, None);
        facts.parent_run_id = None;
        facts.parent_thread_id = None;
        assert_eq!(plan_requeue_from_locked(&facts).parent_thread_clear, None);
        // No pod: no drain.
        facts.pod_id = None;
        assert_eq!(plan_requeue_from_locked(&facts).after_commit, None);
    }

    #[test]
    fn busy_plan_flips_runner_first() {
        let fx = fixture();
        let busy = fx.get("assign_rejected_busy").expect("busy section");
        assert_eq!(
            busy.get("run_status").and_then(Value::as_str),
            Some("queued")
        );
        assert!(busy.get("run_runner").unwrap().is_null());
        assert_eq!(
            busy.get("runner_status").and_then(Value::as_str),
            Some("busy")
        );
        let plan = plan_assign_rejected_busy(Uuid::nil(), &requeue_facts());
        assert_eq!(plan.busy_flip_runner_id, Uuid::nil());
        assert_eq!(plan.requeue, plan_requeue_from_locked(&requeue_facts()));
    }

    // ------------------------------------------------------------------
    // terminal updates
    // ------------------------------------------------------------------

    fn terminal_inputs<'a>(
        status: AgentRunStatus,
        done_payload: &'a Value,
        error_detail: &'a Value,
        refusal_category: &'a Value,
        tokens: &'a Value,
        model: &'a Value,
    ) -> TerminalUpdateInputs<'a> {
        TerminalUpdateInputs {
            status,
            done_payload,
            error_detail,
            refusal_category,
            tokens,
            model,
            live: None,
            runner: None,
        }
    }

    #[test]
    fn terminal_completed_writes_payload_and_clears_error() {
        let fx = fixture();
        let gold = fx["finalize_run_terminal"]["completed"].clone();
        let done = json!({"usage": {"input_tokens": 12}, "conclusion": "ok"});
        let inputs = terminal_inputs(
            AgentRunStatus::Completed,
            &done,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert_eq!(updates.done_payload.as_ref(), Some(&done));
        assert_eq!(updates.error.as_deref(), Some(""));
        assert_eq!(updates.refusal_category, None);
        assert_eq!(updates.done_payload, gold.get("done_payload").cloned());
        assert_eq!(
            gold.get("error").and_then(Value::as_str),
            Some("''"),
            "cleared error renders as empty",
        );
        assert_eq!(
            gold.get("status").and_then(Value::as_str),
            Some("completed")
        );
    }

    #[test]
    fn terminal_failed_enriches_and_truncates() {
        let fx = fixture();
        let detail = json!("invalid authentication credentials (401) for /Users/x");
        let inputs = terminal_inputs(
            AgentRunStatus::Failed,
            &Value::Null,
            &detail,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        let error = updates.error.expect("error written");
        // Same enrich head shape as the fixture's auth failure.
        assert!(error.starts_with("401 authentication_failed\nAI agent: "));
        assert!(error.contains("Raw agent error:"));
        assert!(error.len() <= RUN_ERROR_MAX_CHARS);
        assert_eq!(
            fx["finalize_run_terminal"]
                .get("failed_enriched_has_marker")
                .and_then(Value::as_bool),
            Some(true)
        );
        // 16000-char cap.
        let long = json!("E".repeat(20_000));
        let inputs = terminal_inputs(
            AgentRunStatus::Failed,
            &Value::Null,
            &long,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert_eq!(updates.error.expect("error").chars().count(), 16_000);
        assert_eq!(
            fx["finalize_run_terminal"]
                .get("failed_truncated_len")
                .and_then(Value::as_u64),
            Some(16_000)
        );
        // Empty detail: no error write at all.
        let empty = json!("");
        let inputs = terminal_inputs(
            AgentRunStatus::Failed,
            &Value::Null,
            &empty,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(plan_terminal_updates(&inputs).expect("plans").error, None);
        // Truthy non-string detail: the source's crash.
        let number = json!(5);
        let inputs = terminal_inputs(
            AgentRunStatus::Failed,
            &Value::Null,
            &number,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(
            plan_terminal_updates(&inputs),
            Err(LifecycleError::TerminalErrorDetailNotString)
        );
    }

    #[test]
    fn terminal_refused_normalizes_category() {
        let fx = fixture();
        let refused = fx["finalize_run_terminal"]["refused"].clone();
        let detail = json!("declined hard");
        let category = json!("cyber");
        let inputs = terminal_inputs(
            AgentRunStatus::Refused,
            &Value::Null,
            &detail,
            &category,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert_eq!(updates.refusal_category, Some(RefusalCategory::Cyber));
        assert_eq!(updates.error.as_deref(), Some("declined hard"));
        assert_eq!(
            refused.get("category").and_then(Value::as_str),
            Some("cyber")
        );
        assert_eq!(
            refused.get("error").and_then(Value::as_str),
            Some("declined hard")
        );
        // Bad category + empty detail.
        let bad = fx["finalize_run_terminal"]["refused_bad_category"].clone();
        let empty = json!("");
        let bogus = json!("bogus");
        let inputs = terminal_inputs(
            AgentRunStatus::Refused,
            &Value::Null,
            &empty,
            &bogus,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert_eq!(updates.refusal_category, Some(RefusalCategory::Unknown));
        assert_eq!(updates.error, None);
        assert_eq!(bad.get("category").and_then(Value::as_str), Some("unknown"));
        assert_eq!(bad.get("error").and_then(Value::as_str), Some("''"));
    }

    #[test]
    fn terminal_cancelled_writes_neither_payload_nor_error() {
        let fx = fixture();
        let gold = fx["finalize_run_terminal"]["cancelled"].clone();
        let done = json!({"new": true});
        let ignored = json!("ignored");
        let inputs = terminal_inputs(
            AgentRunStatus::Cancelled,
            &done,
            &ignored,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert_eq!(updates.done_payload, None);
        assert_eq!(updates.error, None);
        assert_eq!(updates.refusal_category, None);
        // Stored values survive untouched.
        assert_eq!(
            gold.get("done_payload"),
            Some(&json!({"old": true})),
            "cancelled keeps stored payload",
        );
        assert_eq!(
            gold.get("error").and_then(Value::as_str),
            Some("'old-err'"),
            "cancelled keeps stored error",
        );
    }

    #[test]
    fn terminal_extras_follow_dict_insertion_order() {
        // COMPLETED: done_payload, error, usage, llm_model.
        let done = json!({"conclusion": "ok"});
        let tokens = json!({"input": 1});
        let model = json!("m");
        let inputs = terminal_inputs(
            AgentRunStatus::Completed,
            &done,
            &Value::Null,
            &Value::Null,
            &tokens,
            &model,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        let keys: Vec<&str> = terminal_extras(&updates)
            .iter()
            .map(|(column, _)| *column)
            .collect();
        assert_eq!(keys, ["done_payload", "error", "usage", "llm_model"]);
        // REFUSED: refusal_category, error, usage, llm_model.
        let detail = json!("d");
        let category = json!("cyber");
        let inputs = terminal_inputs(
            AgentRunStatus::Refused,
            &Value::Null,
            &detail,
            &category,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        let keys: Vec<&str> = terminal_extras(&updates)
            .iter()
            .map(|(column, _)| *column)
            .collect();
        assert_eq!(keys, ["refusal_category", "error"]);
        // FAILED without detail: usage only when reported.
        let empty = json!("");
        let inputs = terminal_inputs(
            AgentRunStatus::Failed,
            &Value::Null,
            &empty,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let updates = plan_terminal_updates(&inputs).expect("plans");
        assert!(terminal_extras(&updates).is_empty());
    }

    #[test]
    fn terminal_finalize_composes_and_rejects_nonterminal() {
        let done = json!({"conclusion": "ok"});
        let inputs = terminal_inputs(
            AgentRunStatus::Completed,
            &done,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        let values = plan_terminal_finalize(&inputs).expect("plans");
        let columns: Vec<&str> = values.clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "status",
                "ended_at",
                "queue_position",
                "terminal_hooks_applied_at",
                "terminal_capacity_released_at",
                "done_payload",
                "error",
            ]
        );
        let inputs = terminal_inputs(
            AgentRunStatus::Running,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(
            plan_terminal_finalize(&inputs),
            Err(LifecycleError::NonTerminalStatus)
        );
        // Late-terminal + wrong-runner fixture pins (decided by the lock).
        let fx = fixture();
        let late = fx["finalize_run_terminal"]["late_terminal"].clone();
        assert_eq!(
            late.get("after_status").and_then(Value::as_str),
            Some("cancelled")
        );
        assert_eq!(
            late["before"].get("ended_at").and_then(Value::as_str),
            late.get("after_ended").and_then(Value::as_str)
        );
        assert_eq!(
            fx["finalize_run_terminal"]
                .get("wrong_runner_status")
                .and_then(Value::as_str),
            Some("running")
        );
    }

    // ------------------------------------------------------------------
    // failure comment
    // ------------------------------------------------------------------

    fn failure_case<'a>(fx: &'a Value, name: &str) -> &'a Value {
        fx.get("post_failure_comment")
            .and_then(Value::as_array)
            .expect("failure cases")
            .iter()
            .find(|c| c.get("case").and_then(Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("case {name}"))
    }

    #[test]
    fn failure_comment_matches_fixture() {
        let fx = fixture();
        let gold = failure_case(&fx, "normal");
        let plan = plan_failure_comment("boom detail", Uuid::nil()).expect("comment");
        assert_eq!(
            plan.comment_html,
            gold["comment"]
                .get("comment_html")
                .and_then(Value::as_str)
                .expect("html")
        );
        assert_eq!(
            gold["comment"].get("speaker_type").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            gold["comment"].get("speaker_label").and_then(Value::as_str),
            Some("Pi Dash")
        );
        assert_eq!(plan.speaker, CommentSpeaker::System);
        // Detail is stripped before render.
        let plan = plan_failure_comment("  boom detail\n", Uuid::nil()).expect("comment");
        assert!(plan.comment_html.ends_with("<pre>boom detail</pre>"));
    }

    #[test]
    fn failure_comment_suppresses_infra_prefixes() {
        assert_eq!(
            INFRA_FAILURE_DETAIL_PREFIXES,
            [
                "daemon shutdown requested",
                "agent stalled: no events for >"
            ]
        );
        for detail in [
            "daemon shutdown requested",
            "daemon shutdown requested at 2026-01-01 (context suffix)",
            "agent stalled: no events for >30s",
        ] {
            assert_eq!(
                plan_failure_comment(detail, Uuid::nil()),
                None,
                "suppressed: {detail}"
            );
        }
        let fx = fixture();
        assert_eq!(
            failure_case(&fx, "infra-prefixes-suppressed")
                .get("count")
                .and_then(Value::as_u64),
            Some(0)
        );
        // Case-sensitive: caps variant posts.
        let caps = failure_case(&fx, "prefix-case-sensitive?");
        let plan =
            plan_failure_comment("Daemon Shutdown Requested (caps)", Uuid::nil()).expect("posts");
        assert_eq!(
            plan.comment_html,
            caps.get("html").and_then(Value::as_str).expect("html")
        );
        assert_eq!(caps.get("count").and_then(Value::as_u64), Some(1));
    }

    #[test]
    fn failure_comment_empty_detail_posts_placeholder() {
        let fx = fixture();
        let gold = failure_case(&fx, "empty-detail");
        for detail in ["", "   "] {
            let plan = plan_failure_comment(detail, Uuid::nil()).expect("placeholder posts");
            assert_eq!(
                plan.comment_html,
                gold.get("html").and_then(Value::as_str).expect("html")
            );
        }
        assert_eq!(
            gold.get("html").and_then(Value::as_str),
            Some("<p><strong>Run failed.</strong> (no diagnostic detail)</p>")
        );
    }

    // ------------------------------------------------------------------
    // SQL
    // ------------------------------------------------------------------

    /// Split a `SELECT` list into `(table, column-count)` runs.
    fn select_runs(sql: &str) -> Vec<(String, usize)> {
        let list = sql
            .strip_prefix("SELECT ")
            .expect("SELECT")
            .split(" FROM ")
            .next()
            .expect("FROM");
        let mut runs: Vec<(String, usize)> = Vec::new();
        for column in list.split(", ") {
            let table = column.split('.').next().expect("table").to_owned();
            match runs.last_mut() {
                Some((last, count)) if *last == table => *count += 1,
                _ => runs.push((table, 1)),
            }
        }
        runs
    }

    #[test]
    fn live_state_sql_matches_django() {
        assert_eq!(
            live_state_by_runner_sql(),
            "SELECT \"runner_live_state\".\"runner_id\", \
             \"runner_live_state\".\"observed_run_id\", \
             \"runner_live_state\".\"last_event_at\", \
             \"runner_live_state\".\"last_event_kind\", \
             \"runner_live_state\".\"last_event_summary\", \
             \"runner_live_state\".\"agent_pid\", \
             \"runner_live_state\".\"agent_subprocess_alive\", \
             \"runner_live_state\".\"approvals_pending\", \
             \"runner_live_state\".\"usage\", \
             \"runner_live_state\".\"llm_model\", \
             \"runner_live_state\".\"turn_count\", \
             \"runner_live_state\".\"updated_at\" \
             FROM \"runner_live_state\" \
             WHERE (\"runner_live_state\".\"observed_run_id\" = $1 AND \
             \"runner_live_state\".\"runner_id\" = $2) LIMIT 1"
        );
        assert_eq!(LIVE_STATE_COLUMNS.len(), 12);
    }

    #[test]
    fn lock_sql_match_django_tails() {
        let pause = lock_run_for_pause_sql();
        assert!(pause.starts_with("SELECT \"agent_run\".\"id\", "), "prefix");
        assert_eq!(select_runs(&pause), vec![("\"agent_run\"".to_owned(), 41)]);
        assert!(
            pause.ends_with(
                " FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \
                 \"agent_run\".\"runner_id\" = $2 AND NOT (\"agent_run\".\"status\" IN \
                 ($3, $4, $5, $6, $7)) AND NOT (\"agent_run\".\"status\" = $8)) \
                 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE"
            ),
            "tail: {pause}"
        );
        let requeue = lock_run_for_requeue_sql();
        assert_eq!(
            select_runs(&requeue),
            vec![("\"agent_run\"".to_owned(), 41)]
        );
        assert!(
            requeue.ends_with(
                " FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \
                 \"agent_run\".\"runner_id\" = $2 AND NOT (\"agent_run\".\"status\" IN \
                 ($3, $4, $5, $6, $7))) ORDER BY \"agent_run\".\"created_at\" DESC \
                 LIMIT 1 FOR UPDATE"
            ),
            "tail: {requeue}"
        );
    }

    #[test]
    fn reread_sql_match_django_shapes() {
        let reread = pause_reread_sql();
        assert_eq!(
            select_runs(&reread),
            vec![
                ("\"agent_run\"".to_owned(), 41),
                ("\"issues\"".to_owned(), 34),
            ]
        );
        assert!(
            reread.ends_with(
                " FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
                 (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") WHERE \
                 \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC"
            ),
            "get() has no LIMIT: {reread}"
        );
        let drain = pause_drain_reread_sql();
        assert_eq!(
            select_runs(&drain),
            vec![
                ("\"agent_run\"".to_owned(), 41),
                ("\"issues\"".to_owned(), 34),
                ("\"projects\"".to_owned(), 46),
                ("\"states\"".to_owned(), 18),
            ]
        );
        assert!(
            drain.ends_with(
                " FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
                 (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") LEFT OUTER JOIN \
                 \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \
                 \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") WHERE \
                 \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1"
            ),
            "tail: {drain}"
        );
        let failure = failure_reread_sql();
        assert_eq!(
            select_runs(&failure),
            vec![
                ("\"agent_run\"".to_owned(), 41),
                ("\"issues\"".to_owned(), 34),
                ("\"projects\"".to_owned(), 46),
            ]
        );
        assert!(
            failure.ends_with(
                " FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
                 (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") LEFT OUTER JOIN \
                 \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") WHERE \
                 \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1"
            ),
            "tail: {failure}"
        );
    }

    #[test]
    fn ticker_and_dedupe_sql_match_django() {
        assert_eq!(
            ticker_pending_entry_sql(),
            "SELECT \"issue_agent_ticker\".\"id\" FROM \"issue_agent_ticker\" WHERE \
             (\"issue_agent_ticker\".\"deleted_at\" IS NULL AND \
             \"issue_agent_ticker\".\"enabled\" AND \"issue_agent_ticker\".\"issue_id\" = $1 AND \
             \"issue_agent_ticker\".\"pending_entry\") LIMIT 1"
        );
        assert_eq!(FIRE_TICK_TASK, "pi_dash.bgtasks.agent_ticker.fire_tick");
        assert_eq!(
            comment_dedupe_exists_sql(),
            "SELECT 1 AS \"a\" FROM \"issue_comments\" WHERE \
             (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"issue_id\" = $1 AND \
             \"issue_comments\".\"speaker_agent_run_id\" = $2 AND \
             \"issue_comments\".\"speaker_type\" = $3) LIMIT 1"
        );
    }

    #[test]
    fn pause_update_sql_orders_set_columns() {
        let full = PauseUpdatePlan {
            done_payload: json!({}),
            usage: Some(json!({"input": 1})),
            llm_model: Some("m".to_owned()),
        };
        assert_eq!(
            pause_update_sql(&full),
            "UPDATE \"agent_run\" SET \"status\" = $1, \"done_payload\" = $2, \"usage\" = $3, \
             \"llm_model\" = $4 WHERE \"agent_run\".\"id\" = $5"
        );
        let bare = PauseUpdatePlan {
            done_payload: json!({}),
            usage: None,
            llm_model: None,
        };
        assert_eq!(
            pause_update_sql(&bare),
            "UPDATE \"agent_run\" SET \"status\" = $1, \"done_payload\" = $2 \
             WHERE \"agent_run\".\"id\" = $3"
        );
        let usage_only = PauseUpdatePlan {
            done_payload: json!({}),
            usage: Some(json!({"input": 1})),
            llm_model: None,
        };
        assert_eq!(
            pause_update_sql(&usage_only),
            "UPDATE \"agent_run\" SET \"status\" = $1, \"done_payload\" = $2, \"usage\" = $3 \
             WHERE \"agent_run\".\"id\" = $4"
        );
    }

    #[test]
    fn requeue_busy_and_insert_sql_shapes() {
        assert_eq!(
            requeue_update_sql(),
            "UPDATE \"agent_run\" SET \"status\" = $1, \"runner_id\" = $2, \
             \"pinned_runner_id\" = $3, \"assigned_at\" = $4, \"queue_position\" = $5 \
             WHERE \"agent_run\".\"id\" = $6"
        );
        assert_eq!(
            parent_thread_clear_sql(),
            "UPDATE \"agent_run\" SET \"thread_id\" = $1 WHERE \"agent_run\".\"id\" = $2"
        );
        assert_eq!(
            runner_busy_update_sql(),
            "UPDATE \"runner\" SET \"status\" = $1 WHERE \"runner\".\"id\" = $2"
        );
        let comment = comment_insert_sql();
        assert_eq!(COMMENT_COLUMNS.len(), 24);
        assert!(comment.starts_with("INSERT INTO \"issue_comments\" (\"created_at\", "));
        assert!(comment.contains("\"speaker_agent_run_id\", \"edited_at\", \"parent_id\""));
        assert!(comment.ends_with(
            "VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
             $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)"
        ));
        let description = description_insert_sql();
        assert_eq!(DESCRIPTION_COLUMNS.len(), 12);
        assert!(description.starts_with("INSERT INTO \"descriptions\" (\"created_at\", "));
        assert!(description.ends_with("VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"));
        assert_eq!(
            comment_description_link_sql(),
            "UPDATE \"issue_comments\" SET \"description_id\" = $1 WHERE \"issue_comments\".\"id\" = $2"
        );
    }

    #[test]
    fn column_snapshots_match_django_meta() {
        assert_eq!(
            LIVE_STATE_COLUMNS.to_vec(),
            [
                "runner_id",
                "observed_run_id",
                "last_event_at",
                "last_event_kind",
                "last_event_summary",
                "agent_pid",
                "agent_subprocess_alive",
                "approvals_pending",
                "usage",
                "llm_model",
                "turn_count",
                "updated_at",
            ]
        );
        // The L2 read subset is covered by the full row.
        for column in pidash_db::runner_runs::live_state::READ_COLUMNS {
            assert!(
                LIVE_STATE_COLUMNS.contains(column),
                "read column {column} selected"
            );
        }
        assert_eq!(
            COMMENT_COLUMNS.to_vec(),
            [
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "comment_stripped",
                "comment_json",
                "comment_html",
                "description_id",
                "attachments",
                "labels",
                "issue_id",
                "actor_id",
                "access",
                "external_source",
                "external_id",
                "speaker_type",
                "speaker_label",
                "speaker_agent_run_id",
                "edited_at",
                "parent_id",
            ]
        );
        assert_eq!(
            DESCRIPTION_COLUMNS.to_vec(),
            [
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "workspace_id",
                "project_id",
                "description_json",
                "description_html",
                "description_binary",
                "description_stripped",
            ]
        );
    }
}
