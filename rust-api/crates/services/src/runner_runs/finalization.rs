#![forbid(unsafe_code)]

//! Terminal finalization core (D-15, stage 5).
//!
//! Pure port of `apps/api/pi_dash/runner/services/agent_run_finalization.py:1-199`:
//! `merge_done_payload` (`:30-45`), `finalize_agent_run` (`:48-86`),
//! `_publish_effects` (`:89-103`) and `apply_terminal_effects`
//! (`:105-199`). `TERMINAL_STATUSES` is the L1
//! `TERMINAL_RUN_STATUSES` (same five members); the terminal
//! orchestration/drain calls reuse the [`LifecycleEffect`]s and the
//! failure comment / scheduler hook come from the sibling modules.
//! `YIELD_KEYS` re-synced to `3869a226` (PDASHOSS01-248).
//!
//! Two transaction boundaries:
//!
//! * Finalize: lock the non-terminal row (`FOR UPDATE` + expected
//!   filters), merge `done_payload` over the locked row, write,
//!   maybe append the cloud-only terminal event, register
//!   `_publish_effects` on commit. First-writer-wins: a second
//!   concurrent finalize finds no row and reports `false`.
//! * Terminal effects: lock issue-then-run (matching issue moves and
//!   `complete_project_move_handoff` — the order that prevents a
//!   Postgres deadlock), run hooks once behind the
//!   `terminal_hooks_applied_at` marker (failure comment and
//!   scheduler hook each in their own savepoint), complete a pending
//!   handoff *after* the transaction, then release capacity
//!   at-least-once behind `terminal_capacity_released_at`.
//!
//! Fixture: FX-RUN-05 (`merge_done_payload`, `finalize_agent_run`,
//! `apply_terminal_effects` sections).
//!
//! PDASHOSS01-187: callers keep naming the flat `error_code` /
//! `error` / `refusal_category` columns, which are one `error_details`
//! JSON column now. [`plan_finalize_values`] folds them into a single
//! `error_details` clause (`split_folded_updates` +
//! `merge_error_details`), and [`finalize_update_sql`] merges that
//! patch into the locked row's bag in SQL — the same read-modify-write
//! Django runs in Python under the same `FOR UPDATE` lock.

use chrono::{DateTime, Utc};
use pidash_db::entity as orm;
use pidash_db::runner_runs::{agent_run, event, pod};
use pidash_db::tasks_ticker::models::scheduler_binding;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;
use serde_json::Value;
use uuid::Uuid;

use super::lifecycle::{has_project_move_handoff, plan_failure_comment, CommentPlan};
use super::scheduler_hook::{plan_scheduler_hook, BindingFacts, SchedulerHookPlan};
use super::{
    qualified_columns, LifecycleEffect, SetClause, SetValue, ISSUE_COLUMNS, PROJECT_COLUMNS,
    STATE_COLUMNS,
};

/// Celery wire name of the terminal-effects task (`runner/tasks.py:333`).
pub const TERMINAL_EFFECTS_TASK: &str = "runner.apply_agent_run_terminal_effects";

/// Keys `pidash run yield` writes on `done_payload`
/// (`agent_run_finalization.py:27`, `3869a226`), in tuple order.
pub const YIELD_KEYS: [&str; 4] = ["status", "note", "yielded_at", "stop_ticking"];

/// `runner` columns in Django `_meta` order (captured from
/// `Runner._meta.concrete_fields`, plus the `09738d222` pause pair):
/// the capacity re-read joins the full row. Canonical home is the
/// D-13 port; this list pins the `SELECT` text until it lands.
const RUNNER_COLUMNS: &[&str] = &[
    "id",
    "owner_id",
    "workspace_id",
    "dev_machine_id",
    "pod_id",
    "name",
    "host_label",
    "provisioning",
    "visibility",
    "refresh_token_hash",
    "refresh_token_fingerprint",
    "refresh_token_generation",
    "previous_refresh_token_hash",
    "access_token_signing_key_version",
    "enrollment_token_hash",
    "enrollment_token_fingerprint",
    "enrolled_at",
    "capabilities",
    "status",
    "os",
    "arch",
    "runner_version",
    "dev_metadata",
    "protocol_version",
    "last_heartbeat_at",
    "created_at",
    "updated_at",
    "revoked_at",
    "revoked_reason",
    "paused_at",
    "paused_by_id",
];

/// How finalization planning fails: `new_status` is not terminal
/// (`ValueError`, message kept verbatim).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FinalizeError {
    #[error("new_status must be terminal")]
    NonTerminalStatus,
}

/// `merge_done_payload` (`agent_run_finalization.py:30-45`): the
/// incoming payload wins wholesale unless the stored one yielded
/// (`yielded_at` set) and the incoming one carries no `status` of its
/// own — then the stored yield keys survive onto a copy of the
/// incoming payload (or onto `{}` for a non-dict incoming). A present
/// `status`, even null, suppresses the merge.
pub fn merge_done_payload(existing: &Value, incoming: &Value) -> Value {
    let yielded = existing
        .as_object()
        .and_then(|map| map.get("yielded_at"))
        .map(super::py_truthy)
        .unwrap_or(false);
    if !yielded {
        return incoming.clone();
    }
    let mut merged = incoming.as_object().cloned().unwrap_or_default();
    if !merged.contains_key("status") {
        if let Some(stored) = existing.as_object() {
            for key in YIELD_KEYS {
                if let Some(value) = stored.get(key) {
                    merged.insert(key.to_owned(), value.clone());
                }
            }
        }
    }
    Value::Object(merged)
}

/// Assembled finalize `SET` values (`finalize_agent_run:52-59`):
/// `status`, `ended_at`, both markers cleared, then
/// the caller extras in order (overwriting same-name base keys in
/// place), then — when the caller named any of the folded columns —
/// one trailing `error_details` clause carrying the patch (see
/// [`ERROR_DETAILS_COLUMN`]). `done_payload` still holds the *incoming* value here; the
/// executing layer runs [`apply_done_payload_merge`] against the
/// locked row before rendering the `UPDATE`.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizeValues {
    pub clauses: Vec<SetClause>,
}

/// The single column the folded flat names write
/// (`AgentRun.error_details`). Its clause value is a
/// [`SetValue::Json`] patch `{bag_key: value | null}`: a string sets
/// the key, `null` removes it (`merge_error_details` drops falsy
/// values rather than storing `""`).
pub const ERROR_DETAILS_COLUMN: &str = "error_details";

/// Python truthiness of a folded clause value, as the patch value it
/// becomes: truthy text / JSON is kept, everything falsy is `null`.
fn folded_patch_value(value: &SetValue) -> Value {
    match value {
        SetValue::Text(text) if !text.is_empty() => Value::String(text.clone()),
        SetValue::Json(json) if super::py_truthy(json) => json.clone(),
        _ => Value::Null,
    }
}

/// `split_folded_updates` + the patch half of `merge_error_details`:
/// pull the flat folded clauses out (dict order kept for a repeated
/// name: the last write wins) and append one `error_details` patch
/// clause, as Django assigns `values["error_details"]` after the pop.
fn fold_error_details(clauses: &mut Vec<SetClause>) {
    let mut patch = serde_json::Map::new();
    let mut folded = false;
    clauses.retain(|clause| match agent_run::folded_key(clause.column) {
        Some(key) => {
            patch.insert(key.to_owned(), folded_patch_value(&clause.value));
            folded = true;
            false
        }
        None => true,
    });
    if folded {
        clauses.push(SetClause {
            column: ERROR_DETAILS_COLUMN,
            value: SetValue::Json(Value::Object(patch)),
        });
    }
}

/// Assemble the finalize values. `extras` are the caller `updates` in
/// dict order. Non-terminal `new_status` fails, as the source raises
/// `ValueError` before touching the database.
pub fn plan_finalize_values(
    new_status: AgentRunStatus,
    extras: &[(&'static str, SetValue)],
) -> Result<FinalizeValues, FinalizeError> {
    if !new_status.is_terminal() {
        return Err(FinalizeError::NonTerminalStatus);
    }
    let mut clauses = vec![
        SetClause {
            column: "status",
            value: SetValue::Text(new_status.value().to_owned()),
        },
        SetClause {
            column: "ended_at",
            value: SetValue::Now,
        },
        SetClause {
            column: "terminal_hooks_applied_at",
            value: SetValue::Null,
        },
        SetClause {
            column: "terminal_capacity_released_at",
            value: SetValue::Null,
        },
    ];
    for (column, value) in extras {
        match clauses.iter_mut().find(|clause| clause.column == *column) {
            Some(existing) => existing.value = value.clone(),
            None => clauses.push(SetClause {
                column,
                value: value.clone(),
            }),
        }
    }
    fold_error_details(&mut clauses);
    Ok(FinalizeValues { clauses })
}

/// Merge the locked row's `done_payload` into the values
/// (`finalize_agent_run:69-70`): only when the values carry a
/// `done_payload` key at all.
pub fn apply_done_payload_merge(values: &mut FinalizeValues, stored_done_payload: &Value) {
    if let Some(clause) = values
        .clauses
        .iter_mut()
        .find(|clause| clause.column == "done_payload")
    {
        let incoming = match &clause.value {
            SetValue::Json(payload) => payload.clone(),
            _ => Value::Null,
        };
        clause.value = SetValue::Json(merge_done_payload(stored_done_payload, &incoming));
    }
}

/// The terminal event's `error_code` (`finalize_agent_run:80-83`):
/// `folded.get("error_code") or ""` — the `code` the caller passed,
/// read back off the `error_details` patch. Every real caller passes
/// a string or nothing; a non-string value (unreachable from them)
/// reads as `""`.
pub fn finalize_error_code(values: &FinalizeValues) -> String {
    values
        .clauses
        .iter()
        .find(|clause| clause.column == ERROR_DETAILS_COLUMN)
        .and_then(|clause| match &clause.value {
            SetValue::Json(patch) => patch
                .get(agent_run::ERROR_DETAILS_CODE)
                .and_then(Value::as_str)
                .map(str::to_owned),
            _ => None,
        })
        .unwrap_or_default()
}

/// The folded `error_details` patch of a finalize plan (`None` when
/// the caller named none of the folded columns), for executors that
/// bind the plan positionally.
pub fn error_details_patch(values: &FinalizeValues) -> Option<&Value> {
    values
        .clauses
        .iter()
        .find(|clause| clause.column == ERROR_DETAILS_COLUMN)
        .and_then(|clause| match &clause.value {
            SetValue::Json(patch) => Some(patch),
            _ => None,
        })
}

/// Finalize-lock predicate (`finalize_agent_run:61-68`): the row must
/// be non-terminal and satisfy the expected filters. The SQL decides
/// for real (`FOR UPDATE` + predicates → row or `None`); this pins
/// the matrix for unit tests.
pub fn finalize_lock_passes(
    row_status: AgentRunStatus,
    row_runner_id: Option<Uuid>,
    expected_runner_id: Option<Uuid>,
    expected_status: Option<AgentRunStatus>,
) -> bool {
    if row_status.is_terminal() {
        return false;
    }
    if expected_runner_id.is_some() && row_runner_id != expected_runner_id {
        return false;
    }
    if expected_status.is_some() && Some(row_status) != expected_status {
        return false;
    }
    true
}

/// Effects-lock predicate (`apply_terminal_effects:126-133`): the row
/// must be terminal, else the function returns `false` with no writes.
pub fn effects_lock_passes(row_status: AgentRunStatus) -> bool {
    row_status.is_terminal()
}

/// Finalize lock (`finalize_agent_run:61-66`): full `agent_run` row by
/// id, excluding terminal states, plus the expected-runner/status
/// predicates when the caller passes them. `ORDER BY created_at DESC
/// LIMIT 1 FOR UPDATE`. Params: `$1` run id, `$2..$6` the terminal
/// values (**tuple order** — Django iterates a `set` here, so its own
/// text varies per process; the semantics are order-free), then the
/// runner id / status when filtered.
pub fn lock_run_for_finalize_sql(with_runner: bool, with_status: bool) -> String {
    let mut where_parts = vec![
        "\"agent_run\".\"id\" = $1".to_owned(),
        "NOT (\"agent_run\".\"status\" IN ($2, $3, $4, $5, $6))".to_owned(),
    ];
    let mut next = 7;
    if with_runner {
        where_parts.push(format!("\"agent_run\".\"runner_id\" = ${next}"));
        next += 1;
    }
    if with_status {
        where_parts.push(format!("\"agent_run\".\"status\" = ${next}"));
    }
    format!(
        "SELECT {} FROM \"agent_run\" WHERE ({}) ORDER BY \
         \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE",
        qualified_columns("agent_run", agent_run::COLUMNS),
        where_parts.join(" AND ")
    )
}

/// Finalize `UPDATE` (`finalize_agent_run:71`): `SET` in values order,
/// `WHERE` on the locked primary key. Params follow the clause order;
/// the id binds last. The `error_details` clause merges its patch
/// into the stored bag ([`error_details_merge_sql`]).
pub fn finalize_update_sql(values: &FinalizeValues) -> String {
    let set = values
        .clauses
        .iter()
        .enumerate()
        .map(|(index, clause)| {
            let param = format!("${}", index + 1);
            if clause.column == ERROR_DETAILS_COLUMN {
                format!(
                    "\"{}\" = {}",
                    clause.column,
                    error_details_merge_sql(&param)
                )
            } else {
                format!("\"{}\" = {param}", clause.column)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"agent_run\" SET {set} WHERE \"agent_run\".\"id\" = ${}",
        values.clauses.len() + 1
    )
}

/// `merge_error_details(run.error_details, **folded)` as SQL over the
/// row being updated: the stored bag (reset to `{}` when not an
/// object), minus every key the patch names, plus the patch's
/// non-`null` keys. `param` is the patch placeholder (`$n`), bound as
/// `jsonb`.
pub fn error_details_merge_sql(param: &str) -> String {
    format!(
        "((CASE WHEN jsonb_typeof(\"agent_run\".\"error_details\") = 'object' \
         THEN \"agent_run\".\"error_details\" ELSE '{{}}'::jsonb END) \
         - ARRAY(SELECT jsonb_object_keys({param}::jsonb))) || jsonb_strip_nulls({param}::jsonb)"
    )
}

/// Terminal-event existence (`finalize_agent_run:74`): cloud-only
/// guard. Params: `$1` run id, `$2` kind (`terminal`).
pub fn terminal_event_exists_sql() -> &'static str {
    "SELECT 1 AS \"a\" FROM \"agent_run_event\" WHERE \
     (\"agent_run_event\".\"agent_run_id\" = $1 AND \"agent_run_event\".\"kind\" = $2) LIMIT 1"
}

/// Terminal-event sequence (`finalize_agent_run:77`): current max, or
/// no row. Param: `$1` run id.
pub fn terminal_event_max_seq_sql() -> &'static str {
    "SELECT \"agent_run_event\".\"seq\" FROM \"agent_run_event\" WHERE \
     \"agent_run_event\".\"agent_run_id\" = $1 ORDER BY \"agent_run_event\".\"seq\" DESC LIMIT 1"
}

/// Terminal-event `INSERT` (`finalize_agent_run:79-84`): the id is a
/// `BigAutoField` (default + `RETURNING`); `created_at` binds `now()`.
/// Params: `$1` run id, `$2` seq, `$3` kind, `$4` payload, `$5` now.
pub fn terminal_event_insert_sql() -> String {
    let columns: Vec<String> = event::COLUMNS
        .iter()
        .filter(|c| **c != "id")
        .map(|c| format!("\"{c}\""))
        .collect();
    let params = (1..=columns.len())
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING \"{}\".\"id\"",
        event::TABLE,
        columns.join(", "),
        params,
        event::TABLE
    )
}

/// The terminal event row (`finalize_agent_run:76-84`): `seq` is max
/// + 1 (or 1), `kind` is `terminal`, payload `{"status",
/// "error_code"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalEventPlan {
    pub seq: i32,
    pub payload: Value,
}

/// Plan the terminal event from the current max seq (or `None`).
pub fn plan_terminal_event(
    max_seq: Option<i32>,
    new_status: AgentRunStatus,
    error_code: &str,
) -> TerminalEventPlan {
    TerminalEventPlan {
        seq: max_seq.unwrap_or(0) + 1,
        payload: terminal_event_payload(new_status, error_code),
    }
}

/// Terminal-event payload: `{"status": <value>, "error_code": <code>}`.
pub fn terminal_event_payload(new_status: AgentRunStatus, error_code: &str) -> Value {
    serde_json::json!({"status": new_status.value(), "error_code": error_code})
}

/// Terminal-event `INSERT` performer ([`terminal_event_insert_sql`]):
/// append the planned cloud-only event for one finalized run.
///
/// `plan` is the [`plan_terminal_event`] row (`seq`, `{"status",
/// "error_code"}` payload); `kind` binds `terminal`, `created_at`
/// binds the caller's own `now` (each driver stamps its own clock,
/// never hoisted across legs). The existence guard and max-seq read
/// stay driver-side. Runs on a SeaORM connection via
/// [`insert_terminal_event_orm`]; `sqlx::Error` is kept via
/// `orm_tx::into_sqlx` so callers' classification is unchanged.
pub async fn insert_terminal_event(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    plan: TerminalEventPlan,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    insert_terminal_event_orm(db, run_id, plan, now)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)
}

/// `_publish_effects` (`agent_run_finalization.py:89-103`): the Celery
/// emit first, then the inline terminal-effects run — each isolated,
/// in order. The executing layer logs `failed to publish terminal
/// effects for run %s` / `failed to apply terminal effects for run
/// %s` (exception, `pi_dash.runner.services.agent_run_finalization`)
/// on failure.
pub fn plan_publish_effects(run_id: Uuid) -> [LifecycleEffect; 2] {
    [
        LifecycleEffect::PublishTerminalEffects { run_id },
        LifecycleEffect::ApplyTerminalEffectsInline { run_id },
    ]
}

/// Work-item pre-read (`apply_terminal_effects:121`): single column,
/// ordered (model ordering) + `LIMIT 1`. Param: `$1` run id.
pub fn select_run_work_item_id_sql() -> &'static str {
    "SELECT \"agent_run\".\"work_item_id\" FROM \"agent_run\" WHERE \
     \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1"
}

/// Issue lock (`apply_terminal_effects:125`): `all_objects` (no
/// soft-delete filter) + `FOR UPDATE OF "issues"`, first in the
/// issue→run lock order. Param: `$1` issue id.
pub fn lock_issue_sql() -> String {
    format!(
        "SELECT {} FROM \"issues\" WHERE \"issues\".\"id\" = $1 ORDER BY \
         \"issues\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"issues\"",
        qualified_columns("issues", ISSUE_COLUMNS)
    )
}

/// Effects run lock+fetch (`apply_terminal_effects:126-131`):
/// `select_related("work_item", "work_item__state",
/// "work_item__project", "scheduler_binding").filter(pk,
/// status__in=terminal).first()` with `FOR UPDATE OF "agent_run"` —
/// 41 + 34 + 46 + 18 + 22 columns. Param: `$1` run id, `$2..$6` the
/// terminal values in tuple order (same set-ordering quirk as the
/// finalize lock).
pub fn lock_run_for_effects_sql() -> String {
    format!(
        "SELECT {}, {}, {}, {}, {} FROM \"agent_run\" LEFT OUTER JOIN \"issues\" ON \
         (\"agent_run\".\"work_item_id\" = \"issues\".\"id\") LEFT OUTER JOIN \
         \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \
         \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") LEFT OUTER JOIN \
         \"scheduler_bindings\" ON (\"agent_run\".\"scheduler_binding_id\" = \
         \"scheduler_bindings\".\"id\") WHERE (\"agent_run\".\"id\" = $1 AND \
         \"agent_run\".\"status\" IN ($2, $3, $4, $5, $6)) ORDER BY \
         \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"agent_run\"",
        qualified_columns("agent_run", agent_run::COLUMNS),
        qualified_columns("issues", ISSUE_COLUMNS),
        qualified_columns("projects", PROJECT_COLUMNS),
        qualified_columns("states", STATE_COLUMNS),
        qualified_columns("scheduler_bindings", scheduler_binding::COLUMNS)
    )
}

/// Hooks-marker write (`apply_terminal_effects:166-167`): single
/// column. Params: `$1` now, `$2` run id. Zero rows raise
/// `DatabaseError`.
pub fn update_hooks_marker_sql() -> &'static str {
    "UPDATE \"agent_run\" SET \"terminal_hooks_applied_at\" = $1 WHERE \"agent_run\".\"id\" = $2"
}

/// Capacity re-read (`apply_terminal_effects:183`):
/// `select_related("runner", "pod").get(pk)` — 40 + 10 + 31 columns,
/// `.get()` so no `LIMIT`. A missing row raises `DoesNotExist` (no
/// guard in the source); the executing layer propagates it. Param:
/// `$1` run id.
pub fn select_run_for_capacity_sql() -> String {
    format!(
        "SELECT {}, {}, {} FROM \"agent_run\" INNER JOIN \"pod\" ON \
         (\"agent_run\".\"pod_id\" = \"pod\".\"id\") LEFT OUTER JOIN \"runner\" ON \
         (\"agent_run\".\"runner_id\" = \"runner\".\"id\") WHERE \"agent_run\".\"id\" = $1 \
         ORDER BY \"agent_run\".\"created_at\" DESC",
        qualified_columns("agent_run", agent_run::COLUMNS),
        qualified_columns("pod", pod::COLUMNS),
        qualified_columns("runner", RUNNER_COLUMNS)
    )
}

/// Capacity-marker write (`apply_terminal_effects:196-198`):
/// conditional on the marker still being null (reconciliation race).
/// Params: `$1` now, `$2` run id.
pub fn update_capacity_marker_sql() -> &'static str {
    "UPDATE \"agent_run\" SET \"terminal_capacity_released_at\" = $1 WHERE \
     (\"agent_run\".\"id\" = $2 AND \"agent_run\".\"terminal_capacity_released_at\" IS NULL)"
}

/// One SQL operation in the terminal-effects statement plan, in
/// execution order. Savepoints bracket the two best-effort hooks;
/// each hook's failure rolls back to its savepoint and logs, never
/// poisoning the enclosing transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectsSql {
    SelectWorkItemId,
    LockIssue,
    LockRunForEffects,
    FailureCommentSavepoint,
    FailureCommentReread,
    FailureCommentDedupeCheck,
    FailureCommentInserts,
    FailureCommentRelease,
    SchedulerSavepoint,
    SchedulerUpdate,
    SchedulerRelease,
    UpdateHooksMarker,
    SelectRunForCapacity,
    UpdateCapacityMarker,
}

/// The hooks branch of [`TerminalEffectsPlan`]: failure comment
/// (`FAILED` without a handoff, savepointed), post-run orchestration
/// (skipped on handoff), scheduler hook (savepointed, when bound).
#[derive(Debug, Clone, PartialEq)]
pub struct HooksPlan {
    pub failure_comment: Option<CommentPlan>,
    pub orchestration: bool,
    pub scheduler: Option<SchedulerHookPlan>,
}

/// The capacity branch: cloud runs dispatch waiting work for the
/// workspace; local runs drain runner and/or pod when set. Drains and
/// dispatch run *before* the marker write, unisolated — a failure
/// leaves the marker unset for the reconciler.
#[derive(Debug, Clone, PartialEq)]
pub enum CapacityPlan {
    Cloud {
        workspace_id: Uuid,
    },
    Local {
        runner_id: Option<Uuid>,
        pod_id: Option<Uuid>,
    },
}

impl CapacityPlan {
    /// The capacity effects in order: dispatch, or runner drain then
    /// pod drain (each when its id is set).
    pub fn effects(&self) -> Vec<LifecycleEffect> {
        match self {
            CapacityPlan::Cloud { workspace_id } => vec![LifecycleEffect::DispatchWaiting {
                workspace_id: *workspace_id,
            }],
            CapacityPlan::Local { runner_id, pod_id } => {
                let mut effects = Vec::new();
                if let Some(runner_id) = runner_id {
                    effects.push(LifecycleEffect::DrainRunner {
                        runner_id: *runner_id,
                    });
                }
                if let Some(pod_id) = pod_id {
                    effects.push(LifecycleEffect::DrainPod { pod_id: *pod_id });
                }
                effects
            }
        }
    }
}

/// The terminal-effects plan (`apply_terminal_effects:105-199`):
/// optional hooks branch (skipped when already applied), the handoff
/// flag (completed *after* the hooks transaction), the optional
/// capacity branch (skipped when already released), and the ordered
/// SQL statement plan pinning lock order + savepoint structure.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalEffectsPlan {
    pub hooks: Option<HooksPlan>,
    pub handoff_after_txn: bool,
    pub capacity: Option<CapacityPlan>,
    pub statements: Vec<EffectsSql>,
}

/// Outcome of [`plan_terminal_effects`]: `NotTerminal` (locked fetch
/// found nothing → return `false`, no writes) or the plan.
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalEffectsOutcome {
    NotTerminal,
    Applied(Box<TerminalEffectsPlan>),
}

/// Facts [`plan_terminal_effects`] reads: status, config, error text,
/// refusal category, binding and `hooks_applied` off the locked run
/// row (+ the work-item pre-read); `capacity_released`, executor,
/// assignment and workspace off the *post-transaction* capacity
/// re-read (`select_run_for_capacity_sql`), which the source fetches
/// fresh after the hooks transaction commits.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalEffectsInputs<'a> {
    pub run_id: Uuid,
    pub status: AgentRunStatus,
    pub run_config: &'a Value,
    pub error: &'a str,
    pub refusal_category: &'a str,
    pub scheduler_binding: Option<BindingFacts>,
    pub hooks_applied: bool,
    pub capacity_released: bool,
    pub executor_kind: AgentExecutorKind,
    pub runner_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub work_item_present: bool,
}

/// Plan `apply_terminal_effects`. Statement sequences per shape (the
/// outer-transaction savepoint pair belongs to the executing layer):
/// completed-local-issue, failed-local-issue (+ comment savepoint),
/// handoff (no orchestration; handoff after commit), scheduler-bound
/// (+ hook savepoint), cloud (dispatch instead of drains),
/// hooks-applied (capacity only), both-markers (capacity re-read
/// only — the `.get()` still runs), non-terminal (work-item read +
/// lock attempt only).
pub fn plan_terminal_effects(inputs: &TerminalEffectsInputs) -> TerminalEffectsOutcome {
    let mut statements = vec![EffectsSql::SelectWorkItemId];
    if inputs.work_item_present {
        statements.push(EffectsSql::LockIssue);
    }
    statements.push(EffectsSql::LockRunForEffects);
    if !effects_lock_passes(inputs.status) {
        return TerminalEffectsOutcome::NotTerminal;
    }

    let handoff = has_project_move_handoff(inputs.run_config);
    let hooks = if inputs.hooks_applied {
        None
    } else {
        let failure_comment = if inputs.status == AgentRunStatus::Failed && !handoff {
            // The savepoint pair always runs around the hook call;
            // the reads only when the comment is not suppressed
            // (suppression returns before any SQL). The inserts are
            // additionally runtime-gated: a dedupe hit or a missing
            // work item means silence after the reads.
            statements.push(EffectsSql::FailureCommentSavepoint);
            let comment = plan_failure_comment(inputs.error, inputs.run_id);
            if comment.is_some() {
                statements.push(EffectsSql::FailureCommentReread);
                statements.push(EffectsSql::FailureCommentDedupeCheck);
                statements.push(EffectsSql::FailureCommentInserts);
            }
            statements.push(EffectsSql::FailureCommentRelease);
            comment
        } else {
            None
        };
        let scheduler = inputs.scheduler_binding.as_ref().map(|binding| {
            statements.push(EffectsSql::SchedulerSavepoint);
            let plan = plan_scheduler_hook(
                Some(binding),
                inputs.status,
                inputs.refusal_category,
                inputs.error,
            );
            // The savepoint pair always runs around the hook call; the
            // `UPDATE` only when the hook rewrites `last_error`.
            if !matches!(plan, SchedulerHookPlan::Noop) {
                statements.push(EffectsSql::SchedulerUpdate);
            }
            statements.push(EffectsSql::SchedulerRelease);
            plan
        });
        statements.push(EffectsSql::UpdateHooksMarker);
        Some(HooksPlan {
            failure_comment,
            orchestration: !handoff,
            scheduler,
        })
    };

    // The capacity re-read (`.get()`) runs unconditionally once the
    // run proved terminal — even when both markers are already set.
    statements.push(EffectsSql::SelectRunForCapacity);
    let capacity = if inputs.capacity_released {
        None
    } else {
        statements.push(EffectsSql::UpdateCapacityMarker);
        Some(if inputs.executor_kind == AgentExecutorKind::CloudAgent {
            CapacityPlan::Cloud {
                workspace_id: inputs.workspace_id,
            }
        } else {
            CapacityPlan::Local {
                runner_id: inputs.runner_id,
                pod_id: inputs.pod_id,
            }
        })
    };

    let handoff_after_txn = handoff && hooks.is_some();
    TerminalEffectsOutcome::Applied(Box::new(TerminalEffectsPlan {
        hooks,
        handoff_after_txn,
        capacity,
        statements,
    }))
}

// ---------------------------------------------------------------------------
// SeaORM finalize kernel
// ---------------------------------------------------------------------------
//
// The statements above as SeaORM / sea-query builders, for drivers that run
// on a pool (`sea_orm_connection`) or an open sqlx transaction
// (`pidash_db::orm_tx::TxConn`). Same predicates, order, lock and binds as
// the text twins; the full row decodes by name into the entity `Model`.

/// The finalize lock statement ([`lock_run_for_finalize_sql`]): the
/// non-terminal row by id, plus the expected runner / status when given,
/// `ORDER BY created_at DESC LIMIT 1 FOR UPDATE`.
pub fn lock_run_for_finalize_select(
    run_id: Uuid,
    expected_runner_id: Option<Uuid>,
    expected_status: Option<AgentRunStatus>,
) -> sea_orm::Select<orm::agent_run::Entity> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    let mut select = Entity::find().filter(Column::Id.eq(run_id)).filter(
        Column::Status.is_not_in(
            pidash_types::runner_runs::TERMINAL_RUN_STATUSES
                .iter()
                .map(AgentRunStatus::value),
        ),
    );
    if let Some(runner_id) = expected_runner_id {
        select = select.filter(Column::RunnerId.eq(runner_id));
    }
    if let Some(status) = expected_status {
        select = select.filter(Column::Status.eq(status.value()));
    }
    select
        .order_by_desc(Column::CreatedAt)
        .limit(1)
        .lock_exclusive()
}

/// Run the finalize lock: the locked full row, or `None` when the run is
/// terminal already or misses an expected filter (first-writer-wins).
pub async fn lock_run_for_finalize(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    expected_runner_id: Option<Uuid>,
    expected_status: Option<AgentRunStatus>,
) -> Result<Option<orm::agent_run::Model>, sea_orm::DbErr> {
    lock_run_for_finalize_select(run_id, expected_runner_id, expected_status)
        .one(db)
        .await
}

/// `merge_error_details` over the row being updated
/// ([`error_details_merge_sql`]): the stored bag (`{}` when not an
/// object), minus every key the patch names, `||` the patch with its
/// nulls stripped.
pub fn error_details_merge_expr(patch: &Value) -> sea_orm::sea_query::SimpleExpr {
    use orm::agent_run::Column;
    use sea_orm::sea_query::{Alias, BinOper, Expr, Func, SimpleExpr};
    let stored = Expr::col((orm::agent_run::Entity, Column::ErrorDetails));
    let mut bag: SimpleExpr = Expr::case(
        SimpleExpr::from(Func::cust(Alias::new("jsonb_typeof")).arg(stored.clone())).eq("object"),
        stored,
    )
    .finally(Expr::val(Value::Object(Default::default())).cast_as(Alias::new("jsonb")))
    .into();
    if let Some(map) = patch.as_object() {
        for key in map.keys() {
            bag = bag.binary(BinOper::Sub, Expr::val(key.clone()));
        }
    }
    bag.binary(
        sea_orm::sea_query::extension::postgres::PgBinOper::Concatenate,
        SimpleExpr::from(
            Func::cust(Alias::new("jsonb_strip_nulls"))
                .arg(Expr::val(patch.clone()).cast_as(Alias::new("jsonb"))),
        ),
    )
}

/// The finalize `UPDATE` statement ([`finalize_update_sql`]): `SET` in
/// clause order on the locked primary key. `Now` clauses take `now`,
/// `Null` clauses write SQL `NULL`, the `error_details` patch merges into
/// the stored bag.
pub fn finalize_update_statement(
    values: &FinalizeValues,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<sea_orm::UpdateMany<orm::agent_run::Entity>, sea_orm::DbErr> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::sea_query::{Expr, Keyword, SimpleExpr};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let mut update = Entity::update_many();
    for clause in &values.clauses {
        let column: Column = clause.column.parse().map_err(|_| {
            sea_orm::DbErr::Custom(format!("unknown agent_run column {}", clause.column))
        })?;
        let expr: SimpleExpr = match (&clause.value, clause.column) {
            (SetValue::Json(patch), ERROR_DETAILS_COLUMN) => error_details_merge_expr(patch),
            (SetValue::Null, _) => SimpleExpr::Keyword(Keyword::Null),
            (SetValue::Now, _) => Expr::val(now).into(),
            (SetValue::Text(text), _) => Expr::val(text.clone()).into(),
            (SetValue::Json(json), _) => Expr::val(json.clone()).into(),
        };
        update = update.col_expr(column, expr);
    }
    Ok(update.filter(Column::Id.eq(run_id)))
}

/// Run the finalize `UPDATE`; returns the rows written.
pub async fn finalize_update(
    db: &impl sea_orm::ConnectionTrait,
    values: &FinalizeValues,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sea_orm::DbErr> {
    Ok(finalize_update_statement(values, run_id, now)?
        .exec(db)
        .await?
        .rows_affected)
}

/// Terminal-event existence ([`terminal_event_exists_sql`]).
pub async fn terminal_event_exists(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    use orm::agent_run_event::{Column, Entity};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    Ok(Entity::find()
        .select_only()
        .column(Column::Id)
        .filter(Column::AgentRunId.eq(run_id))
        .filter(Column::Kind.eq(TERMINAL_EVENT_KIND))
        .into_tuple::<i64>()
        .one(db)
        .await?
        .is_some())
}

/// Terminal-event max sequence ([`terminal_event_max_seq_sql`]): the
/// highest `seq` for the run, or `None` without events.
pub async fn terminal_event_max_seq(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
) -> Result<Option<i32>, sea_orm::DbErr> {
    use orm::agent_run_event::{Column, Entity};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    Entity::find()
        .select_only()
        .column(Column::Seq)
        .filter(Column::AgentRunId.eq(run_id))
        .order_by_desc(Column::Seq)
        .into_tuple::<i32>()
        .one(db)
        .await
}

/// Terminal-event `INSERT` ([`insert_terminal_event`]) on SeaORM: the
/// planned row, `kind = terminal`, `created_at = now`; the id is the
/// database default.
pub async fn insert_terminal_event_orm(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    plan: TerminalEventPlan,
    now: DateTime<Utc>,
) -> Result<(), sea_orm::DbErr> {
    use orm::agent_run_event::{ActiveModel, Entity};
    use sea_orm::{EntityTrait, NotSet, Set};
    Entity::insert(ActiveModel {
        id: NotSet,
        seq: Set(plan.seq),
        kind: Set(TERMINAL_EVENT_KIND.to_owned()),
        payload: Set(plan.payload),
        created_at: Set(now.into()),
        agent_run_id: Set(run_id),
    })
    .exec_without_returning(db)
    .await
    .map(drop)
}

/// `finalize_agent_run:73-84` for a cloud run: append the terminal event
/// unless one exists (exists check, max-seq read, insert: the same three
/// statements, in order).
pub async fn append_terminal_event_once(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    new_status: AgentRunStatus,
    error_code: &str,
    now: DateTime<Utc>,
) -> Result<(), sea_orm::DbErr> {
    if terminal_event_exists(db, run_id).await? {
        return Ok(());
    }
    let max_seq = terminal_event_max_seq(db, run_id).await?;
    let plan = plan_terminal_event(max_seq, new_status, error_code);
    insert_terminal_event_orm(db, run_id, plan, now).await
}

/// The terminal event's `kind`.
pub const TERMINAL_EVENT_KIND: &str = "terminal";

// ---------------------------------------------------------------------------
// SeaORM terminal-effects kernel
// ---------------------------------------------------------------------------
//
// The `apply_terminal_effects` statements as SeaORM builders. Each one
// reads only the columns its planner consumes; the `select_related`
// legs Django adds to the locked fetch (issues, projects, states) and
// the capacity re-read (runner) are 1:1 primary-key joins that cannot
// change the row set, so they are not re-joined. The row locks, the
// ordering, the `LIMIT`s and the predicates are the text twins'.

/// Work-item pre-read ([`select_run_work_item_id_sql`]): `Some(work_item)`
/// when the run row exists (the id itself may be `None`), `None` without
/// a row.
pub fn select_run_work_item_id_select(
    run_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Option<Uuid>>> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    Entity::find()
        .select_only()
        .column(Column::WorkItemId)
        .filter(Column::Id.eq(run_id))
        .order_by_desc(Column::CreatedAt)
        .limit(1)
        .into_tuple::<Option<Uuid>>()
}

/// Run [`select_run_work_item_id_select`].
pub async fn select_run_work_item_id(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
) -> Result<Option<Option<Uuid>>, sea_orm::DbErr> {
    select_run_work_item_id_select(run_id).one(db).await
}

/// Issue lock ([`lock_issue_sql`]): the row by id with no soft-delete
/// filter, `ORDER BY created_at DESC LIMIT 1 FOR UPDATE OF "issues"` —
/// first in the issue→run lock order. Only the id is read.
pub fn lock_issue_select(issue_id: Uuid) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Uuid>> {
    use orm::issues::{Column, Entity};
    use sea_orm::sea_query::LockType;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait};
    let mut select = Entity::find()
        .select_only()
        .column(Column::Id)
        .filter(Column::Id.eq(issue_id))
        .order_by_desc(Column::CreatedAt)
        .limit(1);
    QueryTrait::query(&mut select).lock_with_tables(LockType::Update, [Entity]);
    select.into_tuple::<Uuid>()
}

/// Run [`lock_issue_select`]; the row (if any) stays locked for the
/// transaction.
pub async fn lock_issue(
    db: &impl sea_orm::ConnectionTrait,
    issue_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    lock_issue_select(issue_id).one(db).await.map(drop)
}

/// The locked effects row ([`lock_run_for_effects_sql`]): the planner
/// facts off `agent_run` plus the bound scheduler binding's id and
/// `last_error` (both `None` without a binding row — `LEFT JOIN`).
#[derive(Debug, Clone, PartialEq, sea_orm::FromQueryResult)]
pub struct EffectsLockRow {
    pub status: String,
    pub run_config: Value,
    pub error_details: Value,
    pub refusal_category: Option<String>,
    pub scheduler_binding_id: Option<Uuid>,
    pub terminal_hooks_applied_at: Option<DateTime<Utc>>,
    pub terminal_capacity_released_at: Option<DateTime<Utc>>,
    pub executor_kind: String,
    pub runner_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub workspace_id: Uuid,
    pub binding_id: Option<Uuid>,
    pub binding_last_error: Option<String>,
}

impl EffectsLockRow {
    /// `AgentRun.error`: the `message` key of the bag, `""` when absent.
    pub fn error(&self) -> &str {
        agent_run::error_details_text(&self.error_details, agent_run::ERROR_DETAILS_MESSAGE)
    }

    /// The generated `refusal_category` (`COALESCE(.., '')`, never NULL
    /// on the real table).
    pub fn refusal_category(&self) -> &str {
        self.refusal_category.as_deref().unwrap_or("")
    }

    /// The scheduler binding facts for the hook, when the join found
    /// a row.
    pub fn binding_facts(&self) -> Option<super::scheduler_hook::BindingFacts> {
        self.binding_id
            .map(|id| super::scheduler_hook::BindingFacts {
                id,
                last_error: self.binding_last_error.clone().unwrap_or_default(),
            })
    }
}

/// `agent_run.scheduler_binding_id -> scheduler_bindings.id` (trimmed
/// from the entity, which keeps only the `last_run_id` edge).
fn scheduler_binding_rel() -> sea_orm::RelationDef {
    use sea_orm::EntityTrait;
    orm::agent_run::Entity::belongs_to(orm::scheduler_bindings::Entity)
        .from(orm::agent_run::Column::SchedulerBindingId)
        .to(orm::scheduler_bindings::Column::Id)
        .into()
}

/// `agent_run.pod_id -> pod.id` (trimmed from the entity).
fn pod_rel() -> sea_orm::RelationDef {
    use sea_orm::EntityTrait;
    orm::agent_run::Entity::belongs_to(orm::pod::Entity)
        .from(orm::agent_run::Column::PodId)
        .to(orm::pod::Column::Id)
        .into()
}

/// Effects lock ([`lock_run_for_effects_sql`]): the terminal row by id,
/// `LEFT JOIN scheduler_bindings`, `ORDER BY created_at DESC LIMIT 1
/// FOR UPDATE OF "agent_run"`.
pub fn lock_run_for_effects_select(
    run_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectModel<EffectsLockRow>> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::sea_query::LockType;
    use sea_orm::{
        ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    };
    let mut select = Entity::find()
        .select_only()
        .columns([
            Column::Status,
            Column::RunConfig,
            Column::ErrorDetails,
            Column::RefusalCategory,
            Column::SchedulerBindingId,
            Column::TerminalHooksAppliedAt,
            Column::TerminalCapacityReleasedAt,
            Column::ExecutorKind,
            Column::RunnerId,
            Column::PodId,
            Column::WorkspaceId,
        ])
        .column_as(orm::scheduler_bindings::Column::Id, "binding_id")
        .column_as(
            orm::scheduler_bindings::Column::LastError,
            "binding_last_error",
        )
        .join(JoinType::LeftJoin, scheduler_binding_rel())
        .filter(Column::Id.eq(run_id))
        .filter(
            Column::Status.is_in(
                pidash_types::runner_runs::TERMINAL_RUN_STATUSES
                    .iter()
                    .map(AgentRunStatus::value),
            ),
        )
        .order_by_desc(Column::CreatedAt)
        .limit(1);
    QueryTrait::query(&mut select).lock_with_tables(LockType::Update, [Entity]);
    select.into_model::<EffectsLockRow>()
}

/// Run [`lock_run_for_effects_select`]: `None` when the run is not
/// terminal (or gone).
pub async fn lock_run_for_effects(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
) -> Result<Option<EffectsLockRow>, sea_orm::DbErr> {
    lock_run_for_effects_select(run_id).one(db).await
}

/// Hooks-marker write ([`update_hooks_marker_sql`]).
pub fn update_hooks_marker_statement(
    run_id: Uuid,
    now: DateTime<Utc>,
) -> sea_orm::UpdateMany<orm::agent_run::Entity> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    Entity::update_many()
        .col_expr(Column::TerminalHooksAppliedAt, Expr::value(now))
        .filter(Column::Id.eq(run_id))
}

/// Run [`update_hooks_marker_statement`]; returns the rows written (zero
/// is Django's `DatabaseError` on a vanished row — the caller decides).
pub async fn update_hooks_marker(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sea_orm::DbErr> {
    Ok(update_hooks_marker_statement(run_id, now)
        .exec(db)
        .await?
        .rows_affected)
}

/// The capacity re-read ([`select_run_for_capacity_sql`]): the release
/// marker and the dispatch/drain facts.
#[derive(Debug, Clone, PartialEq, Eq, sea_orm::FromQueryResult)]
pub struct CapacityRow {
    pub terminal_capacity_released_at: Option<DateTime<Utc>>,
    pub executor_kind: String,
    pub runner_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub workspace_id: Uuid,
}

/// Capacity re-read ([`select_run_for_capacity_sql`]): `INNER JOIN pod`
/// (the FK is `NOT NULL`, so the join keeps every row), `ORDER BY
/// created_at DESC`, no `LIMIT` (`.get()`).
pub fn select_run_for_capacity_select(
    run_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectModel<CapacityRow>> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect};
    Entity::find()
        .select_only()
        .columns([
            Column::TerminalCapacityReleasedAt,
            Column::ExecutorKind,
            Column::RunnerId,
            Column::PodId,
            Column::WorkspaceId,
        ])
        .join(JoinType::InnerJoin, pod_rel())
        .filter(Column::Id.eq(run_id))
        .order_by_desc(Column::CreatedAt)
        .into_model::<CapacityRow>()
}

/// Run [`select_run_for_capacity_select`]: `None` is Django's
/// `DoesNotExist` (the caller raises).
pub async fn select_run_for_capacity(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
) -> Result<Option<CapacityRow>, sea_orm::DbErr> {
    select_run_for_capacity_select(run_id).one(db).await
}

/// Capacity-marker write ([`update_capacity_marker_sql`]): conditional on
/// the marker still being null.
pub fn update_capacity_marker_statement(
    run_id: Uuid,
    now: DateTime<Utc>,
) -> sea_orm::UpdateMany<orm::agent_run::Entity> {
    use orm::agent_run::{Column, Entity};
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    Entity::update_many()
        .col_expr(Column::TerminalCapacityReleasedAt, Expr::value(now))
        .filter(Column::Id.eq(run_id))
        .filter(Column::TerminalCapacityReleasedAt.is_null())
}

/// Run [`update_capacity_marker_statement`]; returns the rows written.
pub async fn update_capacity_marker(
    db: &impl sea_orm::ConnectionTrait,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sea_orm::DbErr> {
    Ok(update_capacity_marker_statement(run_id, now)
        .exec(db)
        .await?
        .rows_affected)
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

    // ------------------------------------------------------------------
    // merge_done_payload
    // ------------------------------------------------------------------

    #[test]
    fn yield_keys_match_fixture() {
        let fx = fixture();
        assert_eq!(
            fx.get("YIELD_KEYS"),
            Some(&json!(["status", "note", "yielded_at", "stop_ticking"]))
        );
        assert_eq!(YIELD_KEYS, ["status", "note", "yielded_at", "stop_ticking"]);
    }

    #[test]
    fn merge_done_payload_replays_all_fixture_cases() {
        let fx = fixture();
        let cases = fx
            .get("merge_done_payload")
            .and_then(Value::as_array)
            .expect("merge cases");
        assert_eq!(cases.len(), 11);
        for case in cases {
            let name = case.get("case").and_then(Value::as_str).expect("name");
            let existing = case.get("existing").expect("existing");
            let incoming = case.get("incoming").expect("incoming");
            let expected = case.get("out").expect("out");
            assert_eq!(
                merge_done_payload(existing, incoming),
                *expected,
                "case {name}"
            );
        }
    }

    #[test]
    fn merge_done_payload_does_not_mutate_inputs() {
        let existing = json!({"yielded_at": "x", "status": "s"});
        let incoming = json!({"conclusion": "c"});
        let existing_before = existing.clone();
        let incoming_before = incoming.clone();
        let merged = merge_done_payload(&existing, &incoming);
        assert_eq!(existing, existing_before);
        assert_eq!(incoming, incoming_before);
        assert_eq!(
            merged,
            json!({"conclusion": "c", "status": "s", "yielded_at": "x"})
        );
    }

    // ------------------------------------------------------------------
    // finalize values
    // ------------------------------------------------------------------

    #[test]
    fn finalize_values_assemble_base_then_extras() {
        let values = plan_finalize_values(AgentRunStatus::Completed, &[]).expect("terminal plans");
        let columns: Vec<&str> = values.clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "status",
                "ended_at",
                "terminal_hooks_applied_at",
                "terminal_capacity_released_at",
            ]
        );
        assert_eq!(
            values.clauses[0].value,
            SetValue::Text("completed".to_owned())
        );
        assert_eq!(values.clauses[1].value, SetValue::Now);
        // Caller extras overwrite same-name keys in place, append the rest.
        let values = plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("ended_at", SetValue::Now),
                ("error_code", SetValue::Text("approval_timeout".to_owned())),
                (
                    "error",
                    SetValue::Text("approval request expired".to_owned()),
                ),
                (
                    "cancel_reason",
                    SetValue::Text("approval_timeout".to_owned()),
                ),
            ],
        )
        .expect("terminal plans");
        let columns: Vec<&str> = values.clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "status",
                "ended_at",
                "terminal_hooks_applied_at",
                "terminal_capacity_released_at",
                "cancel_reason",
                "error_details",
            ]
        );
        // The folded trio lands as one patch, in the bag's key names.
        assert_eq!(
            values.clauses.last().map(|clause| &clause.value),
            Some(&SetValue::Json(json!({
                "code": "approval_timeout",
                "message": "approval request expired"
            })))
        );
        // The approval-timeout caller shape (tasks.py:73-81).
        assert_eq!(finalize_error_code(&values), "approval_timeout".to_owned());
    }

    #[test]
    fn folded_falsy_values_remove_their_key() {
        let values = plan_finalize_values(
            AgentRunStatus::Completed,
            &[
                ("error", SetValue::Text(String::new())),
                ("refusal_category", SetValue::Null),
            ],
        )
        .expect("terminal plans");
        assert_eq!(
            values.clauses.last().map(|clause| &clause.value),
            Some(&SetValue::Json(
                json!({"message": null, "refusal_category": null})
            ))
        );
        assert_eq!(finalize_error_code(&values), "");
        let sql = finalize_update_sql(&values);
        assert!(
            sql.contains(&format!(
                "\"error_details\" = {}",
                error_details_merge_sql("$5")
            )),
            "{sql}"
        );
        assert!(sql.ends_with("WHERE \"agent_run\".\"id\" = $6"), "{sql}");
        // A finalize that names none of them leaves the bag alone.
        let untouched = plan_finalize_values(AgentRunStatus::Completed, &[]).expect("plans");
        assert!(!finalize_update_sql(&untouched).contains("error_details"));
    }

    #[test]
    fn finalize_values_reject_nonterminal() {
        let fx = fixture();
        assert_eq!(
            fx["finalize_agent_run"]
                .get("nonterminal_new_status")
                .and_then(Value::as_str),
            Some("ValueError: new_status must be terminal")
        );
        for status in [
            AgentRunStatus::Queued,
            AgentRunStatus::Assigned,
            AgentRunStatus::WaitingForWorktree,
            AgentRunStatus::Running,
            AgentRunStatus::CancelRequested,
            AgentRunStatus::AwaitingApproval,
            AgentRunStatus::AwaitingReauth,
            AgentRunStatus::PausedAwaitingInput,
        ] {
            assert_eq!(
                plan_finalize_values(status, &[]),
                Err(FinalizeError::NonTerminalStatus),
                "{status} rejected"
            );
        }
        for status in TERMINAL_RUN_STATUSES {
            assert!(
                plan_finalize_values(status, &[]).is_ok(),
                "{status} accepted"
            );
        }
    }

    #[test]
    fn done_payload_merge_applies_only_when_key_present() {
        let mut values = plan_finalize_values(
            AgentRunStatus::Completed,
            &[("done_payload", SetValue::Json(json!({"conclusion": "ok"})))],
        )
        .expect("plans");
        let stored = json!({"status": "s", "yielded_at": "t"});
        apply_done_payload_merge(&mut values, &stored);
        let merged = values
            .clauses
            .iter()
            .find(|c| c.column == "done_payload")
            .expect("clause");
        assert_eq!(
            merged.value,
            SetValue::Json(json!({"conclusion": "ok", "status": "s", "yielded_at": "t"}))
        );
        // No done_payload key: untouched.
        let mut values = plan_finalize_values(AgentRunStatus::Cancelled, &[]).expect("plans");
        let before = values.clone();
        apply_done_payload_merge(&mut values, &stored);
        assert_eq!(values, before);
        // Absent error_code reads as "".
        assert_eq!(finalize_error_code(&values), "");
    }

    #[test]
    fn finalize_lock_matrix() {
        let fx = fixture();
        let finalize = fx.get("finalize_agent_run").expect("finalize section");
        assert_eq!(
            finalize
                .get("local_terminal_events")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            finalize
                .get("expected_runner_mismatch")
                .and_then(|m| m.get("status_still"))
                .and_then(Value::as_str),
            Some("running")
        );
        assert_eq!(
            finalize
                .get("expected_status_mismatch")
                .and_then(|m| m.get("status_still"))
                .and_then(Value::as_str),
            Some("assigned")
        );
        let runner = Uuid::nil();
        let other = Uuid::from_u128(1);
        for status in TERMINAL_RUN_STATUSES {
            assert!(
                !finalize_lock_passes(status, Some(runner), None, None),
                "{status} is closed"
            );
        }
        assert!(finalize_lock_passes(
            AgentRunStatus::Running,
            Some(runner),
            None,
            None
        ));
        assert!(finalize_lock_passes(
            AgentRunStatus::Running,
            Some(runner),
            Some(runner),
            None
        ));
        assert!(!finalize_lock_passes(
            AgentRunStatus::Running,
            Some(runner),
            Some(other),
            None
        ));
        assert!(!finalize_lock_passes(
            AgentRunStatus::Running,
            None,
            Some(runner),
            None
        ));
        assert!(finalize_lock_passes(
            AgentRunStatus::Assigned,
            Some(runner),
            None,
            Some(AgentRunStatus::Assigned)
        ));
        assert!(!finalize_lock_passes(
            AgentRunStatus::Assigned,
            Some(runner),
            None,
            Some(AgentRunStatus::Running)
        ));
        for status in [
            AgentRunStatus::Running,
            AgentRunStatus::Assigned,
            AgentRunStatus::Queued,
        ] {
            assert!(!effects_lock_passes(status), "{status} not terminal");
        }
        for status in TERMINAL_RUN_STATUSES {
            assert!(effects_lock_passes(status), "{status} terminal");
        }
    }

    // ------------------------------------------------------------------
    // terminal event + publish
    // ------------------------------------------------------------------

    #[test]
    fn terminal_event_seq_and_payload() {
        let fx = fixture();
        let cloud = fx["finalize_agent_run"]["cloud"].clone();
        let event = plan_terminal_event(None, AgentRunStatus::Completed, "E1");
        assert_eq!(event.seq, 1);
        assert_eq!(
            event.payload,
            json!({"status": "completed", "error_code": "E1"})
        );
        assert_eq!(
            cloud["events"][0].get("payload"),
            Some(&json!({"status": "completed", "error_code": "E1"}))
        );
        assert_eq!(
            cloud["events"][0].get("seq").and_then(Value::as_i64),
            Some(1)
        );
        assert_eq!(
            cloud["events"][0].get("kind").and_then(Value::as_str),
            Some("terminal")
        );
        // max + 1 over a gap.
        assert_eq!(
            plan_terminal_event(Some(5), AgentRunStatus::Completed, "").seq,
            6
        );
        assert_eq!(
            fx["finalize_agent_run"]
                .get("cloud_seq_after_gap")
                .and_then(|v| v.as_array().and_then(|a| a.first()))
                .and_then(Value::as_i64),
            Some(6)
        );
    }

    #[test]
    fn publish_effects_emit_then_inline() {
        assert_eq!(
            TERMINAL_EFFECTS_TASK,
            "runner.apply_agent_run_terminal_effects"
        );
        let effects = plan_publish_effects(Uuid::nil());
        assert_eq!(
            effects,
            [
                LifecycleEffect::PublishTerminalEffects {
                    run_id: Uuid::nil()
                },
                LifecycleEffect::ApplyTerminalEffectsInline {
                    run_id: Uuid::nil()
                },
            ]
        );
        let fx = fixture();
        let commits = fx["finalize_agent_run"]
            .get("on_commit_registered")
            .and_then(Value::as_array)
            .expect("on_commit");
        assert!(!commits.is_empty());
        for entry in commits {
            assert_eq!(entry.as_str(), Some("finalize_agent_run.<locals>.<lambda>"));
        }
    }

    // ------------------------------------------------------------------
    // terminal effects plans
    // ------------------------------------------------------------------

    fn effects_inputs(status: AgentRunStatus, run_config: &Value) -> TerminalEffectsInputs<'_> {
        TerminalEffectsInputs {
            run_id: Uuid::nil(),
            status,
            run_config,
            error: "",
            refusal_category: "",
            scheduler_binding: None,
            hooks_applied: false,
            capacity_released: false,
            executor_kind: AgentExecutorKind::LocalRunner,
            runner_id: Some(Uuid::nil()),
            pod_id: Some(Uuid::nil()),
            workspace_id: Uuid::nil(),
            work_item_present: true,
        }
    }

    fn effects_case<'a>(fx: &'a Value, name: &str) -> &'a Value {
        fx.get("apply_terminal_effects")
            .and_then(Value::as_array)
            .expect("effects cases")
            .iter()
            .find(|c| c.get("case").and_then(Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("case {name}"))
    }

    fn effect_names(plan: &TerminalEffectsPlan) -> Vec<&'static str> {
        let mut names = Vec::new();
        if let Some(hooks) = &plan.hooks {
            if hooks.failure_comment.is_some() {
                names.push("failure_comment");
            }
            if hooks.orchestration {
                names.push("orchestration");
            }
            if hooks.scheduler.is_some() {
                names.push("scheduler_hook");
            }
        }
        if plan.handoff_after_txn {
            names.push("handoff");
        }
        if let Some(capacity) = &plan.capacity {
            match capacity {
                CapacityPlan::Cloud { .. } => names.push("dispatch_waiting"),
                CapacityPlan::Local { runner_id, pod_id } => {
                    if runner_id.is_some() {
                        names.push("drain_runner");
                    }
                    if pod_id.is_some() {
                        names.push("drain_pod");
                    }
                }
            }
        }
        names
    }

    #[test]
    fn effects_completed_local_issue() {
        use EffectsSql::*;
        let fx = fixture();
        let gold = effects_case(&fx, "completed-local-issue");
        let outcome =
            plan_terminal_effects(&effects_inputs(AgentRunStatus::Completed, &Value::Null));
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        let hooks = plan.hooks.as_ref().expect("hooks run");
        assert_eq!(hooks.failure_comment, None);
        assert!(hooks.orchestration);
        assert_eq!(hooks.scheduler, None);
        assert!(!plan.handoff_after_txn);
        assert!(matches!(plan.capacity, Some(CapacityPlan::Local { .. })));
        assert_eq!(
            effect_names(&plan),
            ["orchestration", "drain_runner", "drain_pod"]
        );
        assert_eq!(
            gold.get("calls"),
            Some(&json!(["orchestration", "drain_runner", "drain_pod"]))
        );
        // Lock order issue→run, then markers; outer-transaction
        // savepoint pair belongs to the executing layer (n_sql 8).
        assert_eq!(
            plan.statements,
            [
                SelectWorkItemId,
                LockIssue,
                LockRunForEffects,
                UpdateHooksMarker,
                SelectRunForCapacity,
                UpdateCapacityMarker,
            ]
        );
        assert_eq!(gold.get("n_sql").and_then(Value::as_u64), Some(8));
        assert_eq!(gold.get("hooks_set").and_then(Value::as_bool), Some(true));
        assert_eq!(
            gold.get("capacity_set").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn effects_failed_local_issue_posts_comment_in_savepoint() {
        use EffectsSql::*;
        let fx = fixture();
        let gold = effects_case(&fx, "failed-local-issue");
        let mut inputs = effects_inputs(AgentRunStatus::Failed, &Value::Null);
        inputs.error = "boom detail";
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        let hooks = plan.hooks.as_ref().expect("hooks run");
        let comment = hooks.failure_comment.as_ref().expect("failure comment");
        assert!(comment.comment_html.contains("Run failed."));
        assert!(hooks.orchestration);
        assert_eq!(
            effect_names(&plan),
            [
                "failure_comment",
                "orchestration",
                "drain_runner",
                "drain_pod"
            ]
        );
        assert_eq!(
            gold.get("calls"),
            Some(&json!([
                "failure_comment",
                "orchestration",
                "drain_runner",
                "drain_pod"
            ]))
        );
        assert_eq!(
            plan.statements,
            [
                SelectWorkItemId,
                LockIssue,
                LockRunForEffects,
                FailureCommentSavepoint,
                FailureCommentReread,
                FailureCommentDedupeCheck,
                FailureCommentInserts,
                FailureCommentRelease,
                UpdateHooksMarker,
                SelectRunForCapacity,
                UpdateCapacityMarker,
            ]
        );
        // Suppressed detail: savepoint pair runs, reads do not.
        inputs.error = "daemon shutdown requested: restart";
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert_eq!(plan.hooks.as_ref().expect("hooks").failure_comment, None);
        assert!(plan.statements.contains(&FailureCommentSavepoint));
        assert!(plan.statements.contains(&FailureCommentRelease));
        assert!(!plan.statements.contains(&FailureCommentReread));
    }

    #[test]
    fn effects_handoff_skips_orchestration_and_comment() {
        use EffectsSql::*;
        let fx = fixture();
        let gold = effects_case(&fx, "handoff");
        let handoff_config = json!({"_project_move_handoff": {"target": "p"}});
        let mut inputs = effects_inputs(AgentRunStatus::Failed, &handoff_config);
        inputs.error = "boom detail";
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        let hooks = plan.hooks.as_ref().expect("hooks run");
        assert_eq!(hooks.failure_comment, None);
        assert!(!hooks.orchestration);
        assert!(plan.handoff_after_txn);
        assert_eq!(
            effect_names(&plan),
            ["handoff", "drain_runner", "drain_pod"]
        );
        assert_eq!(
            gold.get("calls"),
            Some(&json!(["handoff", "drain_runner", "drain_pod"]))
        );
        assert_eq!(
            plan.statements,
            [
                SelectWorkItemId,
                LockIssue,
                LockRunForEffects,
                UpdateHooksMarker,
                SelectRunForCapacity,
                UpdateCapacityMarker,
            ]
        );
    }

    #[test]
    fn effects_scheduler_binding_runs_hook_in_savepoint() {
        use EffectsSql::*;
        let fx = fixture();
        let gold = effects_case(&fx, "scheduler-binding");
        let mut inputs = effects_inputs(AgentRunStatus::Completed, &Value::Null);
        inputs.scheduler_binding = Some(BindingFacts {
            id: Uuid::nil(),
            last_error: "stale".to_owned(),
        });
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        let hooks = plan.hooks.as_ref().expect("hooks run");
        assert!(matches!(
            hooks.scheduler,
            Some(SchedulerHookPlan::ClearError { .. })
        ));
        assert_eq!(
            effect_names(&plan),
            [
                "orchestration",
                "scheduler_hook",
                "drain_runner",
                "drain_pod"
            ]
        );
        assert_eq!(
            gold.get("calls"),
            Some(&json!([
                "orchestration",
                "scheduler_hook",
                "drain_runner",
                "drain_pod"
            ]))
        );
        assert_eq!(
            plan.statements,
            [
                SelectWorkItemId,
                LockIssue,
                LockRunForEffects,
                SchedulerSavepoint,
                SchedulerUpdate,
                SchedulerRelease,
                UpdateHooksMarker,
                SelectRunForCapacity,
                UpdateCapacityMarker,
            ]
        );
        // Unchanged message: savepoint pair, no UPDATE.
        inputs.scheduler_binding = Some(BindingFacts {
            id: Uuid::nil(),
            last_error: String::new(),
        });
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert!(matches!(
            plan.hooks.as_ref().expect("hooks").scheduler,
            Some(SchedulerHookPlan::Noop)
        ));
        assert!(!plan.statements.contains(&SchedulerUpdate));
    }

    #[test]
    fn effects_cloud_dispatches_instead_of_draining() {
        let fx = fixture();
        let gold = effects_case(&fx, "cloud");
        let mut inputs = effects_inputs(AgentRunStatus::Completed, &Value::Null);
        inputs.executor_kind = AgentExecutorKind::CloudAgent;
        inputs.runner_id = None;
        inputs.pod_id = None;
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert!(matches!(plan.capacity, Some(CapacityPlan::Cloud { .. })));
        assert_eq!(effect_names(&plan), ["orchestration", "dispatch_waiting"]);
        assert_eq!(
            gold.get("calls"),
            Some(&json!(["orchestration", "dispatch_waiting"]))
        );
        let capacity = plan.capacity.expect("capacity");
        assert_eq!(
            capacity.effects(),
            [LifecycleEffect::DispatchWaiting {
                workspace_id: Uuid::nil()
            }]
        );
        // Local drains degrade gracefully without ids.
        let local = CapacityPlan::Local {
            runner_id: None,
            pod_id: None,
        };
        assert!(local.effects().is_empty());
    }

    #[test]
    fn effects_idempotency_markers_gate_branches() {
        let fx = fixture();
        // Hooks applied: capacity only.
        let applied = effects_case(&fx, "hooks-already-applied");
        let mut inputs = effects_inputs(AgentRunStatus::Completed, &Value::Null);
        inputs.hooks_applied = true;
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert_eq!(plan.hooks, None);
        assert!(!plan.handoff_after_txn);
        assert!(plan.capacity.is_some());
        assert_eq!(effect_names(&plan), ["drain_runner", "drain_pod"]);
        assert_eq!(
            applied.get("calls"),
            Some(&json!(["drain_runner", "drain_pod"]))
        );
        // Both markers: the capacity re-read still runs.
        let both = effects_case(&fx, "both-markers-set");
        inputs.capacity_released = true;
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert_eq!(plan.capacity, None);
        assert!(effect_names(&plan).is_empty());
        assert_eq!(both.get("calls"), Some(&json!([])));
        assert_eq!(
            plan.statements,
            [
                EffectsSql::SelectWorkItemId,
                EffectsSql::LockIssue,
                EffectsSql::LockRunForEffects,
                EffectsSql::SelectRunForCapacity,
            ]
        );
        // Non-terminal: no plan, lock attempt only.
        let non_terminal = effects_case(&fx, "non-terminal");
        let outcome = plan_terminal_effects(&effects_inputs(AgentRunStatus::Running, &Value::Null));
        assert_eq!(outcome, TerminalEffectsOutcome::NotTerminal);
        assert_eq!(non_terminal.get("calls"), Some(&json!([])));
        // No work item: no issue lock.
        let mut inputs = effects_inputs(AgentRunStatus::Completed, &Value::Null);
        inputs.work_item_present = false;
        let outcome = plan_terminal_effects(&inputs);
        let TerminalEffectsOutcome::Applied(plan) = outcome else {
            panic!("expected Applied");
        };
        assert!(!plan.statements.contains(&EffectsSql::LockIssue));
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
    fn finalize_lock_sql_variants() {
        let bare = lock_run_for_finalize_sql(false, false);
        assert_eq!(select_runs(&bare), vec![("\"agent_run\"".to_owned(), 40)]);
        assert!(
            bare.ends_with(
                " FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND NOT \
                 (\"agent_run\".\"status\" IN ($2, $3, $4, $5, $6))) ORDER BY \
                 \"agent_run\".\"created_at\" DESC LIMIT 1 FOR UPDATE"
            ),
            "tail: {bare}"
        );
        let full = lock_run_for_finalize_sql(true, true);
        assert!(
            full.ends_with(
                " WHERE (\"agent_run\".\"id\" = $1 AND NOT (\"agent_run\".\"status\" IN \
                 ($2, $3, $4, $5, $6)) AND \"agent_run\".\"runner_id\" = $7 AND \
                 \"agent_run\".\"status\" = $8) ORDER BY \"agent_run\".\"created_at\" DESC \
                 LIMIT 1 FOR UPDATE"
            ),
            "tail: {full}"
        );
        let runner_only = lock_run_for_finalize_sql(true, false);
        assert!(runner_only.contains("\"agent_run\".\"runner_id\" = $7"));
        assert!(!runner_only.contains("$8"));
        let status_only = lock_run_for_finalize_sql(false, true);
        assert!(status_only.contains("\"agent_run\".\"status\" = $7"));
    }

    #[test]
    fn finalize_update_sql_orders_set_columns() {
        let values = plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("error_code", SetValue::Text("approval_timeout".to_owned())),
                (
                    "error",
                    SetValue::Text("approval request expired".to_owned()),
                ),
                (
                    "cancel_reason",
                    SetValue::Text("approval_timeout".to_owned()),
                ),
            ],
        )
        .expect("plans");
        // The folded trio drops out of the SET list and lands last as one
        // `error_details` merge (Django's dict order after the pop).
        assert_eq!(
            finalize_update_sql(&values),
            format!(
                "UPDATE \"agent_run\" SET \"status\" = $1, \"ended_at\" = $2, \
                 \"terminal_hooks_applied_at\" = $3, \
                 \"terminal_capacity_released_at\" = $4, \"cancel_reason\" = $5, \
                 \"error_details\" = {} WHERE \"agent_run\".\"id\" = $7",
                error_details_merge_sql("$6")
            )
        );
    }

    #[test]
    fn terminal_event_sql_match_django() {
        assert_eq!(
            terminal_event_exists_sql(),
            "SELECT 1 AS \"a\" FROM \"agent_run_event\" WHERE \
             (\"agent_run_event\".\"agent_run_id\" = $1 AND \"agent_run_event\".\"kind\" = $2) LIMIT 1"
        );
        assert_eq!(
            terminal_event_max_seq_sql(),
            "SELECT \"agent_run_event\".\"seq\" FROM \"agent_run_event\" WHERE \
             \"agent_run_event\".\"agent_run_id\" = $1 ORDER BY \"agent_run_event\".\"seq\" DESC LIMIT 1"
        );
        assert_eq!(
            terminal_event_insert_sql(),
            "INSERT INTO \"agent_run_event\" (\"agent_run_id\", \"seq\", \"kind\", \
             \"payload\", \"created_at\") VALUES ($1, $2, $3, $4, $5) \
             RETURNING \"agent_run_event\".\"id\""
        );
    }

    #[test]
    fn effects_lock_sql_match_django_shapes() {
        assert_eq!(
            select_run_work_item_id_sql(),
            "SELECT \"agent_run\".\"work_item_id\" FROM \"agent_run\" WHERE \
             \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1"
        );
        let issue = lock_issue_sql();
        assert_eq!(select_runs(&issue), vec![("\"issues\"".to_owned(), 34)]);
        assert!(
            issue.ends_with(
                " FROM \"issues\" WHERE \"issues\".\"id\" = $1 ORDER BY \
                 \"issues\".\"created_at\" DESC LIMIT 1 FOR UPDATE OF \"issues\""
            ),
            "tail: {issue}"
        );
        let fetch = lock_run_for_effects_sql();
        assert_eq!(
            select_runs(&fetch),
            vec![
                ("\"agent_run\"".to_owned(), 40),
                ("\"issues\"".to_owned(), 34),
                ("\"projects\"".to_owned(), 46),
                ("\"states\"".to_owned(), 18),
                ("\"scheduler_bindings\"".to_owned(), 22),
            ]
        );
        assert!(
            fetch.ends_with(
                " LEFT OUTER JOIN \"scheduler_bindings\" ON \
                 (\"agent_run\".\"scheduler_binding_id\" = \"scheduler_bindings\".\"id\") WHERE \
                 (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"status\" IN \
                 ($2, $3, $4, $5, $6)) ORDER BY \"agent_run\".\"created_at\" DESC \
                 LIMIT 1 FOR UPDATE OF \"agent_run\""
            ),
            "tail: {fetch}"
        );
        let capacity = select_run_for_capacity_sql();
        assert_eq!(
            select_runs(&capacity),
            vec![
                ("\"agent_run\"".to_owned(), 40),
                ("\"pod\"".to_owned(), 10),
                ("\"runner\"".to_owned(), 31),
            ]
        );
        assert!(
            capacity.ends_with(
                " FROM \"agent_run\" INNER JOIN \"pod\" ON (\"agent_run\".\"pod_id\" = \"pod\".\"id\") \
                 LEFT OUTER JOIN \"runner\" ON (\"agent_run\".\"runner_id\" = \"runner\".\"id\") WHERE \
                 \"agent_run\".\"id\" = $1 ORDER BY \"agent_run\".\"created_at\" DESC"
            ),
            "get() has no LIMIT: {capacity}"
        );
        assert_eq!(
            update_hooks_marker_sql(),
            "UPDATE \"agent_run\" SET \"terminal_hooks_applied_at\" = $1 WHERE \"agent_run\".\"id\" = $2"
        );
        assert_eq!(
            update_capacity_marker_sql(),
            "UPDATE \"agent_run\" SET \"terminal_capacity_released_at\" = $1 WHERE \
             (\"agent_run\".\"id\" = $2 AND \"agent_run\".\"terminal_capacity_released_at\" IS NULL)"
        );
    }

    #[test]
    fn runner_columns_snapshot_matches_django_meta() {
        assert_eq!(
            RUNNER_COLUMNS.to_vec(),
            [
                "id",
                "owner_id",
                "workspace_id",
                "dev_machine_id",
                "pod_id",
                "name",
                "host_label",
                "provisioning",
                "visibility",
                "refresh_token_hash",
                "refresh_token_fingerprint",
                "refresh_token_generation",
                "previous_refresh_token_hash",
                "access_token_signing_key_version",
                "enrollment_token_hash",
                "enrollment_token_fingerprint",
                "enrolled_at",
                "capabilities",
                "status",
                "os",
                "arch",
                "runner_version",
                "dev_metadata",
                "protocol_version",
                "last_heartbeat_at",
                "created_at",
                "updated_at",
                "revoked_at",
                "revoked_reason",
                "paused_at",
                "paused_by_id",
            ]
        );
        assert_eq!(scheduler_binding::COLUMNS.len(), 22);
        assert_eq!(pod::COLUMNS.len(), 10);
    }

    // ------------------------------------------------------------------
    // Live kernel round trips (session temp tables, rolled back)
    // ------------------------------------------------------------------

    /// A one-connection pool on the scratch `DATABASE_URL`, or `None`
    /// (skip) without one.
    async fn temp_table_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    /// Session temp copies that shadow the real tables for this
    /// transaction (no constraints, no foreign keys, no generated
    /// columns: the test seeds what the entities decode).
    async fn temp_tables(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tables: &[&str]) {
        for table in tables {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut **tx)
            .await
            .expect("temp table");
        }
    }

    /// Seed one `agent_run` row with every NOT NULL column the entity
    /// decodes (the executor, status, runner, work item, parent and
    /// `error_details` vary per test).
    #[allow(clippy::too_many_arguments)]
    async fn seed_run(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
        status: &str,
        executor_kind: &str,
        runner_id: Option<Uuid>,
        work_item_id: Option<Uuid>,
        parent_run_id: Option<Uuid>,
        scheduler_binding_id: Option<Uuid>,
        pod_id: Uuid,
        error_details: Value,
    ) {
        sqlx::query(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, runner_id, \
             work_item_id, parent_run_id, scheduler_binding_id, status, executor_kind, \
             dispatch_attempts, cancel_reason, tool_plan, prompt, trigger, phase_kind, \
             run_config, required_capabilities, thread_id, agent_metadata, error_details, \
             llm_model, usage, created_at, done_payload) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, \
             $9, $10, 0, '', '{}', '', 'direct', 'work', '{}', '[]', 'thread', '{}', $11, '', \
             '{}', now(), NULL)",
        )
        .bind(id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(pod_id)
        .bind(runner_id)
        .bind(work_item_id)
        .bind(parent_run_id)
        .bind(scheduler_binding_id)
        .bind(status)
        .bind(executor_kind)
        .bind(error_details)
        .execute(&mut **tx)
        .await
        .expect("seed run");
    }

    /// The finalize kernel end to end on temp tables: expected-runner /
    /// status gates, first-writer-wins, the `error_details` merge (keys
    /// the patch names drop, nulls strip, the rest stays), the
    /// idempotent cloud terminal event, then the effects lock, the
    /// capacity re-read and both marker writes.
    #[tokio::test]
    async fn live_finalize_and_effects_kernel() {
        let Some(pool) = temp_table_pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        temp_tables(
            &mut tx,
            &["agent_run", "scheduler_bindings", "pod", "issues"],
        )
        .await;
        // `LIKE ... INCLUDING DEFAULTS` keeps the `BigAutoField` sequence.
        sqlx::query(
            "CREATE TEMP TABLE agent_run_event (LIKE public.agent_run_event INCLUDING DEFAULTS INCLUDING IDENTITY)",
        )
        .execute(&mut *tx)
        .await
        .expect("temp event table");
        let run = Uuid::new_v4();
        let pod_id = Uuid::new_v4();
        let binding = Uuid::new_v4();
        let issue = Uuid::new_v4();
        sqlx::query("INSERT INTO pod (id, name, description, is_default, created_at, updated_at, workspace_id, project_id) VALUES ($1, 'p', '', true, now(), now(), $2, $3)")
            .bind(pod_id).bind(Uuid::new_v4()).bind(Uuid::new_v4())
            .execute(&mut *tx).await.expect("seed pod");
        sqlx::query("INSERT INTO scheduler_bindings (id, last_error, created_at, updated_at) VALUES ($1, 'stale', now(), now())")
            .bind(binding)
            .execute(&mut *tx).await.expect("seed binding");
        sqlx::query("INSERT INTO issues (id, created_at, updated_at, name, project_id, workspace_id) VALUES ($1, now(), now(), 'i', $2, $3)")
            .bind(issue).bind(Uuid::new_v4()).bind(Uuid::new_v4())
            .execute(&mut *tx).await.expect("seed issue");
        seed_run(
            &mut tx,
            run,
            "running",
            "cloud_agent",
            None,
            Some(issue),
            None,
            Some(binding),
            pod_id,
            serde_json::json!({"x": 1, "message": "old", "code": "old"}),
        )
        .await;
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        // The expected-runner gate misses (runnerless cloud run); the
        // expected-status gate passes.
        assert!(
            lock_run_for_finalize(&conn, run, Some(Uuid::new_v4()), None)
                .await
                .expect("lock")
                .is_none()
        );
        let locked = lock_run_for_finalize(&conn, run, None, Some(AgentRunStatus::Running))
            .await
            .expect("lock")
            .expect("non-terminal row");
        assert_eq!(locked.executor_kind, "cloud_agent");
        assert_eq!(locked.done_payload, None);
        // `error` folds to null → the stored `message` key drops; `code`
        // is replaced; `x` survives.
        let values = plan_finalize_values(
            AgentRunStatus::Failed,
            &[
                ("error_code", SetValue::Text("boom".to_owned())),
                ("error", SetValue::Text(String::new())),
                ("cancel_reason", SetValue::Text("r".to_owned())),
            ],
        )
        .expect("plan");
        let now = Utc::now();
        assert_eq!(
            finalize_update(&conn, &values, run, now)
                .await
                .expect("update"),
            1
        );
        // First-writer-wins: the row is terminal now.
        assert!(lock_run_for_finalize(&conn, run, None, None)
            .await
            .expect("lock")
            .is_none());
        append_terminal_event_once(&conn, run, AgentRunStatus::Failed, "boom", now)
            .await
            .expect("event");
        append_terminal_event_once(&conn, run, AgentRunStatus::Failed, "boom", now)
            .await
            .expect("event again");
        assert!(terminal_event_exists(&conn, run).await.expect("exists"));
        assert_eq!(
            terminal_event_max_seq(&conn, run).await.expect("max"),
            Some(1)
        );
        // The effects lock sees the written row and the joined binding.
        let effects = lock_run_for_effects(&conn, run)
            .await
            .expect("effects lock")
            .expect("terminal row");
        assert_eq!(effects.status, "failed");
        assert_eq!(
            effects.error_details,
            serde_json::json!({"x": 1, "code": "boom"})
        );
        assert_eq!(effects.error(), "");
        assert_eq!(effects.refusal_category(), "");
        assert_eq!(
            effects.binding_facts(),
            Some(BindingFacts {
                id: binding,
                last_error: "stale".to_owned()
            })
        );
        assert!(effects.terminal_hooks_applied_at.is_none());
        assert_eq!(
            select_run_work_item_id(&conn, run)
                .await
                .expect("work item"),
            Some(Some(issue))
        );
        lock_issue(&conn, issue).await.expect("issue lock");
        assert_eq!(
            update_hooks_marker(&conn, run, now).await.expect("marker"),
            1
        );
        let capacity = select_run_for_capacity(&conn, run)
            .await
            .expect("capacity")
            .expect("row");
        assert_eq!(capacity.pod_id, pod_id);
        assert_eq!(capacity.executor_kind, "cloud_agent");
        assert!(capacity.terminal_capacity_released_at.is_none());
        assert_eq!(
            update_capacity_marker(&conn, run, now)
                .await
                .expect("marker"),
            1
        );
        // Conditional on the marker still being null.
        assert_eq!(
            update_capacity_marker(&conn, run, now)
                .await
                .expect("marker"),
            0
        );
        assert!(lock_run_for_effects(&conn, run)
            .await
            .expect("effects lock")
            .expect("row")
            .terminal_hooks_applied_at
            .is_some());
        // A missing run: no work-item row, no capacity row.
        assert_eq!(
            select_run_work_item_id(&conn, Uuid::new_v4())
                .await
                .expect("miss"),
            None
        );
        assert!(select_run_for_capacity(&conn, Uuid::new_v4())
            .await
            .expect("miss")
            .is_none());
        tx.rollback().await.expect("rollback");
    }
}
