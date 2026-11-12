#![forbid(unsafe_code)]

//! Chat approval-request model (D-15, stage 5).
//!
//! Ports `AgentChatApprovalRequest`
//! (`apps/api/pi_dash/runner/models.py:1391-1430`): row struct +
//! column list. `kind` / `status` reuse the L1
//! [`pidash_types::runner_runs::ApprovalKind`] /
//! [`pidash_types::runner_runs::ApprovalStatus`].
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `AgentChatApprovalRequest`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;
use pidash_types::runner_runs::{ApprovalKind, ApprovalStatus};

/// Physical table (`Meta.db_table`, `models.py:1417`).
pub const TABLE: &str = "agent_chat_approval";
/// Default ordering (`Meta.ordering`, `models.py:1418`).
pub const ORDERING: &[&str] = &["-requested_at"];

/// Columns in declaration order (`models.py:1392-1414`), FK entries
/// as the Django attnames.
pub const COLUMNS: &[&str] = &[
    "id",
    "session_id",
    "local_approval_id",
    "kind",
    "payload",
    "reason",
    "status",
    "decision_source",
    "decided_by_id",
    "requested_at",
    "expires_at",
    "decided_at",
];

/// `local_approval_id` bound (`models.py:1394`, `max_length=160`).
pub const LOCAL_APPROVAL_ID_MAX_LENGTH: usize = 160;
/// `kind` bound (`models.py:1395`, `max_length=24`).
pub const KIND_MAX_LENGTH: usize = 24;
/// `status` bound (`models.py:1398-1403`, `max_length=16`).
pub const STATUS_MAX_LENGTH: usize = 16;
/// `decision_source` bound (`models.py:1404`, `max_length=16`).
pub const DECISION_SOURCE_MAX_LENGTH: usize = 16;

/// `status` Django-side default (`models.py:1398-1403`,
/// `default=ApprovalStatus.PENDING`).
pub const DEFAULT_STATUS: ApprovalStatus = ApprovalStatus::Pending;
/// Shared `default=""` for `reason` (`models.py:1397`) and
/// `decision_source` (`models.py:1404`).
pub const EMPTY_TEXT: &str = "";

/// Fresh `payload` default (`models.py:1396`, `default=dict`).
pub fn default_payload() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `session` FK: `CASCADE` (`models.py:1393`).
pub const SESSION_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `decided_by` FK: `SET_NULL`, nullable (`models.py:1405-1411`).
pub const DECIDED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Local-approval identity per session (`models.py:1419-1424`).
pub const LOCAL_UNIQUE: &str = "ac_appr_local_uniq";
/// Fields of [`LOCAL_UNIQUE`], Django field names.
pub const LOCAL_UNIQUE_FIELDS: &[&str] = &["session", "local_approval_id"];

/// Session + status lookup index (`models.py:1425-1430`).
pub const SESSION_STATUS_INDEX: &str = "ac_appr_sess_stat_idx";
/// Fields of [`SESSION_STATUS_INDEX`], Django field names.
pub const SESSION_STATUS_INDEX_FIELDS: &[&str] = &["session", "status"];

/// One chat approval-request row, declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatApprovalRequest {
    pub id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub local_approval_id: String,
    pub kind: ApprovalKind,
    pub payload: serde_json::Value,
    pub reason: String,
    pub status: ApprovalStatus,
    pub decision_source: String,
    pub decided_by_id: Option<uuid::Uuid>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn approval_fixture() -> serde_json::Value {
        ts::model(&ts::fx03(), "AgentChatApprovalRequest").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&approval_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = approval_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let uniq = ts::constraint(&m, LOCAL_UNIQUE);
        assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(uniq["fields"], serde_json::json!(LOCAL_UNIQUE_FIELDS));
        let index = ts::index(&m, SESSION_STATUS_INDEX);
        assert_eq!(
            index["fields"],
            serde_json::json!(SESSION_STATUS_INDEX_FIELDS)
        );
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = approval_fixture();
        assert_eq!(
            ts::field(&m, "id")["default"].as_str(),
            Some("callable:<uuid4>")
        );
        let session = ts::field(&m, "session");
        assert_eq!(session["type"].as_str(), Some("ForeignKey"));
        assert_eq!(session["rel"]["to"].as_str(), Some("agent_chat_session"));
        assert_eq!(session["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(SESSION_ON_DELETE, OnDelete::Cascade);
        let decided_by = ts::field(&m, "decided_by");
        assert_eq!(decided_by["null"].as_bool(), Some(true));
        assert_eq!(decided_by["rel"]["to"].as_str(), Some("users"));
        assert_eq!(decided_by["rel"]["on_delete"].as_str(), Some("SET_NULL"));
        assert_eq!(DECIDED_BY_ON_DELETE, OnDelete::SetNull);
        for (field, bound) in [
            ("local_approval_id", LOCAL_APPROVAL_ID_MAX_LENGTH),
            ("kind", KIND_MAX_LENGTH),
            ("status", STATUS_MAX_LENGTH),
            ("decision_source", DECISION_SOURCE_MAX_LENGTH),
        ] {
            assert_eq!(
                ts::field(&m, field)["max_length"].as_u64(),
                Some(bound as u64),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "kind")["default"].as_str(),
            Some("NOT_PROVIDED"),
            "kind has no default: every row sets it"
        );
        assert_eq!(
            ts::field(&m, "status")["default"].as_str(),
            Some(DEFAULT_STATUS.value())
        );
        assert_eq!(ts::field(&m, "status")["db_index"].as_bool(), Some(true));
        for field in ["reason", "decision_source"] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some(EMPTY_TEXT),
                "{field}"
            );
        }
        assert_eq!(ts::field(&m, "reason")["db_type"].as_str(), Some("text"));
        assert_eq!(
            ts::field(&m, "payload")["default"].as_str(),
            Some("callable:<dict>")
        );
        assert!(default_payload().is_object());
        assert_eq!(
            ts::field(&m, "requested_at")["auto_now_add"].as_bool(),
            Some(true)
        );
        assert_eq!(ts::field(&m, "expires_at")["null"].as_bool(), Some(true));
        assert_eq!(ts::field(&m, "decided_at")["null"].as_bool(), Some(true));
    }
}
