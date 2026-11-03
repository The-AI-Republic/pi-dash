#![forbid(unsafe_code)]

//! Chat-message dedupe model (D-15, stage 5).
//!
//! Ports `ChatMessageDedupe`
//! (`apps/api/pi_dash/runner/models.py:1433-1451`): row struct +
//! column list. The `id` column is Django's implicit `BigAutoField`.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `ChatMessageDedupe`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:1439`).
pub const TABLE: &str = "chat_message_dedupe";

/// Columns in declaration order (`models.py:1434-1436` plus the
/// implicit `id`), FK entry as the Django attname.
pub const COLUMNS: &[&str] = &["id", "session_id", "message_id", "created_at"];

/// `message_id` bound (`models.py:1435`, `max_length=128`).
pub const MESSAGE_ID_MAX_LENGTH: usize = 128;

/// `session` FK: `CASCADE` (`models.py:1434`).
pub const SESSION_ON_DELETE: OnDelete = OnDelete::Cascade;

/// `(session, message_id)` idempotency key (`models.py:1440-1445`).
pub const DEDUPE_UNIQUE: &str = "chat_dedupe_unique";
/// Fields of [`DEDUPE_UNIQUE`], Django field names.
pub const DEDUPE_UNIQUE_FIELDS: &[&str] = &["session", "message_id"];

/// Dedupe-sweep scan index (`models.py:1446-1451`).
pub const CREATED_INDEX: &str = "chat_dedupe_created_idx";
/// Fields of [`CREATED_INDEX`], Django field names.
pub const CREATED_INDEX_FIELDS: &[&str] = &["created_at"];

/// One idempotency record, declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessageDedupe {
    pub id: i64,
    pub session_id: uuid::Uuid,
    pub message_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn dedupe_fixture() -> serde_json::Value {
        ts::model(&ts::fx03(), "ChatMessageDedupe").clone()
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
        let session = ts::field(&m, "session");
        assert_eq!(session["type"].as_str(), Some("ForeignKey"));
        assert_eq!(session["null"].as_bool(), Some(false));
        assert_eq!(session["rel"]["to"].as_str(), Some("agent_chat_session"));
        assert_eq!(session["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(SESSION_ON_DELETE, OnDelete::Cascade);
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
