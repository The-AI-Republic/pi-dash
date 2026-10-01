#![forbid(unsafe_code)]

//! Agent run event read shape for dispatch (D-11, stage 5).
//!
//! Ports the full `AgentRunEvent` column list
//! (`apps/api/pi_dash/runner/models.py:1162-1176`): the fixture's
//! `dispatch_touched_subset.agent_run_event` covers all six columns,
//! in declaration order. The FK entry uses the Django attname
//! (`agent_run_id`), like the D-20 `v1_cycles_modules::cycle`
//! port. `payload`'s `default=dict` is Django-side (no
//! `column_default` on the live table).
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json`
//! (`dispatch_touched_subset.agent_run_event`,
//! `agent_run_event_columns`, `agent_run_event_meta`).
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
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-02-models.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    /// Copy a const into an owned vec so assertions compare two
    /// runtime values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture subset entries mapped to physical columns: the fixture
    /// records Django field names while [`COLUMNS`] uses the attnames
    /// Django actually stores (`agent_run_id` for the FK).
    fn subset_columns(v: &Value) -> Vec<String> {
        v["dispatch_touched_subset"]["agent_run_event"]
            .as_array()
            .expect("subset is an array")
            .iter()
            .filter_map(|e| e.as_str())
            .map(|name| {
                if name == "agent_run" {
                    "agent_run_id".to_string()
                } else {
                    name.to_string()
                }
            })
            .collect()
    }

    #[test]
    fn columns_match_fixture_subset() {
        assert_eq!(owned(COLUMNS), subset_columns(&fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let meta = &fixture()["agent_run_event_meta"];
        assert_eq!(TABLE, meta["db_table"].as_str().expect("db_table"));
        assert_eq!(
            serde_json::json!(ORDERING),
            meta["ordering"],
            "Meta.ordering"
        );
        assert_eq!(
            serde_json::json!(UNIQUE_TOGETHER),
            meta["unique_together"],
            "Meta.unique_together"
        );
    }

    #[test]
    fn defaults_and_rules_match_fixture_column_entries() {
        let columns = &fixture()["agent_run_event_columns"];
        let entry = |name: &str| {
            columns
                .as_array()
                .expect("columns is an array")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name}"))
        };
        assert!(entry("payload")["default"]
            .as_str()
            .expect("payload default")
            .contains("dict"));
        assert_eq!(default_payload(), serde_json::json!({}));
        assert_eq!(
            entry("kind")["max_length"].as_u64().expect("max_length") as usize,
            KIND_MAX_LENGTH
        );
        assert!(entry("agent_run")["on_delete"]
            .as_str()
            .expect("on_delete")
            .contains("CASCADE"));
        assert_eq!(AGENT_RUN_ON_DELETE, OnDelete::Cascade);
        assert_eq!(entry("id")["type"], serde_json::json!("BigAutoField"));
        assert_eq!(
            entry("seq")["type"],
            serde_json::json!("PositiveIntegerField")
        );
    }
}
