//! Agent executor kinds (D-11, stage 5).
//!
//! Port of the executor-kind slice of
//! `apps/api/pi_dash/core/agent_execution.py:7-31`:
//!
//! * `AgentExecutorKind` (`:7-16`) → [`AgentExecutorKind`].
//! * `MACHINE_EXECUTORS` (`:23`) → [`MACHINE_EXECUTORS`].
//! * `DEPRECATED_EXECUTORS` (`:36`) → [`DEPRECATED_EXECUTORS`].
//! * `normalize_executor_write` (`:39-45`) → [`normalize_executor_write`].
//! * `get_default_agent_executor` (`:48-55`) → [`get_default_agent_executor`].
//!
//! Only this slice is ported here; the rest of `agent_execution.py`
//! (`cloud_agent_is_configured`, `managed_runner_is_enabled`,
//! `effective_executor_for_issue`, `user_has_llm_config`,
//! `agent_executor_options`) belongs to the policy layers (FX-DISP-03).
//!
//! Translation notes:
//!
//! * `TextChoices` members are `str` values with labels; the enum carries
//!   both ([`AgentExecutorKind::value`], [`AgentExecutorKind::label`]).
//!   `Display` renders the value, as `str(member)` does in Django. Serde
//!   reads/writes the value; unknown values are rejected, as Django choice
//!   validation would.
//! * `get_default_agent_executor` (#2903) ignores the
//!   `DEFAULT_AGENT_EXECUTOR` setting entirely — there is no setting read
//!   left to port; any other value is reported by the `managed.W001`
//!   system check and ignored here.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json`
//! (`agent_executor_kind`, `get_default_agent_executor`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

/// Executor flavors (`agent_execution.py:7-16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutorKind {
    LocalRunner,
    CloudAgent,
    ManagedRunner,
}

impl AgentExecutorKind {
    /// The `TextChoices` value (`local_runner`, …).
    pub fn value(&self) -> &'static str {
        match self {
            AgentExecutorKind::LocalRunner => "local_runner",
            AgentExecutorKind::CloudAgent => "cloud_agent",
            AgentExecutorKind::ManagedRunner => "managed_runner",
        }
    }

    /// The human label (`agent_execution.py:8-16`).
    pub fn label(&self) -> &'static str {
        match self {
            AgentExecutorKind::LocalRunner => "Local Runner",
            AgentExecutorKind::CloudAgent => "Pi Dash Cloud Agent",
            AgentExecutorKind::ManagedRunner => "Pi Dash Agent",
        }
    }

    /// Parse a stored value (`AgentExecutorKind.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "local_runner" => Some(AgentExecutorKind::LocalRunner),
            "cloud_agent" => Some(AgentExecutorKind::CloudAgent),
            "managed_runner" => Some(AgentExecutorKind::ManagedRunner),
            _ => None,
        }
    }

    /// `self in MACHINE_EXECUTORS` — every "is this local work" branch must
    /// use this rather than comparing against `LocalRunner` alone, or
    /// managed runs silently take the Cloud Agent path
    /// (`agent_execution.py:19-22`).
    pub fn is_machine_executor(&self) -> bool {
        MACHINE_EXECUTORS.contains(self)
    }
}

impl std::fmt::Display for AgentExecutorKind {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Executors whose runs execute on a machine through the `pidash` daemon
/// (`agent_execution.py:23`), in tuple order.
pub const MACHINE_EXECUTORS: [AgentExecutorKind; 2] = [
    AgentExecutorKind::LocalRunner,
    AgentExecutorKind::ManagedRunner,
];

/// Executor values no longer offered for issues and projects
/// (`agent_execution.py:36`), in tuple order. Still accepted on write
/// from older clients and translated to `local_runner`.
pub const DEPRECATED_EXECUTORS: [AgentExecutorKind; 2] = [
    AgentExecutorKind::CloudAgent,
    AgentExecutorKind::ManagedRunner,
];

/// The stored value for an executor written by a client
/// (`agent_execution.py:39-45`): deprecated choices become
/// `local_runner`.
///
/// `value` is the non-null write payload (`None` inherit/clear and
/// unknown values are the caller's own validation — a `None` return
/// here means unknown, for the caller's `unknown agent executor`
/// refusal).
pub fn normalize_executor_write(value: &str) -> Option<AgentExecutorKind> {
    let kind = AgentExecutorKind::from_value(value)?;
    if DEPRECATED_EXECUTORS.contains(&kind) {
        Some(AgentExecutorKind::LocalRunner)
    } else {
        Some(kind)
    }
}

/// Instance default for newly-created projects (`agent_execution.py:48-55`):
/// always `local_runner`. `DEFAULT_AGENT_EXECUTOR` is ignored (any other
/// value is reported by the `managed.W001` system check, not refused).
pub fn get_default_agent_executor() -> AgentExecutorKind {
    AgentExecutorKind::LocalRunner
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-01-types.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn values_labels_and_machine_set_match_fixture() {
        let golden = &fixture()["agent_executor_kind"];
        assert_eq!(
            golden["values"],
            serde_json::json!(["local_runner", "cloud_agent", "managed_runner"])
        );
        let kinds = [
            AgentExecutorKind::LocalRunner,
            AgentExecutorKind::CloudAgent,
            AgentExecutorKind::ManagedRunner,
        ];
        let values: Vec<&str> = kinds.iter().map(AgentExecutorKind::value).collect();
        assert_eq!(serde_json::json!(values), golden["values"]);
        for kind in kinds {
            assert_eq!(
                golden["labels"][kind.value()],
                serde_json::Value::String(kind.label().to_string()),
                "label {}",
                kind.value()
            );
        }

        assert_eq!(
            golden["machine_executors"],
            serde_json::json!(["local_runner", "managed_runner"])
        );
        assert_eq!(
            MACHINE_EXECUTORS,
            [
                AgentExecutorKind::LocalRunner,
                AgentExecutorKind::ManagedRunner
            ]
        );
        assert!(AgentExecutorKind::LocalRunner.is_machine_executor());
        assert!(!AgentExecutorKind::CloudAgent.is_machine_executor());
        assert!(AgentExecutorKind::ManagedRunner.is_machine_executor());
    }

    #[test]
    fn serde_round_trips_values_and_rejects_unknown() {
        for kind in [
            AgentExecutorKind::LocalRunner,
            AgentExecutorKind::CloudAgent,
            AgentExecutorKind::ManagedRunner,
        ] {
            let rendered = serde_json::to_string(&kind).expect("serializes");
            assert_eq!(rendered, format!("\"{}\"", kind.value()));
            let back: AgentExecutorKind = serde_json::from_str(&rendered).expect("parses");
            assert_eq!(back, kind);
            assert_eq!(AgentExecutorKind::from_value(kind.value()), Some(kind));
        }
        assert_eq!(AgentExecutorKind::from_value("bogus"), None);
        assert!(serde_json::from_str::<AgentExecutorKind>("\"bogus\"").is_err());
        assert_eq!(AgentExecutorKind::CloudAgent.to_string(), "cloud_agent");
    }

    #[test]
    fn default_resolution_matches_fixture() {
        // #2903: the setting is ignored; the default is always local.
        assert_eq!(get_default_agent_executor().value(), "local_runner");
        assert_eq!(get_default_agent_executor(), AgentExecutorKind::LocalRunner);
    }

    #[test]
    fn normalize_executor_write_translates_deprecated() {
        // Deprecated choices become local_runner (deep-link fix case).
        assert_eq!(
            normalize_executor_write("cloud_agent"),
            Some(AgentExecutorKind::LocalRunner)
        );
        assert_eq!(
            normalize_executor_write("managed_runner"),
            Some(AgentExecutorKind::LocalRunner)
        );
        // Local passes through; unknown is the caller's 400.
        assert_eq!(
            normalize_executor_write("local_runner"),
            Some(AgentExecutorKind::LocalRunner)
        );
        assert_eq!(normalize_executor_write("bogus"), None);
        assert_eq!(
            DEPRECATED_EXECUTORS,
            [
                AgentExecutorKind::CloudAgent,
                AgentExecutorKind::ManagedRunner
            ]
        );
    }
}
