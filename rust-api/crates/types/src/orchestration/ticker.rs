//! Ticker event and decision types (D-12, stage 5).
//!
//! Port of the event half of `apps/api/pi_dash/orchestration/scheduling.py`:
//!
//! * `TickerEventKind` (`:100-106`) → [`TickerEventKind`].
//! * `TickerEvent` (`:109-183`) → [`TickerEvent`].
//! * `TickerDecision` (`:186-202`) → [`TickerDecision`].
//!
//! Translation notes:
//!
//! * Row references become ids: runs and actors are [`Uuid`]s (both PKs
//!   are UUIDs), the decision's ticker is the ticker row id. Handlers
//!   re-read rows; this crate does no I/O.
//! * The stamped trigger values come from [`super::outcomes`] (`TRIGGER_*`
//!   leaf literals of `AgentRunTrigger`, verbatim).
//!
//! Fixture: `rust-api/fixtures/orchestration/fx01_types/ticker_primitives.golden.json`
//! (FX-ORCH-01).
//!
//! Ported bugs: none found in this unit on read-through.

use super::outcomes::{TRIGGER_RUN_AI, TRIGGER_STATE_TRANSITION};
use uuid::Uuid;

/// Kinds of clock events (`scheduling.py:100-106`).
pub struct TickerEventKind;

impl TickerEventKind {
    pub const ENTERED_BUCKET: &'static str = "entered_bucket";
    pub const MOVED_STAGE: &'static str = "moved_stage";
    pub const LEFT_BUCKET: &'static str = "left_bucket";
    pub const RUN_ENDED: &'static str = "run_ended";
    pub const HUMAN_RUN_REQUESTED: &'static str = "human_run_requested";
    pub const RETICK: &'static str = "retick";
}

/// One thing that happened to an issue that the clock must react to
/// (`scheduling.py:109-183`).
///
/// Field order mirrors the dataclass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TickerEvent {
    pub kind: String,
    pub moved_by_run: Option<Uuid>,
    pub resume_parent: Option<Uuid>,
    pub run: Option<Uuid>,
    pub outcome: Option<String>,
    pub want_run: bool,
    pub actor: Option<Uuid>,
    pub trigger: String,
}

impl TickerEvent {
    /// The issue entered the ticking bucket (`scheduling.py:137-146`).
    pub fn entered_bucket(
        moved_by_run: Option<Uuid>,
        resume_parent: Option<Uuid>,
        want_run: bool,
        actor: Option<Uuid>,
    ) -> Self {
        Self {
            kind: TickerEventKind::ENTERED_BUCKET.to_string(),
            moved_by_run,
            resume_parent,
            run: None,
            outcome: None,
            want_run,
            actor,
            trigger: TRIGGER_STATE_TRANSITION.to_string(),
        }
    }

    /// The issue moved within the ticking bucket (`scheduling.py:148-157`).
    pub fn moved_stage(
        moved_by_run: Option<Uuid>,
        resume_parent: Option<Uuid>,
        want_run: bool,
        actor: Option<Uuid>,
    ) -> Self {
        Self {
            kind: TickerEventKind::MOVED_STAGE.to_string(),
            moved_by_run,
            resume_parent,
            run: None,
            outcome: None,
            want_run,
            actor,
            trigger: TRIGGER_STATE_TRANSITION.to_string(),
        }
    }

    /// The issue left the ticking bucket (`scheduling.py:159-161`).
    pub fn left_bucket() -> Self {
        Self {
            kind: TickerEventKind::LEFT_BUCKET.to_string(),
            moved_by_run: None,
            resume_parent: None,
            run: None,
            outcome: None,
            want_run: true,
            actor: None,
            trigger: String::new(),
        }
    }

    /// A run ended with an outcome (`None` when it exited without
    /// yielding) (`scheduling.py:163-165`).
    pub fn run_ended(run: Option<Uuid>, outcome: Option<&str>) -> Self {
        Self {
            kind: TickerEventKind::RUN_ENDED.to_string(),
            moved_by_run: None,
            resume_parent: None,
            run,
            outcome: outcome.map(str::to_string),
            want_run: true,
            actor: None,
            trigger: String::new(),
        }
    }

    /// A human asked for a run (`scheduling.py:167-174`). An empty
    /// trigger falls back to the Run AI trigger, as `trigger or ...` does.
    pub fn human_run_requested(want_run: bool, actor: Option<Uuid>, trigger: &str) -> Self {
        Self {
            kind: TickerEventKind::HUMAN_RUN_REQUESTED.to_string(),
            moved_by_run: None,
            resume_parent: None,
            run: None,
            outcome: None,
            want_run,
            actor,
            trigger: if trigger.is_empty() {
                TRIGGER_RUN_AI.to_string()
            } else {
                trigger.to_string()
            },
        }
    }

    /// A Re-tick asked for budget (`scheduling.py:176-183`).
    pub fn retick(want_run: bool, actor: Option<Uuid>) -> Self {
        Self {
            kind: TickerEventKind::RETICK.to_string(),
            moved_by_run: None,
            resume_parent: None,
            run: None,
            outcome: None,
            want_run,
            actor,
            trigger: TRIGGER_RUN_AI.to_string(),
        }
    }
}

/// What `reconcile` decided, for the caller to act on
/// (`scheduling.py:186-202`).
///
/// Field order mirrors the dataclass. `ticker` is the ticker row id.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TickerDecision {
    pub ticker: Option<Uuid>,
    pub dispatch_now: bool,
    pub queued: bool,
    pub parked: bool,
    pub granted: bool,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx01_types/ticker_primitives.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn event_kinds_match_fixture() {
        let golden = fixture();
        let kinds = &golden["TickerEventKind"];
        assert_eq!(
            TickerEventKind::ENTERED_BUCKET,
            kinds["ENTERED_BUCKET"].as_str().unwrap()
        );
        assert_eq!(
            TickerEventKind::MOVED_STAGE,
            kinds["MOVED_STAGE"].as_str().unwrap()
        );
        assert_eq!(
            TickerEventKind::LEFT_BUCKET,
            kinds["LEFT_BUCKET"].as_str().unwrap()
        );
        assert_eq!(
            TickerEventKind::RUN_ENDED,
            kinds["RUN_ENDED"].as_str().unwrap()
        );
        assert_eq!(
            TickerEventKind::HUMAN_RUN_REQUESTED,
            kinds["HUMAN_RUN_REQUESTED"].as_str().unwrap()
        );
        assert_eq!(TickerEventKind::RETICK, kinds["RETICK"].as_str().unwrap());
    }

    #[test]
    fn constructors_match_fixture() {
        let golden = fixture();
        let cases = &golden["constructors"];
        let built: &[(&str, TickerEvent)] = &[
            (
                "entered_bucket.defaults",
                TickerEvent::entered_bucket(None, None, true, None),
            ),
            (
                "entered_bucket.no_want_run",
                TickerEvent::entered_bucket(None, None, false, None),
            ),
            (
                "moved_stage.defaults",
                TickerEvent::moved_stage(None, None, true, None),
            ),
            (
                "moved_stage.no_want_run",
                TickerEvent::moved_stage(None, None, false, None),
            ),
            ("left_bucket", TickerEvent::left_bucket()),
            (
                "run_ended.with_outcome",
                TickerEvent::run_ended(None, Some("progressed")),
            ),
            ("run_ended.no_outcome", TickerEvent::run_ended(None, None)),
            (
                "human_run_requested.defaults",
                TickerEvent::human_run_requested(true, None, ""),
            ),
            (
                "human_run_requested.custom_trigger",
                TickerEvent::human_run_requested(true, None, "custom-trigger"),
            ),
            (
                "human_run_requested.no_want_run",
                TickerEvent::human_run_requested(false, None, ""),
            ),
            ("retick.defaults", TickerEvent::retick(true, None)),
            ("retick.no_want_run", TickerEvent::retick(false, None)),
        ];
        for (label, event) in built {
            assert_eq!(
                serde_json::to_value(event).unwrap(),
                cases[*label],
                "{label}"
            );
        }
        assert_eq!(
            cases.as_object().unwrap().len(),
            built.len(),
            "all ctors replayed"
        );
    }

    #[test]
    fn decision_defaults_match_fixture() {
        let golden = fixture();
        assert_eq!(
            serde_json::to_value(TickerDecision::default()).unwrap(),
            golden["TickerDecision.defaults"]
        );
    }
}
