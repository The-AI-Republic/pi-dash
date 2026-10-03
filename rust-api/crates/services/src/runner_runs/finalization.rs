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
/// (`agent_run_finalization.py:27`), in tuple order.
pub const YIELD_KEYS: [&str; 3] = ["status", "note", "yielded_at"];

/// `runner` columns in Django `_meta` order (captured from
/// `Runner._meta.concrete_fields`): the capacity re-read joins the
/// full row. Canonical home is the D-13 port; this list pins the
/// `SELECT` text until it lands.
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
    "free_worktrees",
    "created_at",
    "updated_at",
    "revoked_at",
    "revoked_reason",
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
/// `status`, `ended_at`, `queue_position`, both markers cleared, then
/// the caller extras in order (overwriting same-name base keys in
/// place). `done_payload` still holds the *incoming* value here; the
/// executing layer runs [`apply_done_payload_merge`] against the
/// locked row before rendering the `UPDATE`.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizeValues {
    pub clauses: Vec<SetClause>,
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
            column: "queue_position",
            value: SetValue::Null,
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

/// The terminal event's `error_code` (`finalize_agent_run:83`):
/// `values.get("error_code", "")`. Both real callers pass a string
/// or nothing; a non-string value (unreachable from them) reads as
/// `""`, matching the default arm.
pub fn finalize_error_code(values: &FinalizeValues) -> String {
    values
        .clauses
        .iter()
        .find(|clause| clause.column == "error_code")
        .and_then(|clause| match &clause.value {
            SetValue::Text(code) => Some(code.clone()),
            _ => None,
        })
        .unwrap_or_default()
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
/// the id binds last.
pub fn finalize_update_sql(values: &FinalizeValues) -> String {
    let set = values
        .clauses
        .iter()
        .enumerate()
        .map(|(index, clause)| format!("\"{}\" = ${}", clause.column, index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"agent_run\" SET {set} WHERE \"agent_run\".\"id\" = ${}",
        values.clauses.len() + 1
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
/// `select_related("runner", "pod").get(pk)` — 41 + 10 + 30 columns,
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
            Some(&json!(["status", "note", "yielded_at"]))
        );
        assert_eq!(YIELD_KEYS, ["status", "note", "yielded_at"]);
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
                "queue_position",
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
                "queue_position",
                "terminal_hooks_applied_at",
                "terminal_capacity_released_at",
                "error_code",
                "error",
                "cancel_reason",
            ]
        );
        // The approval-timeout caller shape (tasks.py:73-81).
        assert_eq!(finalize_error_code(&values), "approval_timeout".to_owned());
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
        assert_eq!(select_runs(&bare), vec![("\"agent_run\"".to_owned(), 41)]);
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
        assert_eq!(
            finalize_update_sql(&values),
            "UPDATE \"agent_run\" SET \"status\" = $1, \"ended_at\" = $2, \
             \"queue_position\" = $3, \"terminal_hooks_applied_at\" = $4, \
             \"terminal_capacity_released_at\" = $5, \"error_code\" = $6, \"error\" = $7, \
             \"cancel_reason\" = $8 WHERE \"agent_run\".\"id\" = $9"
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
                ("\"agent_run\"".to_owned(), 41),
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
                ("\"agent_run\"".to_owned(), 41),
                ("\"pod\"".to_owned(), 10),
                ("\"runner\"".to_owned(), 30),
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
                "free_worktrees",
                "created_at",
                "updated_at",
                "revoked_at",
                "revoked_reason",
            ]
        );
        assert_eq!(scheduler_binding::COLUMNS.len(), 22);
        assert_eq!(pod::COLUMNS.len(), 10);
    }
}
