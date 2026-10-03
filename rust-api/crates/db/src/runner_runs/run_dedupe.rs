#![forbid(unsafe_code)]

//! Run-message dedupe model (D-15, stage 5).
//!
//! Ports `RunMessageDedupe`
//! (`apps/api/pi_dash/runner/models.py:788-808`): row struct +
//! column list. The `id` column is Django's implicit `BigAutoField`.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02 `RunMessageDedupe`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:801`).
pub const TABLE: &str = "run_message_dedupe";

/// Columns in declaration order (`models.py:796-798` plus the
/// implicit `id`), FK entry as the Django attname.
pub const COLUMNS: &[&str] = &["id", "run_id", "message_id", "created_at"];

/// `message_id` bound (`models.py:797`, `max_length=128`).
pub const MESSAGE_ID_MAX_LENGTH: usize = 128;

/// `run` FK: `CASCADE` (`models.py:796`).
pub const RUN_ON_DELETE: OnDelete = OnDelete::Cascade;

/// `(run, message_id)` idempotency key (`models.py:802-807`).
pub const DEDUPE_UNIQUE: &str = "run_message_dedupe_unique";
/// Fields of [`DEDUPE_UNIQUE`], Django field names.
pub const DEDUPE_UNIQUE_FIELDS: &[&str] = &["run", "message_id"];

/// Dedupe-sweep scan index (`models.py:808`).
pub const CREATED_INDEX: &str = "run_message_created_2bae73_idx";
/// Fields of [`CREATED_INDEX`], Django field names.
pub const CREATED_INDEX_FIELDS: &[&str] = &["created_at"];

/// One idempotency record, declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMessageDedupe {
    pub id: i64,
    pub run_id: uuid::Uuid,
    pub message_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn dedupe_fixture() -> serde_json::Value {
        ts::model(&ts::fx02(), "RunMessageDedupe").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&dedupe_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = dedupe_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert!(m["ordering"].as_array().expect("o").is_empty());
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let uniq = ts::constraint(&m, DEDUPE_UNIQUE);
        assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(uniq["fields"], serde_json::json!(DEDUPE_UNIQUE_FIELDS));
        let index = ts::index(&m, CREATED_INDEX);
        assert_eq!(index["fields"], serde_json::json!(CREATED_INDEX_FIELDS));
    }

    #[test]
    fn types_and_relations_match_fixture() {
        let m = dedupe_fixture();
        let id = ts::field(&m, "id");
        assert_eq!(id["type"].as_str(), Some("BigAutoField"));
        assert_eq!(id["db_type"].as_str(), Some("bigint"));
        let run = ts::field(&m, "run");
        assert_eq!(run["type"].as_str(), Some("ForeignKey"));
        assert_eq!(run["null"].as_bool(), Some(false));
        assert_eq!(run["rel"]["to"].as_str(), Some("agent_run"));
        assert_eq!(run["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(RUN_ON_DELETE, OnDelete::Cascade);
        let message_id = ts::field(&m, "message_id");
        assert_eq!(message_id["db_type"].as_str(), Some("varchar(128)"));
        assert_eq!(message_id["max_length"].as_u64(), Some(128));
        assert_eq!(MESSAGE_ID_MAX_LENGTH, 128);
        assert_eq!(
            ts::field(&m, "created_at")["auto_now_add"].as_bool(),
            Some(true)
        );
    }
}
