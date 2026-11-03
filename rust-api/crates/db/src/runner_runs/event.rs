#![forbid(unsafe_code)]

//! Agent run event model (D-15, stage 5).
//!
//! Ports `AgentRunEvent` (`apps/api/pi_dash/runner/models.py:1162-1176`):
//! row struct + column list. D-11's `db::dispatch::event` keeps its
//! own read shape of this table; this is the runner-domain full port.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02 `AgentRunEvent`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:1173`).
pub const TABLE: &str = "agent_run_event";
/// Default ordering (`Meta.ordering`, `models.py:1175`).
pub const ORDERING: &[&str] = &["agent_run", "seq"];
/// Append-only transcript identity (`Meta.unique_together`,
/// `models.py:1174`): the sequence number is unique per run.
pub const UNIQUE_TOGETHER: &[&[&str]] = &[&["agent_run", "seq"]];

/// Columns in declaration order (`models.py:1165-1170`), FK entry as
/// the Django attname.
pub const COLUMNS: &[&str] = &["id", "agent_run_id", "seq", "kind", "payload", "created_at"];

/// `kind` bound (`models.py:1168`, `max_length=64`).
pub const KIND_MAX_LENGTH: usize = 64;

/// Fresh `payload` default (`models.py:1169`, `default=dict`).
pub fn default_payload() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `agent_run` FK: `CASCADE` (`models.py:1166`).
pub const AGENT_RUN_ON_DELETE: OnDelete = OnDelete::Cascade;

/// One transcript event. `id` is a `BigAutoField`
/// (`models.py:1165`), hence `i64`; `seq` is a
/// `PositiveIntegerField` (`models.py:1167`), hence `i32` like the
/// D-02 `repository_count` port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunEvent {
    pub id: i64,
    pub agent_run_id: uuid::Uuid,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn event_fixture() -> serde_json::Value {
        ts::model(&ts::fx02(), "AgentRunEvent").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&event_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = event_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert_eq!(
            serde_json::json!(UNIQUE_TOGETHER),
            m["unique_together"],
            "Meta.unique_together"
        );
        assert!(m["constraints"].as_array().expect("c").is_empty());
        assert!(m["indexes"].as_array().expect("i").is_empty());
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = event_fixture();
        let id = ts::field(&m, "id");
        assert_eq!(id["type"].as_str(), Some("BigAutoField"));
        assert_eq!(id["db_type"].as_str(), Some("bigint"));
        let run = ts::field(&m, "agent_run");
        assert_eq!(run["type"].as_str(), Some("ForeignKey"));
        assert_eq!(run["null"].as_bool(), Some(false));
        assert_eq!(run["rel"]["to"].as_str(), Some("agent_run"));
        assert_eq!(run["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(AGENT_RUN_ON_DELETE, OnDelete::Cascade);
        let seq = ts::field(&m, "seq");
        assert_eq!(seq["type"].as_str(), Some("PositiveIntegerField"));
        assert_eq!(seq["db_type"].as_str(), Some("integer"));
        let kind = ts::field(&m, "kind");
        assert_eq!(kind["db_type"].as_str(), Some("varchar(64)"));
        assert_eq!(kind["max_length"].as_u64(), Some(64));
        assert_eq!(KIND_MAX_LENGTH, 64);
        let payload = ts::field(&m, "payload");
        assert_eq!(payload["db_type"].as_str(), Some("jsonb"));
        assert_eq!(payload["default"].as_str(), Some("callable:<dict>"));
        assert!(default_payload().is_object());
        let created_at = ts::field(&m, "created_at");
        assert_eq!(created_at["auto_now_add"].as_bool(), Some(true));
    }
}
