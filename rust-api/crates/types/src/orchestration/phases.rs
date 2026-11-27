//! Phase registry for the issue ticking system (D-12, stage 5).
//!
//! Port of `apps/api/pi_dash/orchestration/agent_phases.py` (whole file):
//!
//! * `CadenceFields` (`:32-46`) → [`CadenceFields`].
//! * `CADENCE_FIELDS` (`:53-66`) → [`CADENCE_FIELDS`].
//! * `DEFAULT_CADENCE_KEY` (`:71`) → [`DEFAULT_CADENCE_KEY`].
//! * `PhaseConfig` (`:74-114`) → [`PhaseConfig`].
//! * `PHASES` (`:117-146`) → [`PHASES`].
//! * the Python ticking check (`:149-162`) → split by question (lean agent
//!   workflow): [`phase_config_for`] answers "has an agent prompt",
//!   [`stage_runs_automatically`] answers "entry runs and ticks happen".
//! * `phase_config_for` (`:165-176`) → [`phase_config_for`].
//! * `template_name_for` (`:179-188`) → [`template_name_for`].
//! * `cadence_fields_for` (`:191-200`) → [`cadence_fields_for`].
//! * `auto_pauses_on_cap` (`:203-212`) → [`auto_pauses_on_cap`].
//! * `cadence_fields_by_group` (`:215-219`) → [`cadence_fields_by_group`].
//! * `ticking_state_names_by_group` (`:222-224`) → [`ticking_state_names_by_group`].
//!
//! Translation notes:
//!
//! * Python takes a duck-typed `state` (`state.group` / `state.name`);
//!   the port takes [`StateRef`], the minimal `group` + `name` view, so
//!   callers with real rows never hand a row to the types crate.
//! * `KIND_CODING_TASK` is a leaf literal of `prompting/recipes.py:24`.
//!   The `services → types` edge means `types` cannot import
//!   `services::prompting`, so the value lives here with this trace and
//!   is pinned by FX-ORCH-01.
//!
//! Fixture: `rust-api/fixtures/orchestration/fx01_types/phases.golden.json`
//! (FX-ORCH-01).
//!
//! Ported bugs: none found in this unit on read-through.

/// Leaf literal of `prompting/recipes.py:24` (`KIND_CODING_TASK`).
///
/// Pinned by FX-ORCH-01. `types` cannot import `services::prompting`
/// (the crate graph bottoms out here), so the value is local.
pub const KIND_CODING_TASK: &str = "coding-task";

/// Which project column holds one phase's *interval* (`agent_phases.py:32-46`).
///
/// Cadence is rhythm, not budget. Field order mirrors the dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CadenceFields {
    pub project_interval: &'static str,
    pub default_interval: i64,
}

/// Cadence key → the project interval column that phase reads
/// (`agent_phases.py:53-66`), in source order.
pub const CADENCE_FIELDS: &[(&str, CadenceFields)] = &[
    (
        "impl",
        CadenceFields {
            project_interval: "agent_default_interval_seconds",
            default_interval: 10800,
        },
    ),
    (
        "review",
        CadenceFields {
            project_interval: "agent_review_default_interval_seconds",
            default_interval: 10800,
        },
    ),
    (
        "test",
        CadenceFields {
            project_interval: "agent_test_default_interval_seconds",
            default_interval: 10800,
        },
    ),
];

/// Fallback cadence key for states outside the registry
/// (`agent_phases.py:71`).
pub const DEFAULT_CADENCE_KEY: &str = "impl";

/// Static metadata for a ticking phase (`agent_phases.py:74-114`).
///
/// Field order mirrors the dataclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhaseConfig {
    pub state_name: &'static str,
    pub template_name: &'static str,
    pub cadence_key: &'static str,
    pub fresh_session_on_entry: bool,
    pub disarm_on_completed: bool,
    pub auto_pause_on_cap: bool,
}

/// State group → the phase that ticks in it (`agent_phases.py:117-146`),
/// in source order (`started`, `review`, `test`).
pub const PHASES: &[(&str, PhaseConfig)] = &[
    (
        "started",
        PhaseConfig {
            state_name: "In Progress",
            template_name: KIND_CODING_TASK,
            cadence_key: "impl",
            fresh_session_on_entry: false,
            disarm_on_completed: true,
            auto_pause_on_cap: true,
        },
    ),
    (
        "review",
        PhaseConfig {
            state_name: "In Review",
            template_name: "review",
            cadence_key: "review",
            fresh_session_on_entry: true,
            disarm_on_completed: true,
            auto_pause_on_cap: false,
        },
    ),
    (
        "test",
        PhaseConfig {
            state_name: "In Test",
            template_name: "test",
            cadence_key: "test",
            fresh_session_on_entry: true,
            disarm_on_completed: true,
            auto_pause_on_cap: false,
        },
    ),
];

/// Minimal `group` + `name` view of a state row — the only two attributes
/// the registry functions read. Callers re-read their own rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRef<'a> {
    pub group: &'a str,
    pub name: &'a str,
}

/// Which optional stages run automatically in one project (the
/// project's `agent_stages` setting, resolved). In Progress is always
/// on and has no switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSwitches {
    pub review: bool,
    pub test: bool,
}

impl StageSwitches {
    /// Both optional stages on — the default for every project.
    pub const ALL: StageSwitches = StageSwitches {
        review: true,
        test: true,
    };
}

impl Default for StageSwitches {
    fn default() -> Self {
        Self::ALL
    }
}

/// The state an agent finishes a task in (NEWPIDASH-305). Done is a
/// person's confirmation of it.
pub const AI_COMPLETED_STATE_NAME: &str = "AI Completed";

/// Why an issue run may not move its own issue into a state (lean agent
/// workflow §6.5, §7). A person moving an issue is never refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMoveRefusal {
    /// Cancelled, or a completed state other than "AI Completed".
    HumanOnly,
    /// A registered stage the project switched off: entering it starts no
    /// run, so the issue would sit there with nothing to tell anyone why.
    StageOff,
}

/// Whether an issue run may move its issue into `state`; `None` allows it.
pub fn agent_move_refusal(
    state: &StateRef<'_>,
    switches: StageSwitches,
) -> Option<AgentMoveRefusal> {
    if state.group == "cancelled"
        || (state.group == "completed" && state.name != AI_COMPLETED_STATE_NAME)
    {
        return Some(AgentMoveRefusal::HumanOnly);
    }
    if phase_config_for(Some(state)).is_some() && !stage_runs_automatically(Some(state), switches) {
        return Some(AgentMoveRefusal::StageOff);
    }
    None
}

/// True when entry runs and ticks happen in `state`: it is a registered
/// phase, and that phase is In Progress or is switched on in the
/// project. Whether a state *has* an agent prompt is a different
/// question, answered by [`phase_config_for`] / [`template_name_for`].
pub fn stage_runs_automatically(state: Option<&StateRef<'_>>, switches: StageSwitches) -> bool {
    let Some(cfg) = phase_config_for(state) else {
        return false;
    };
    match cfg.cadence_key {
        "review" => switches.review,
        "test" => switches.test,
        _ => true,
    }
}

/// Return the [`PhaseConfig`] for the given state's phase, or `None` when
/// the state is not a registered ticking state (`agent_phases.py:165-176`).
pub fn phase_config_for(state: Option<&StateRef<'_>>) -> Option<&'static PhaseConfig> {
    let state = state?;
    let (_, cfg) = PHASES.iter().find(|(group, _)| *group == state.group)?;
    if state.name != cfg.state_name {
        return None;
    }
    Some(cfg)
}

/// Return the prompt-template name to render for the given state, falling
/// back to the default template name outside the registry
/// (`agent_phases.py:179-188`).
pub fn template_name_for(state: Option<&StateRef<'_>>) -> &'static str {
    phase_config_for(state).map_or(KIND_CODING_TASK, |cfg| cfg.template_name)
}

/// Return the [`CadenceFields`] the given state resolves through.
/// Non-registry states fall back to the implementation pair
/// (`agent_phases.py:191-200`).
pub fn cadence_fields_for(state: Option<&StateRef<'_>>) -> &'static CadenceFields {
    let key = phase_config_for(state).map_or(DEFAULT_CADENCE_KEY, |cfg| cfg.cadence_key);
    CADENCE_FIELDS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, fields)| fields)
        .expect("cadence key is always registered")
}

/// Return `true` when exhausting the budget should auto-Pause the issue.
/// Non-ticking states answer `false` (`agent_phases.py:203-212`).
pub fn auto_pauses_on_cap(state: Option<&StateRef<'_>>) -> bool {
    phase_config_for(state).is_some_and(|cfg| cfg.auto_pause_on_cap)
}

/// Return `state group -> CadenceFields` for every ticking phase, in
/// registry order (`agent_phases.py:215-219`).
pub fn cadence_fields_by_group() -> Vec<(&'static str, &'static CadenceFields)> {
    PHASES
        .iter()
        .map(|(group, cfg)| {
            let fields = CADENCE_FIELDS
                .iter()
                .find(|(k, _)| *k == cfg.cadence_key)
                .map(|(_, fields)| fields)
                .expect("cadence key is always registered");
            (*group, fields)
        })
        .collect()
}

/// Return `state group -> the literal state name that ticks`, in registry
/// order (`agent_phases.py:222-224`).
pub fn ticking_state_names_by_group() -> Vec<(&'static str, &'static str)> {
    PHASES
        .iter()
        .map(|(group, cfg)| (*group, cfg.state_name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    static FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx01_types/phases.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn registry_tables_match_fixture() {
        let golden = fixture();
        assert_eq!(
            KIND_CODING_TASK,
            golden["KIND_CODING_TASK"].as_str().unwrap()
        );
        assert_eq!(
            DEFAULT_CADENCE_KEY,
            golden["DEFAULT_CADENCE_KEY"].as_str().unwrap()
        );

        let cadence: serde_json::Map<String, Value> = CADENCE_FIELDS
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::to_value(v).unwrap()))
            .collect();
        assert_eq!(Value::Object(cadence), golden["CADENCE_FIELDS"]);

        let phases: serde_json::Map<String, Value> = PHASES
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::to_value(v).unwrap()))
            .collect();
        assert_eq!(Value::Object(phases), golden["PHASES"]);

        let by_group: serde_json::Map<String, Value> = cadence_fields_by_group()
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::to_value(v).unwrap()))
            .collect();
        assert_eq!(Value::Object(by_group), golden["cadence_fields_by_group"]);

        let names: serde_json::Map<String, Value> = ticking_state_names_by_group()
            .into_iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        assert_eq!(Value::Object(names), golden["ticking_state_names_by_group"]);
    }

    #[test]
    fn lookup_vectors_match_fixture() {
        let golden = fixture();
        let owned: Vec<(String, String, Value)> = golden["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| {
                let input = &case["input"];
                let pair = if input.is_null() {
                    (String::new(), String::new())
                } else {
                    (
                        input["group"].as_str().unwrap().to_string(),
                        input["name"].as_str().unwrap().to_string(),
                    )
                };
                (pair.0, pair.1, case.clone())
            })
            .collect();
        for (group, name, case) in &owned {
            let state = if case["input"].is_null() {
                None
            } else {
                Some(StateRef {
                    group: group.as_str(),
                    name: name.as_str(),
                })
            };
            let label = case["case"].as_str().unwrap();
            assert_eq!(
                stage_runs_automatically(state.as_ref(), StageSwitches::ALL),
                case["stage_runs_automatically"].as_bool().unwrap(),
                "{label}: stage_runs_automatically"
            );
            assert_eq!(
                phase_config_for(state.as_ref()).map(|cfg| serde_json::to_value(cfg).unwrap()),
                Some(case["phase_config_for"].clone()).filter(|v| !v.is_null()),
                "{label}: phase_config_for"
            );
            assert_eq!(
                template_name_for(state.as_ref()),
                case["template_name_for"].as_str().unwrap(),
                "{label}: template_name_for"
            );
            assert_eq!(
                serde_json::to_value(cadence_fields_for(state.as_ref())).unwrap(),
                case["cadence_fields_for"],
                "{label}: cadence_fields_for"
            );
            assert_eq!(
                auto_pauses_on_cap(state.as_ref()),
                case["auto_pauses_on_cap"].as_bool().unwrap(),
                "{label}: auto_pauses_on_cap"
            );
        }
        assert_eq!(owned.len(), 13, "all fixture vectors replayed");
    }

    #[test]
    fn stage_runs_automatically_honours_switches() {
        let progress = StateRef {
            group: "started",
            name: "In Progress",
        };
        let review = StateRef {
            group: "review",
            name: "In Review",
        };
        let test = StateRef {
            group: "test",
            name: "In Test",
        };
        let other_review = StateRef {
            group: "review",
            name: "Code Review",
        };
        let done = StateRef {
            group: "completed",
            name: "Done",
        };
        for (r, t) in [(true, true), (true, false), (false, true), (false, false)] {
            let sw = StageSwitches { review: r, test: t };
            assert!(stage_runs_automatically(Some(&progress), sw));
            assert_eq!(stage_runs_automatically(Some(&review), sw), r);
            assert_eq!(stage_runs_automatically(Some(&test), sw), t);
            assert!(!stage_runs_automatically(Some(&other_review), sw));
            assert!(!stage_runs_automatically(Some(&done), sw));
            assert!(!stage_runs_automatically(None, sw));
            // The prompt question is unaffected by the switches.
            assert!(phase_config_for(Some(&review)).is_some());
            assert!(phase_config_for(Some(&test)).is_some());
        }
    }

    #[test]
    fn agent_move_refusal_rules() {
        let on = StageSwitches::ALL;
        let review_off = StageSwitches {
            review: false,
            test: true,
        };
        let st = |group, name| StateRef { group, name };
        assert_eq!(
            agent_move_refusal(&st("completed", "Done"), on),
            Some(AgentMoveRefusal::HumanOnly)
        );
        assert_eq!(
            agent_move_refusal(&st("cancelled", "Cancelled"), on),
            Some(AgentMoveRefusal::HumanOnly)
        );
        assert_eq!(
            agent_move_refusal(&st("completed", "AI Completed"), on),
            None
        );
        assert_eq!(
            agent_move_refusal(&st("review", "In Review"), review_off),
            Some(AgentMoveRefusal::StageOff)
        );
        assert_eq!(agent_move_refusal(&st("review", "In Review"), on), None);
        assert_eq!(
            agent_move_refusal(&st("started", "In Progress"), review_off),
            None
        );
        assert_eq!(
            agent_move_refusal(&st("unstarted", "Todo"), review_off),
            None
        );
    }
}
