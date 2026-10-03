//! Orchestration L1 types: phases + done-signal + ticker + outcomes (D-12, stage 5).
//!
//! Ports the exception-free bottom of the orchestration engine:
//!
//! * [`phases`] — `orchestration/agent_phases.py` (whole file).
//! * [`done_signal`] — `orchestration/done_signal.py` parse half
//!   (`FENCE_RE`, `VALID_STATUSES`, `DoneSignalError`, `DoneSignal`,
//!   `extract_fence`, `parse`, `_normalize`).
//! * [`ticker`] — `orchestration/scheduling.py` event/decision half
//!   (`TickerEventKind`, `TickerEvent`, `TickerDecision`).
//! * [`outcomes`] — `orchestration/scheduling.py` outcome vocabulary +
//!   `orchestration/service.py` outcome dataclasses and constants.
//!
//! Pure types only: this crate does no I/O. Events carry ids and handlers
//! re-read rows; `outcome_for_run` takes a minimal payload/kind struct.
//!
//! Fixture id replayed by the unit tests alongside each module: FX-ORCH-01
//! (`rust-api/fixtures/orchestration/fx01_types/*.golden.json`).

pub mod done_signal;
pub mod outcomes;
pub mod phases;
pub mod ticker;

pub use done_signal::{
    extract_fence, normalize, parse, DoneSignal, DoneSignalError, VALID_STATUSES,
};
pub use outcomes::{
    default_outcome_for_kind, normalize_outcome, outcome_for_run, ContinuationOutcome,
    RunOutcomeRef, TransitionOutcome,
};
pub use outcomes::{
    CONTINUATION_ELIGIBLE_GROUPS, LEGACY_OUTCOME_ALIASES, MACHINE_TRIGGERS, OUTCOME_BLOCKED,
    OUTCOME_DONE, OUTCOME_PROGRESSED, OUTCOME_WAITING_ON_EXTERNAL, OUTCOME_WAITING_ON_HUMAN,
    PAUSED_STATE_NAME, PROJECT_MOVE_HANDOFF_CONFIG_KEY, RUN_AI_ACTIVE_RUN_EXISTS,
    RUN_AI_NO_ELIGIBLE_RUNNER, RUN_AI_NO_POD, RUN_OUTCOMES, STOPPING_OUTCOMES,
    TRIGGER_COMMENT_AND_RUN, TRIGGER_DIRECT, TRIGGER_RUN_AI, TRIGGER_SCHEDULER,
    TRIGGER_STATE_TRANSITION, TRIGGER_TICK, WAIT_ACTIVITY_FIELD, WAIT_CAP_REACHED, WAIT_GRANTED,
    WAIT_INFINITE_POOL, WAIT_NO_TICKER, WAIT_REARMABLE_DISARMS,
};
#[allow(deprecated)]
pub use outcomes::{SCHEDULING_DELEGATION_STATE_NAME, SERVICE_DELEGATION_STATE_NAME};
pub use phases::{
    auto_pauses_on_cap, cadence_fields_by_group, cadence_fields_for, is_ticking_state,
    phase_config_for, template_name_for, ticking_state_names_by_group, CadenceFields, PhaseConfig,
    StateRef, CADENCE_FIELDS, DEFAULT_CADENCE_KEY, KIND_CODING_TASK, PHASES,
};
pub use ticker::{TickerDecision, TickerEvent, TickerEventKind};

/// Python `str.strip()` (no-arg) semantics as a `&str` view.
///
/// Rust `str::trim` uses Unicode `White_Space`, which excludes
/// U+001C..=U+001F; Python strips those too (`'\x1c'.isspace()` is
/// `True`). The union below is exactly Python's strip set.
pub(crate) fn strip_python_whitespace(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python truthiness for JSON values (`if not x`).
///
/// `Null` → false, bools/strings/arrays/objects by emptiness, numbers by
/// zero-ness — mirroring what `or {}` / `or []` / `if not` see in the
/// done-signal and outcome units.
pub(crate) fn is_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
    }
}
