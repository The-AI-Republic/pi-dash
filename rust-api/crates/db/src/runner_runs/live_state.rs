#![forbid(unsafe_code)]

//! Runner live-state read model (D-15, stage 5).
//!
//! Ports the read subset of `RunnerLiveState`
//! (`apps/api/pi_dash/runner/models.py:1454-1527`): the six
//! FX-RUN-03 columns plus the `runner_id` primary key that
//! attributes the read. The remaining descriptive columns
//! (`last_event_kind`, `last_event_summary`, `agent_pid`,
//! `agent_subprocess_alive`, `turn_count`) and the write path belong
//! to D-14's session service, which extends this module later. The
//! token properties (`:1517-1527`) reuse L1
//! [`pidash_types::runner_runs::flat_token_fields`].
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `livestate_read_subset`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:1503`).
pub const TABLE: &str = "runner_live_state";

/// Read-subset columns in fixture order
/// (`livestate_read_subset.columns`). `runner_id` (the `OneToOne`
/// primary key, `models.py:1471-1476`) is not among them — it
/// attributes the read rather than describing the snapshot — but
/// every read still selects it (see [`RunnerLiveState`]).
pub const READ_COLUMNS: &[&str] = &[
    "observed_run_id",
    "usage",
    "llm_model",
    "last_event_at",
    "approvals_pending",
    "updated_at",
];

/// `llm_model` bound (`models.py:1498`, `max_length=128`).
pub const LLM_MODEL_MAX_LENGTH: usize = 128;

/// Fresh `usage` default (`models.py:1497`, `default=dict`).
pub fn default_usage() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `runner` one-to-one primary key: `CASCADE`
/// (`models.py:1471-1476`).
pub const RUNNER_ON_DELETE: OnDelete = OnDelete::Cascade;

/// Watchdog lookup index (`models.py:1508-1511`): the
/// `reconcile_stalled_runs` filter on run-id match, snapshot
/// freshness and agent-activity staleness.
pub const WATCHDOG_INDEX: &str = "runner_live_watchdog_idx";
/// Fields of [`WATCHDOG_INDEX`], Django field names.
pub const WATCHDOG_INDEX_FIELDS: &[&str] = &["observed_run_id", "updated_at", "last_event_at"];

/// One live-state snapshot: the `runner_id` primary key plus the
/// six read-subset columns. `NULL` is the canonical "unknown"
/// sentinel for every nullable field (`models.py:1460-1462`).
/// `approvals_pending` is a `PositiveIntegerField`
/// (`models.py:1493`), hence `i32`; `llm_model` is nullable
/// (`models.py:1498`), unlike `AgentRun.llm_model`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunnerLiveState {
    pub runner_id: uuid::Uuid,
    pub observed_run_id: Option<uuid::Uuid>,
    pub usage: serde_json::Value,
    pub llm_model: Option<String>,
    pub last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    pub approvals_pending: Option<i32>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl RunnerLiveState {
    /// `input_tokens` property (`models.py:1517-1519`).
    pub fn input_tokens(&self) -> Option<i64> {
        pidash_types::runner_runs::flat_token_fields(&self.usage).input_tokens
    }

    /// `output_tokens` property (`models.py:1521-1523`).
    pub fn output_tokens(&self) -> Option<i64> {
        pidash_types::runner_runs::flat_token_fields(&self.usage).output_tokens
    }

    /// `total_tokens` property (`models.py:1525-1527`).
    pub fn total_tokens(&self) -> Option<i64> {
        pidash_types::runner_runs::flat_token_fields(&self.usage).total_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn livestate_fixture() -> serde_json::Value {
        ts::fx03()["livestate_read_subset"].clone()
    }

    fn sample() -> RunnerLiveState {
        RunnerLiveState {
            runner_id: uuid::Uuid::nil(),
            observed_run_id: None,
            usage: serde_json::json!({"input": 5, "output": "7", "total": null}),
            llm_model: None,
            last_event_at: None,
            approvals_pending: None,
            updated_at: chrono::DateTime::<chrono::Utc>::MIN_UTC,
        }
    }

    #[test]
    fn read_columns_match_fixture_in_order() {
        let fixture_columns: Vec<String> = livestate_fixture()["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(|c| c["column"].as_str().expect("column").to_string())
            .collect();
        assert_eq!(ts::owned(READ_COLUMNS), fixture_columns);
    }

    #[test]
    fn meta_matches_fixture() {
        let v = livestate_fixture();
        assert_eq!(v["model"].as_str(), Some("RunnerLiveState"));
        assert_eq!(v["db_table"].as_str(), Some(TABLE));
        let index = WATCHDOG_INDEX;
        assert_eq!(index, "runner_live_watchdog_idx");
        assert_eq!(
            WATCHDOG_INDEX_FIELDS,
            &["observed_run_id", "updated_at", "last_event_at"]
        );
        assert_eq!(RUNNER_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn types_and_defaults_match_fixture() {
        let v = livestate_fixture();
        let column = |name: &str| {
            v["columns"]
                .as_array()
                .expect("columns")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name} in fixture"))
                .clone()
        };
        let observed = column("observed_run_id");
        assert_eq!(observed["type"].as_str(), Some("UUIDField"));
        assert_eq!(observed["db_type"].as_str(), Some("uuid"));
        assert_eq!(observed["null"].as_bool(), Some(true));
        let usage = column("usage");
        assert_eq!(usage["db_type"].as_str(), Some("jsonb"));
        assert_eq!(usage["null"].as_bool(), Some(false));
        assert_eq!(usage["default"].as_str(), Some("callable:<dict>"));
        assert!(default_usage().is_object());
        let model = column("llm_model");
        assert_eq!(model["db_type"].as_str(), Some("varchar(128)"));
        assert_eq!(model["max_length"].as_u64(), Some(128));
        assert_eq!(model["null"].as_bool(), Some(true));
        assert_eq!(LLM_MODEL_MAX_LENGTH, 128);
        let last_event = column("last_event_at");
        assert_eq!(
            last_event["db_type"].as_str(),
            Some("timestamp with time zone")
        );
        assert_eq!(last_event["null"].as_bool(), Some(true));
        let pending = column("approvals_pending");
        assert_eq!(pending["type"].as_str(), Some("PositiveIntegerField"));
        assert_eq!(pending["db_type"].as_str(), Some("integer"));
        assert_eq!(pending["null"].as_bool(), Some(true));
        let updated = column("updated_at");
        assert_eq!(updated["auto_now"].as_bool(), Some(true));
    }

    #[test]
    fn token_properties_delegate_to_flat_token_fields() {
        let v = livestate_fixture();
        for (property, key) in [
            ("input_tokens", "input"),
            ("output_tokens", "output"),
            ("total_tokens", "total"),
        ] {
            assert!(
                v["token_properties"][property]
                    .as_str()
                    .expect("pin")
                    .contains(&format!("flat_token_fields(self.usage)['{property}']")),
                "{property} pins the L1 delegation ({key})"
            );
        }
        let row = sample();
        let flat = pidash_types::runner_runs::flat_token_fields(&row.usage);
        assert_eq!(row.input_tokens(), flat.input_tokens);
        assert_eq!(row.output_tokens(), flat.output_tokens);
        assert_eq!(row.total_tokens(), flat.total_tokens);
        assert_eq!(row.input_tokens(), Some(5));
        assert_eq!(row.output_tokens(), Some(7));
        assert_eq!(row.total_tokens(), None);
    }
}
