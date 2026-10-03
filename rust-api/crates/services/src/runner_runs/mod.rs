#![forbid(unsafe_code)]

//! Runner runs services (D-15, stage 5).
//!
//! * [`chat`] — the chat service closure (`services/chat.py`, L5,
//!   PIDASHCONV-537).
//! * [`guards`] — `runner/services/permissions.py:1-137` (predicate SQL),
//!   `runner/views/runs.py:67-122` (`_parse_pagination`, `_can_view_run`,
//!   `_can_cancel_run`) and the 404-not-403 denial (L3, PIDASHCONV-529).
//! * [`shape`] — `runner/serializers.py` run (`:241-319`), approval
//!   (`:320-340`) and chat (`:342-422`) shapes, plus the
//!   accept/decline decision validation (L3, PIDASHCONV-529).
//!
//! The L3 guards/shapes are pure kernels over caller-owned L2 rows
//! ([`pidash_db::runner_runs`] in, `Serialize` views and SQL fragments
//! out); fixture contract FX-RUN-04 plus the shape goldens inside
//! FX-RUN-08/09.
//!
//! Pure port of the lifecycle closure in
//! `apps/api/pi_dash/runner/services/`:
//!
//! * [`lifecycle`] — `run_lifecycle.py:1-463` (pause, requeue pair,
//!   terminal-update planning, failure comment, pending-entry fire).
//! * [`finalization`] — `agent_run_finalization.py:1-199`
//!   (`merge_done_payload`, first-writer-wins `finalize_agent_run`,
//!   `apply_terminal_effects`).
//! * [`scheduler_hook`] — `scheduler_hook.py:1-42`
//!   (`update_scheduler_binding_on_terminate`).
//!
//! The Python modules import each other (`run_lifecycle` ↔
//! `agent_run_finalization`); here they are sibling modules of one
//! directory, which is the same cycle cut the crate graph requires.
//!
//! # Layering: plans, not queries
//!
//! This crate has no database handle, so every entry point is a pure
//! function over pre-fetched facts: it returns an update plan (typed
//! fields plus an ordered [`SetClause`] list that pins the `SET`
//! order), the exact SQL text the executing layer runs (Django's
//! `%s` placeholders rendered as Postgres `$N`), and the ordered
//! [`LifecycleEffect`]s the executing layer fires after commit via
//! the foundation post-commit wrapper (`pidash_db::tx`). The
//! executing layers are L6 (`pidash-jobs`, which runs the stall
//! reap, the approval-expiry cancel and the terminal-effects task)
//! and L7/L8 (`pidash-api` run endpoints), all `blocked_by` this
//! issue. Cross-domain work (D-12 orchestration + handoff, D-14
//! drains, D-11 `dispatch_waiting`, the `fire_tick` /
//! terminal-effects Celery emits) is carried as effect descriptors —
//! never executed here — so each lands exactly once, on the same
//! transaction boundary as in Django, when its provider exists.
//!
//! Reused, not forked: L1 (`pidash_types::runner_runs` enums, usage
//! merge, diagnostics enrich), L2 (`pidash_db::runner_runs` row
//! structs and `_meta`-order column lists), the scheduler-binding
//! columns + `LAST_ERROR_MAX_LEN` (`pidash_db::tasks_ticker::models`),
//! and the `MLStripper` port (`pidash_db::app_pages::strip`) that
//! `IssueComment.save` funnels through. Django's own `strip_tags`
//! (which `Description.save` uses, and which keeps entities verbatim
//! where `MLStripper` decodes them) is mirrored in [`lifecycle`]
//! because the `api`-crate twin is unreachable from `services`.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-05-lifecycle.golden.json`
//! (FX-RUN-05). Every section is replayed by the unit tests beside
//! the code, and every SQL builder is pinned byte-for-byte against
//! Django 4.2.30 output captured under a minimal settings module
//! (column lists via `_meta.concrete_fields`, statements via the
//! query compiler; `%s` → `$N` positionally).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * The finalize lock's `NOT (status IN …)` list comes from a Python
//!   `set`, so Django emits the five values in hash order — different
//!   text on every process start. The semantics are order-free, and
//!   Rust pins the `run_lifecycle` tuple order deterministically.
//! * A truthy non-dict pause `payload` (or a truthy non-dict
//!   `autonomy` value) commits the phase-1 row update and *then*
//!   raises `AttributeError` on `.get`. Rust returns a typed error
//!   from the comment planner instead; the executing layer maps it
//!   to the same 500.
//! * `Description`'s `created_by/updated_by` come from the ambient
//!   request user (`BaseModel.save`), silently discarding the
//!   `description_defaults` values `IssueComment.save` passes.
//! * `save(update_fields=[…])` raises `DatabaseError` when zero rows
//!   match; every single-row save in this port documents that as a
//!   `RowMissing` mapping for the executing layer.
//! * A comment's stripped text is stored twice with different
//!   functions: `comment_stripped` via `MLStripper` (entities
//!   decoded), `description_stripped` via Django's `strip_tags`
//!   (entities verbatim). Both are computed, exactly as saved.

pub mod chat;
pub mod finalization;
pub mod guards;
pub mod lifecycle;
pub mod scheduler_hook;
pub mod shape;

pub use finalization::{
    apply_done_payload_merge, effects_lock_passes, finalize_error_code, finalize_lock_passes,
    finalize_update_sql, lock_issue_sql, lock_run_for_effects_sql, lock_run_for_finalize_sql,
    merge_done_payload, plan_finalize_values, plan_publish_effects, plan_terminal_effects,
    plan_terminal_event, select_run_for_capacity_sql, select_run_work_item_id_sql,
    terminal_event_exists_sql, terminal_event_insert_sql, terminal_event_max_seq_sql,
    terminal_event_payload, update_capacity_marker_sql, update_hooks_marker_sql, CapacityPlan,
    EffectsSql, FinalizeError, FinalizeValues, HooksPlan, TerminalEffectsInputs,
    TerminalEffectsOutcome, TerminalEffectsPlan, TerminalEventPlan,
};
pub use lifecycle::{
    comment_dedupe_exists_sql, comment_description_link_sql, comment_insert_sql,
    description_insert_sql, failure_reread_sql, has_project_move_handoff, is_infra_failure_detail,
    live_state_by_runner_sql, lock_run_for_pause_sql, lock_run_for_requeue_sql, normalize_model,
    normalize_refusal_category, parent_thread_clear_sql, pause_comment_html,
    pause_drain_reread_sql, pause_lock_passes, pause_reread_sql, pause_update_sql, payload_usage,
    plan_assign_rejected_busy, plan_failure_comment, plan_pause_comment, plan_pause_update,
    plan_requeue_from_locked, plan_terminal_finalize, plan_terminal_updates, requeue_lock_passes,
    requeue_update_sql, runner_busy_update_sql, terminal_extras, ticker_pending_entry_sql,
    usage_updates, AssignRejectedBusyPlan, CommentInsert, CommentPlan, CommentSpeaker,
    LifecycleError, LiveStateUsageFacts, PauseUpdatePlan, RequeuePlan, RequeueRunFacts,
    TerminalUpdateInputs, TerminalUpdates, UsageUpdates, FIRE_TICK_TASK,
    INFRA_FAILURE_DETAIL_PREFIXES, LLM_MODEL_MAX_CHARS, PROJECT_MOVE_HANDOFF_CONFIG_KEY,
    RUN_ERROR_MAX_CHARS,
};
pub use scheduler_hook::{
    plan_scheduler_hook, scheduler_binding_update_sql, BindingFacts, SchedulerHookPlan,
};

use serde_json::Value;
use uuid::Uuid;

/// One deferred or post-commit side effect. Each variant names the
/// exact Python call it replaces and the boundary it fires on; the
/// executing layer runs them through `pidash_db::tx` post-commit
/// actions (or inline, where noted) with the same per-effect
/// isolation (`try/except` + `logger.exception`) as the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEffect {
    /// Pause-path `_pause_and_drain`: re-read the run (`pause_drain_reread_sql`),
    /// run [`LifecycleEffect::PostRunOrchestration`] when the row is still there,
    /// then always [`LifecycleEffect::DrainRunner`]. Registered with
    /// `on_commit` (deferred to the request commit at every real call site).
    PauseAndDrain { run_id: Uuid, runner_id: Uuid },
    /// `_apply_post_run_orchestration`: `maybe_disarm_on_terminal_signal`,
    /// then `maybe_apply_deferred_pause`, then the pending-entry fire —
    /// each in its own isolation scope, in order. Runs inline in
    /// `apply_terminal_effects`, post-commit on the pause path.
    PostRunOrchestration { run_id: Uuid },
    /// `fire_tick.delay(str(ticker_id))`: Celery emit of
    /// `FIRE_TICK_TASK` with one positional string arg.
    FireTick { ticker_id: Uuid },
    /// `drain_for_runner_by_id(runner_id)` (D-14).
    DrainRunner { runner_id: Uuid },
    /// `drain_pod_by_id(pod_id)` (D-14).
    DrainPod { pod_id: Uuid },
    /// `dispatch_waiting(workspace_id)` (D-11): synchronous, *not*
    /// isolated — a failure propagates and the capacity marker stays
    /// unset so the reconciler retries.
    DispatchWaiting { workspace_id: Uuid },
    /// `complete_project_move_handoff(run_id)` (D-12): after the
    /// hooks transaction commits, isolated.
    CompleteProjectMoveHandoff { run_id: Uuid },
    /// `apply_agent_run_terminal_effects.delay(str(run_id))`: Celery
    /// emit of `finalization::TERMINAL_EFFECTS_TASK`, first half of
    /// `_publish_effects`, isolated.
    PublishTerminalEffects { run_id: Uuid },
    /// Inline `apply_terminal_effects(run_id)`, second half of
    /// `_publish_effects`, isolated. Harmless when racing the queued
    /// task: the hooks marker is locked and idempotent.
    ApplyTerminalEffectsInline { run_id: Uuid },
}

/// One `SET` assignment in an `UPDATE`, in exact Django order. `Now`
/// is `timezone.now()` evaluated once per statement at execution;
/// `Text`/`Json` carry the bound value; `Null` writes SQL `NULL`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetClause {
    pub column: &'static str,
    pub value: SetValue,
}

/// A bound `SET` value (see [`SetClause`]).
#[derive(Debug, Clone, PartialEq)]
pub enum SetValue {
    Null,
    Now,
    Text(String),
    Json(Value),
}

/// Python truthiness for JSON frame values (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy).
pub(crate) fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().map(|f| f != 0.0).unwrap_or(false)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `str.strip()`: Rust's `is_whitespace` set plus U+001C..=U+001F
/// (same formula as the L1 `py_strip` twins).
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// First `max` Unicode code points of `text` (`text[:max]`).
/// Byte-slicing would panic on a UTF-8 boundary; `chars().take()`
/// cannot. A string that fits in `max` bytes needs no walk.
pub(crate) fn truncate_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// Python `str(value)` for JSON frame values. Only ever called on
/// truthy values (every call site checks truthiness first, as the
/// `or ""` / `if …` in the source does), but total: `None`/`True`/`False`
/// spellings, integers verbatim, floats in CPython repr form,
/// strings as-is, containers in single-quote repr form. Container
/// key order follows `serde_json::Map` iteration order.
pub(crate) fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr(value)` for a value nested in a container: identical
/// to [`py_str`] except strings, which gain quotes.
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => py_repr_str(s),
        other => py_str(other),
    }
}

/// Python `str(number)`: integers verbatim, floats in CPython repr form.
/// Integers beyond `u64` parse as `f64` (their digits are already lost
/// by `serde_json`, exactly as for a float literal with the same
/// value), so they render in exponent form like the float they parse
/// to — `10^30` and `1e30` are the same `Value` and the same output.
fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    n.as_f64()
        .map(py_float_str)
        .unwrap_or_else(|| n.to_string())
}

/// CPython `repr(float)`: Rust's `Debug` shortest round-trip with the
/// exponent normalized to Python's `e±XX` form (`1e16` → `1e+16`,
/// `1e-5` → `1e-05`). The two engines switch to exponent notation
/// at different magnitudes in rare cases; that edge is documented,
/// not bridged — frame floats are already absurd this far down.
fn py_float_str(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let rust = format!("{f:?}");
    let Some(pos) = rust.find('e') else {
        return rust;
    };
    let (mantissa, exp) = rust.split_at(pos);
    let exp = &exp[1..];
    let (sign, digits) = match exp.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exp.strip_prefix('+').unwrap_or(exp)),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// Python `repr(str)`: single quotes unless the string contains `'`
/// but not `"`, backslash escapes for quotes/backslash/whitespace,
/// `\xNN` / `\uNNNN` / `\U00NNNNNN` for the rest of the
/// non-printables. The printable test covers controls, DEL and the
/// line/paragraph separators; exotic format characters (`Cf`) pass
/// through raw — a divergence reachable only from hostile frames.
fn py_repr_str(s: &str) -> String {
    let use_double = s.contains('\'') && !s.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if py_printable(c) => out.push(c),
                c if (c as u32) < 0x100 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if (c as u32) < 0x1_0000 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => {
                    out.push_str(&format!("\\U{:08x}", c as u32));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// Approximation of `str.isprintable` for [`py_repr_str`]: controls,
/// DEL and U+2028/2029 are non-printable; everything else (including
/// the exotic `Cf` format characters) prints raw.
fn py_printable(c: char) -> bool {
    !(c.is_control() || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}')
}

/// `issues` columns in Django `_meta` order (captured from
/// `Issue._meta.concrete_fields`). The `app_issues` port keeps an
/// id-first struct order for its own reads; these lists pin the
/// `SELECT` text only. Canonical home for row structs is the owning
/// domain's port; the lists here exist so the joined lifecycle
/// reads compile byte-exact today.
pub(crate) const ISSUE_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "parent_id",
    "state_id",
    "point",
    "estimate_point_id",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "type_id",
    "git_work_branch",
    "workpad",
    "created_via",
    "assigned_pod_id",
    "agent_executor",
];

/// `projects` columns in Django `_meta` order (captured from
/// `Project._meta.concrete_fields`); see [`ISSUE_COLUMNS`].
pub(crate) const PROJECT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "workspace_id",
    "identifier",
    "default_assignee_id",
    "project_lead_id",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "cover_image_asset_id",
    "estimate_id",
    "archive_in",
    "close_in",
    "logo_props",
    "default_state_id",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
];

/// `states` columns in Django `_meta` order (captured from
/// `State._meta.concrete_fields`); see [`ISSUE_COLUMNS`].
pub(crate) const STATE_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "name",
    "description",
    "color",
    "slug",
    "sequence",
    "group",
    "is_triage",
    "default",
    "external_source",
    "external_id",
];

/// Render `"table"."a", "table"."b", …` for a column list.
pub(crate) fn qualified_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|c| format!("\"{table}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn py_truthy_matches_python() {
        for falsy in [
            Value::Null,
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!py_truthy(&falsy), "{falsy} is falsy");
        }
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
            assert!(py_truthy(&truthy), "{truthy} is truthy");
        }
    }

    #[test]
    fn py_str_matches_python_battery() {
        // Differential oracle: /tmp/dj534diff.py `str:*` / `repr:*`.
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(false)), "False");
        assert_eq!(py_str(&json!(5)), "5");
        assert_eq!(py_str(&json!(-3)), "-3");
        assert_eq!(py_str(&json!(5.0)), "5.0");
        assert_eq!(py_str(&json!(0.1)), "0.1");
        assert_eq!(py_str(&json!(1e16)), "1e+16");
        assert_eq!(py_str(&json!(1e-5)), "1e-05");
        assert_eq!(py_str(&json!("abc")), "abc");
        assert_eq!(
            py_str(&json!([1, "a", Value::Null, true])),
            "[1, 'a', None, True]"
        );
        assert_eq!(py_str(&json!([])), "[]");
        assert_eq!(py_str(&json!({})), "{}");
        // Beyond u64 the digits are already f64: same Value, same
        // output as the equivalent float literal.
        let big: Value = serde_json::from_str("1000000000000000000000000000000").expect("big");
        assert_eq!(py_str(&big), "1e+30");
        assert_eq!(py_str(&json!(1e30)), "1e+30");
        // Single-key dict (multi-key order follows the map).
        assert_eq!(py_str(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_str(&json!({"a": "x's"})), "{'a': \"x's\"}");
    }

    #[test]
    fn py_repr_str_quotes_and_escapes() {
        assert_eq!(py_repr_str("abc"), "'abc'");
        assert_eq!(py_repr_str("x's"), "\"x's\"");
        assert_eq!(py_repr_str("say \"hi\""), "'say \"hi\"'");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr_str("a\nb\tc\\d"), "'a\\nb\\tc\\\\d'");
        assert_eq!(py_repr_str("\u{1}"), "'\\x01'");
        assert_eq!(py_repr_str("\u{7f}"), "'\\x7f'");
        assert_eq!(py_repr_str("—"), "'—'");
    }

    #[test]
    fn truncate_chars_counts_code_points() {
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("abcdef", 3), "abc");
        assert_eq!(truncate_chars("———", 2), "——");
        assert_eq!(truncate_chars("", 10), "");
    }

    #[test]
    fn shared_column_snapshots_match_django_meta() {
        assert_eq!(ISSUE_COLUMNS.len(), 34);
        assert_eq!(
            &ISSUE_COLUMNS[..6],
            [
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id"
            ]
        );
        assert_eq!(
            &ISSUE_COLUMNS[29..],
            [
                "git_work_branch",
                "workpad",
                "created_via",
                "assigned_pod_id",
                "agent_executor"
            ]
        );
        assert!(ISSUE_COLUMNS.contains(&"sequence_id"));
        assert_eq!(PROJECT_COLUMNS.len(), 46);
        assert_eq!(
            &PROJECT_COLUMNS[..6],
            [
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id"
            ]
        );
        assert_eq!(
            &PROJECT_COLUMNS[44..],
            ["agent_ticking_enabled", "default_agent_executor"]
        );
        assert!(PROJECT_COLUMNS.contains(&"agent_default_max_ticks"));
        assert_eq!(STATE_COLUMNS.len(), 18);
        assert_eq!(
            STATE_COLUMNS.to_vec(),
            [
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "name",
                "description",
                "color",
                "slug",
                "sequence",
                "group",
                "is_triage",
                "default",
                "external_source",
                "external_id",
            ]
        );
        assert_eq!(
            qualified_columns("t", &["a", "b"]),
            "\"t\".\"a\", \"t\".\"b\""
        );
    }
}
