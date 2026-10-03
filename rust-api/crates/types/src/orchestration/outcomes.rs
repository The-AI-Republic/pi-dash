//! Outcome vocabulary, outcome dataclasses, and dispatch constants (D-12, stage 5).
//!
//! Port of the outcome half of `apps/api/pi_dash/orchestration/scheduling.py`:
//!
//! * `OUTCOME_*` (`:74-78`) → [`OUTCOME_PROGRESSED`] et al.
//! * `RUN_OUTCOMES` (`:79-87`) → [`RUN_OUTCOMES`].
//! * `STOPPING_OUTCOMES` (`:89`) → [`STOPPING_OUTCOMES`].
//! * `_LEGACY_OUTCOME_ALIASES` (`:94-97`) → [`LEGACY_OUTCOME_ALIASES`].
//! * `normalize_outcome` (`:333-350`) → [`normalize_outcome`].
//! * `outcome_for_run` (`:353-357`) → [`outcome_for_run`].
//! * `default_outcome_for_kind` (`:360-371`) → [`default_outcome_for_kind`].
//! * `WAIT_*` (`:742-754`) → [`WAIT_GRANTED`] et al.
//! * `_WAIT_REARMABLE_DISARMS` (`:758-763`) → [`WAIT_REARMABLE_DISARMS`].
//! * `TRIGGER_*` (`:863-865`) → [`TRIGGER_TICK`] et al.
//! * `_MACHINE_TRIGGERS` (`:868`) → [`MACHINE_TRIGGERS`].
//! * `RUN_AI_*` (`:1223-1225`) → [`RUN_AI_ACTIVE_RUN_EXISTS`] et al.
//! * `PAUSED_STATE_NAME` (`:46`) → [`PAUSED_STATE_NAME`].
//! * `DELEGATION_STATE_NAME` (`:54`, deprecated) → [`SCHEDULING_DELEGATION_STATE_NAME`].
//!
//! Port of the outcome half of `apps/api/pi_dash/orchestration/service.py`:
//!
//! * `TransitionOutcome` (`:67-72`) → [`TransitionOutcome`].
//! * `ContinuationOutcome` (`:75-81`) → [`ContinuationOutcome`].
//! * `CONTINUATION_ELIGIBLE_GROUPS` (`:346`) → [`CONTINUATION_ELIGIBLE_GROUPS`].
//! * `PROJECT_MOVE_HANDOFF_CONFIG_KEY` (`:57`) → [`PROJECT_MOVE_HANDOFF_CONFIG_KEY`].
//! * `DELEGATION_STATE_NAME` (`:64`, deprecated) → [`SERVICE_DELEGATION_STATE_NAME`].
//!
//! Translation notes:
//!
//! * `normalize_outcome` takes a JSON value because both Python call
//!   sites pass dynamic payloads (`payload.get("status")`); non-strings
//!   answer `None`, as the `isinstance` guard does.
//! * `outcome_for_run` takes [`RunOutcomeRef`] — the `done_payload` /
//!   `phase_kind` the function reads — never a row.
//! * `TRIGGER_*` and the full `AgentRunTrigger` value map are leaf
//!   literals of `runner/models.py:248-253`; `types` cannot import the
//!   runner, so the values live here with traces, pinned by FX-ORCH-01.
//! * `_PROMPT_BUILD_ERRORS` (`service.py:51`) has no value-level
//!   translation: it only groups `except` clauses, which the services
//!   catch sites (L6/L7) express by matching the three prompting error
//!   variants directly.
//!
//! Fixtures: `rust-api/fixtures/orchestration/fx01_types/outcomes.golden.json`,
//! `dispatch_constants.golden.json`, and the outcome-const slices of
//! `ticker_primitives.golden.json` (FX-ORCH-01).
//!
//! Ported bugs: none found in this unit on read-through.

use serde_json::Value;
use uuid::Uuid;

/// Outcomes a run may report through `pidash run yield`
/// (`scheduling.py:74-78`).
pub const OUTCOME_PROGRESSED: &str = "progressed";
pub const OUTCOME_WAITING_ON_HUMAN: &str = "waiting_on_human";
pub const OUTCOME_WAITING_ON_EXTERNAL: &str = "waiting_on_external";
pub const OUTCOME_DONE: &str = "done";
pub const OUTCOME_BLOCKED: &str = "blocked";

/// The §7 outcome vocabulary (`scheduling.py:79-87`), in source order.
pub const RUN_OUTCOMES: &[&str] = &[
    OUTCOME_PROGRESSED,
    OUTCOME_WAITING_ON_HUMAN,
    OUTCOME_WAITING_ON_EXTERNAL,
    OUTCOME_DONE,
    OUTCOME_BLOCKED,
];

/// Outcomes that stop the clock for the rendered stage
/// (`scheduling.py:89`), in source order.
pub const STOPPING_OUTCOMES: &[&str] = &[OUTCOME_DONE, OUTCOME_BLOCKED, OUTCOME_WAITING_ON_HUMAN];

/// Legacy done-payload statuses mapped onto the §7 vocabulary
/// (`scheduling.py:94-97`), in source order.
pub const LEGACY_OUTCOME_ALIASES: &[(&str, &str)] = &[
    ("completed", OUTCOME_DONE),
    ("paused", OUTCOME_WAITING_ON_HUMAN),
];

/// Map a done-payload `status` onto the §7 outcome vocabulary
/// (`scheduling.py:333-350`).
///
/// `None` for anything unrecognised (including non-strings); legacy
/// `noop` follows the per-kind default.
pub fn normalize_outcome(value: &Value, phase_kind: &str) -> Option<&'static str> {
    let text = value.as_str()?;
    let lowered = super::strip_python_whitespace(text).to_lowercase();
    match lowered.as_str() {
        "progressed" => Some(OUTCOME_PROGRESSED),
        "waiting_on_human" => Some(OUTCOME_WAITING_ON_HUMAN),
        "waiting_on_external" => Some(OUTCOME_WAITING_ON_EXTERNAL),
        "done" => Some(OUTCOME_DONE),
        "blocked" => Some(OUTCOME_BLOCKED),
        "noop" => Some(default_outcome_for_kind(phase_kind)),
        "completed" => Some(OUTCOME_DONE),
        "paused" => Some(OUTCOME_WAITING_ON_HUMAN),
        _ => None,
    }
}

/// Minimal `done_payload` + `phase_kind` view of a run — the only two
/// attributes `outcome_for_run` reads. Callers re-read their own rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunOutcomeRef<'a> {
    pub done_payload: Option<&'a Value>,
    pub phase_kind: &'a str,
}

/// The outcome a run reported, if any (`scheduling.py:353-357`).
///
/// A missing/falsy payload behaves like `{}` (no status → `None`), and a
/// truthy non-dict payload answers `None`, as the `or {}` /
/// `isinstance` guards do.
pub fn outcome_for_run(run: &RunOutcomeRef<'_>) -> Option<&'static str> {
    let payload = run.done_payload.filter(|v| super::is_truthy(v))?;
    let obj = payload.as_object()?;
    normalize_outcome(obj.get("status").unwrap_or(&Value::Null), run.phase_kind)
}

/// What a run that exited without yielding is taken to mean
/// (`scheduling.py:360-371`): `coding-task` (and the empty kind) keeps
/// ticking, anything else is satisfied.
pub fn default_outcome_for_kind(phase_kind: &str) -> &'static str {
    if phase_kind.is_empty() || phase_kind == super::phases::KIND_CODING_TASK {
        OUTCOME_PROGRESSED
    } else {
        OUTCOME_DONE
    }
}

/// Leaf literals of `AgentRunTrigger` (`runner/models.py:248-253`),
/// verbatim. The three `TRIGGER_*` names are `scheduling.py:863-865`;
/// the other three complete the enum map the fixture pins.
pub const TRIGGER_STATE_TRANSITION: &str = "state_transition";
pub const TRIGGER_RUN_AI: &str = "run_ai";
pub const TRIGGER_COMMENT_AND_RUN: &str = "comment_and_run";
pub const TRIGGER_TICK: &str = "tick";
pub const TRIGGER_SCHEDULER: &str = "scheduler";
pub const TRIGGER_DIRECT: &str = "direct";

/// Triggers the clock starts on its own (`scheduling.py:868`).
pub const MACHINE_TRIGGERS: &[&str] = &[TRIGGER_TICK];

/// Machine-readable refusal reasons for a "Run AI" dispatch that produced
/// no run (`scheduling.py:1223-1225`).
pub const RUN_AI_ACTIVE_RUN_EXISTS: &str = "active_run_exists";
pub const RUN_AI_NO_POD: &str = "no_pod";
pub const RUN_AI_NO_ELIGIBLE_RUNNER: &str = "no_eligible_runner";

/// `wait_ticker` results (`scheduling.py:742-751`).
pub const WAIT_GRANTED: &str = "waited";
pub const WAIT_CAP_REACHED: &str = "wait_cap_reached";
pub const WAIT_INFINITE_POOL: &str = "infinite_pool";
pub const WAIT_NO_TICKER: &str = "no_ticker";

/// `IssueActivity.field` recorded on every wait call (`scheduling.py:754`).
pub const WAIT_ACTIVITY_FIELD: &str = "agent_wait";

/// Disarm reasons a wait may lift (`scheduling.py:758-763`): the values of
/// `TickerDisarmReason.CAP_HIT` / `POOL_SPENT`
/// (`db/models/issue_agent_ticker.py:57-61`).
pub const WAIT_REARMABLE_DISARMS: &[&str] = &["cap_hit", "pool_spent"];

/// The project's auto-pause parking state (`scheduling.py:46`).
pub const PAUSED_STATE_NAME: &str = "Paused";

/// DEPRECATED (`scheduling.py:54`): retained only for backward
/// compatibility with external importers.
#[deprecated(note = "use phases::is_ticking_state / phases::phase_config_for")]
pub const SCHEDULING_DELEGATION_STATE_NAME: &str = "In Progress";

/// DEPRECATED (`service.py:64`): retained only for backward compatibility
/// with external importers.
#[deprecated(note = "use phases::is_ticking_state / phases::phase_config_for")]
pub const SERVICE_DELEGATION_STATE_NAME: &str = "In Progress";

/// What `handle_issue_state_transition` decided to do (`service.py:67-72`).
///
/// Field order mirrors the dataclass. The run reference is its id.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransitionOutcome {
    pub created_run: Option<Uuid>,
    pub reason: String,
}

/// What `handle_issue_comment` decided to do (`service.py:75-81`).
///
/// Field order mirrors the dataclass. Run references are their ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContinuationOutcome {
    pub created_run: Option<Uuid>,
    pub coalesced_into: Option<Uuid>,
    pub reason: String,
}

/// Issue state groups in which a comment may wake the agent
/// (`service.py:346`): derived from the phase registry, in `PHASES` key
/// order. The ordering test below enforces the derivation.
pub const CONTINUATION_ELIGIBLE_GROUPS: &[&str] = &["started", "review", "test"];

/// Handoff intent key on a run's config while a cross-project move waits
/// for the source runner (`service.py:57`).
pub const PROJECT_MOVE_HANDOFF_CONFIG_KEY: &str = "_project_move_handoff";

#[cfg(test)]
mod tests {
    use super::super::phases::PHASES;
    use super::*;
    use serde_json::{json, Map};

    static OUTCOMES_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx01_types/outcomes.golden.json");
    static CONSTANTS_FIXTURE: &str = include_str!(
        "../../../../fixtures/orchestration/fx01_types/dispatch_constants.golden.json"
    );
    static PRIMITIVES_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx01_types/ticker_primitives.golden.json");

    #[test]
    fn outcome_sets_match_fixture() {
        let golden: Value = serde_json::from_str(OUTCOMES_FIXTURE).expect("fixture parses");
        assert_eq!(
            OUTCOME_PROGRESSED,
            golden["OUTCOME_PROGRESSED"].as_str().unwrap()
        );
        assert_eq!(
            OUTCOME_WAITING_ON_HUMAN,
            golden["OUTCOME_WAITING_ON_HUMAN"].as_str().unwrap()
        );
        assert_eq!(
            OUTCOME_WAITING_ON_EXTERNAL,
            golden["OUTCOME_WAITING_ON_EXTERNAL"].as_str().unwrap()
        );
        assert_eq!(OUTCOME_DONE, golden["OUTCOME_DONE"].as_str().unwrap());
        assert_eq!(OUTCOME_BLOCKED, golden["OUTCOME_BLOCKED"].as_str().unwrap());

        let mut run = RUN_OUTCOMES.to_vec();
        run.sort_unstable();
        assert_eq!(json!(run), golden["RUN_OUTCOMES"]);
        let mut stopping = STOPPING_OUTCOMES.to_vec();
        stopping.sort_unstable();
        assert_eq!(json!(stopping), golden["STOPPING_OUTCOMES"]);

        let aliases: Map<String, Value> = LEGACY_OUTCOME_ALIASES
            .iter()
            .map(|(k, v)| ((*k).to_string(), json!(v)))
            .collect();
        assert_eq!(Value::Object(aliases), golden["_LEGACY_OUTCOME_ALIASES"]);
    }

    #[test]
    fn default_outcome_vectors_match_fixture() {
        let golden: Value = serde_json::from_str(OUTCOMES_FIXTURE).expect("fixture parses");
        let cases = golden["default_outcome_for_kind"].as_array().unwrap();
        for case in cases {
            assert_eq!(
                default_outcome_for_kind(case["phase_kind"].as_str().unwrap()),
                case["result"].as_str().unwrap(),
                "kind {:?}",
                case["phase_kind"]
            );
        }
        assert_eq!(cases.len(), 5, "all default vectors replayed");
    }

    fn typed_fixture_value(case: &Value) -> Value {
        let raw = case["value"].as_str().unwrap();
        match case.get("value_type").and_then(Value::as_str) {
            None => Value::String(raw.to_string()),
            Some("NoneType") => Value::Null,
            Some("int") => json!(raw.parse::<i64>().unwrap()),
            Some("float") => json!(raw.parse::<f64>().unwrap()),
            Some("dict" | "list") => serde_json::from_str(raw).unwrap(),
            Some("bool") => Value::Bool(raw == "True"),
            // Bytes have no JSON spelling; as a string the value is an
            // unknown outcome, which answers `None` like the guard does.
            Some("bytes") => Value::String(raw.to_string()),
            Some(other) => panic!("unknown value_type {other}"),
        }
    }

    #[test]
    fn normalize_matrix_matches_fixture() {
        let golden: Value = serde_json::from_str(OUTCOMES_FIXTURE).expect("fixture parses");
        let cases = golden["normalize_outcome"].as_array().unwrap();
        for case in cases {
            let value = typed_fixture_value(case);
            let expected = case["result"].as_str().map(str::to_string);
            assert_eq!(
                normalize_outcome(&value, case["phase_kind"].as_str().unwrap()).map(str::to_string),
                expected,
                "value {:?} kind {:?}",
                case["value"],
                case["phase_kind"]
            );
        }
        assert_eq!(cases.len(), 33, "all normalize rows replayed");
    }

    #[test]
    fn outcome_for_run_vectors_match_fixture() {
        let golden: Value = serde_json::from_str(OUTCOMES_FIXTURE).expect("fixture parses");
        let cases = golden["outcome_for_run"].as_array().unwrap();
        for case in cases {
            let payload = case.get("done_payload").unwrap();
            let input = RunOutcomeRef {
                done_payload: if payload.is_null() {
                    None
                } else {
                    Some(payload)
                },
                phase_kind: case["phase_kind"].as_str().unwrap(),
            };
            let expected = case["result"].as_str().map(str::to_string);
            assert_eq!(
                outcome_for_run(&input).map(str::to_string),
                expected,
                "payload {:?}",
                case["done_payload"]
            );
        }
        assert_eq!(cases.len(), 11, "all outcome_for_run rows replayed");
    }

    #[test]
    fn dispatch_constants_match_fixture() {
        let golden: Value = serde_json::from_str(CONSTANTS_FIXTURE).expect("fixture parses");
        let triggers = &golden["AgentRunTrigger"];
        assert_eq!(
            TRIGGER_STATE_TRANSITION,
            triggers["STATE_TRANSITION"].as_str().unwrap()
        );
        assert_eq!(TRIGGER_RUN_AI, triggers["RUN_AI"].as_str().unwrap());
        assert_eq!(
            TRIGGER_COMMENT_AND_RUN,
            triggers["COMMENT_AND_RUN"].as_str().unwrap()
        );
        assert_eq!(TRIGGER_TICK, triggers["TICK"].as_str().unwrap());
        assert_eq!(TRIGGER_SCHEDULER, triggers["SCHEDULER"].as_str().unwrap());
        assert_eq!(TRIGGER_DIRECT, triggers["DIRECT"].as_str().unwrap());
        assert_eq!(TRIGGER_TICK, golden["TRIGGER_TICK"].as_str().unwrap());
        assert_eq!(
            TRIGGER_COMMENT_AND_RUN,
            golden["TRIGGER_COMMENT_AND_RUN"].as_str().unwrap()
        );
        assert_eq!(TRIGGER_RUN_AI, golden["TRIGGER_RUN_AI"].as_str().unwrap());
        assert_eq!(json!(MACHINE_TRIGGERS), golden["_MACHINE_TRIGGERS"]);

        assert_eq!(
            RUN_AI_ACTIVE_RUN_EXISTS,
            golden["RUN_AI_ACTIVE_RUN_EXISTS"].as_str().unwrap()
        );
        assert_eq!(RUN_AI_NO_POD, golden["RUN_AI_NO_POD"].as_str().unwrap());
        assert_eq!(
            RUN_AI_NO_ELIGIBLE_RUNNER,
            golden["RUN_AI_NO_ELIGIBLE_RUNNER"].as_str().unwrap()
        );

        assert_eq!(WAIT_GRANTED, golden["WAIT_GRANTED"].as_str().unwrap());
        assert_eq!(
            WAIT_CAP_REACHED,
            golden["WAIT_CAP_REACHED"].as_str().unwrap()
        );
        assert_eq!(
            WAIT_INFINITE_POOL,
            golden["WAIT_INFINITE_POOL"].as_str().unwrap()
        );
        assert_eq!(WAIT_NO_TICKER, golden["WAIT_NO_TICKER"].as_str().unwrap());
        assert_eq!(
            WAIT_ACTIVITY_FIELD,
            golden["WAIT_ACTIVITY_FIELD"].as_str().unwrap()
        );
        assert_eq!(
            json!(WAIT_REARMABLE_DISARMS),
            golden["_WAIT_REARMABLE_DISARMS"]
        );
    }

    #[test]
    #[allow(deprecated)]
    fn state_names_and_outcome_defaults_match_fixture() {
        let golden: Value = serde_json::from_str(PRIMITIVES_FIXTURE).expect("fixture parses");
        assert_eq!(
            PAUSED_STATE_NAME,
            golden["PAUSED_STATE_NAME"].as_str().unwrap()
        );
        assert_eq!(
            SCHEDULING_DELEGATION_STATE_NAME,
            golden["scheduling.DELEGATION_STATE_NAME"].as_str().unwrap()
        );
        assert_eq!(
            SERVICE_DELEGATION_STATE_NAME,
            golden["service.DELEGATION_STATE_NAME"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::to_value(TransitionOutcome::default()).unwrap(),
            golden["TransitionOutcome.defaults"]
        );
        assert_eq!(
            serde_json::to_value(ContinuationOutcome::default()).unwrap(),
            golden["ContinuationOutcome.defaults"]
        );
        assert_eq!(
            PROJECT_MOVE_HANDOFF_CONFIG_KEY, "_project_move_handoff",
            "handoff key (`service.py:57`)"
        );
    }

    #[test]
    fn continuation_groups_derive_from_phases() {
        let keys: Vec<&str> = PHASES.iter().map(|(group, _)| *group).collect();
        assert_eq!(CONTINUATION_ELIGIBLE_GROUPS, keys.as_slice());
    }

    // Oracle-verified regression pins (live Django tree, 2026-10-03)
    // beyond the fixture goldens.

    #[test]
    fn normalize_unicode_edges_match_python() {
        // Python strips U+001C..=U+001F, NBSP, NEL, and em-space; the
        // kind comparison is an exact match (no case/space folding).
        let cases: &[(&str, &str, Option<&str>)] = &[
            ("DONE\x1c", "", Some("done")),
            ("\u{a0}done\u{a0}", "", Some("done")),
            ("done\u{85}", "", Some("done")),
            ("\u{2003}done\u{2003}", "", Some("done")),
            ("İ", "", None),
            ("noop", "CODING-TASK", Some("done")),
            ("noop", " coding-task ", Some("done")),
        ];
        for (value, kind, expected) in cases {
            assert_eq!(
                normalize_outcome(&Value::String((*value).to_string()), kind),
                *expected,
                "{value:?} / {kind:?}"
            );
        }
        assert_eq!(default_outcome_for_kind("Coding-Task"), "done");
    }
}
