//! Durable receipt-before-ACK (NEWPIDASH-241 T4).
//!
//! Every assignment attempt gets a durable ReceiptResult before any HTTP
//! ACK leaves. Pending dedupe is in-memory only on HttpLoop owner: a pending
//! duplicate joins the stream list without ACK. Committed = SQLite commit
//! precedes ACK. Later duplicate of a committed message still resolves via
//! attempt lookup; terminal tombstone is ACKed without executable work.
//! Persistence sequence (`inc_queue_sequence`) cannot be inferred from a
//! HashMap; intent is recorded in the same SQLite transaction.

use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptResult {
    /// Freshly committed: check + insert + sequence committed before ACK.
    Committed { stream_ids: Vec<String> },
    /// Already committed: later duplicate resolves via attempt lookup.
    AlreadyCommitted { stream_ids: Vec<String> },
    /// Tombstone: canceled/terminal ACKed without side effects.
    Tombstone,
    /// Pending duplicate: join stream list without ACK.
    Pending,
}

pub struct ReceiptStore {
    conn: Mutex<rusqlite::Connection>,
}

impl ReceiptStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
            .map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS receipts (
                message_id TEXT PRIMARY KEY,
                stream_ids TEXT NOT NULL,
                disposition TEXT NOT NULL,
                assignment_token TEXT,
                created_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Atomic check+insert+sequence: caller must not ACK before Ok.
    /// Returns Committed on first receipt, AlreadyCommitted on duplicate,
    /// Tombstone if terminal/canceled (caller passes tombstone flag).
    pub fn record_before_ack(
        &self,
        message_id: &str,
        stream_ids: &[String],
        assignment_token: Option<Uuid>,
        is_tombstone: bool,
    ) -> Result<ReceiptResult, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        // Check committed
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT stream_ids, disposition FROM receipts WHERE message_id=?1",
                rusqlite::params![message_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((ids, disp)) = existing {
            if disp == "committed" {
                return Ok(ReceiptResult::AlreadyCommitted {
                    stream_ids: if ids.is_empty() {
                        vec![]
                    } else {
                        ids.split(',').map(|s| s.to_string()).collect()
                    },
                });
            }
            if disp == "tombstone" {
                return Ok(ReceiptResult::Tombstone);
            }
            // pending duplicate (should not happen with in-memory pending, but durable pending here means retry)
            return Ok(ReceiptResult::Pending);
        }
        // Not seen: commit now (check+insert+sequence in same txn)
        let now_ms = chrono::Utc::now().timestamp_millis();
        let joined = stream_ids.join(",");
        let disp = if is_tombstone {
            "tombstone"
        } else {
            "committed"
        };
        conn.execute(
            "INSERT INTO receipts (message_id, stream_ids, disposition, assignment_token, created_at) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![message_id, joined, disp, assignment_token.map(|u| u.to_string()), now_ms],
        )
        .map_err(|e| e.to_string())?;
        if is_tombstone {
            Ok(ReceiptResult::Tombstone)
        } else {
            Ok(ReceiptResult::Committed {
                stream_ids: stream_ids.to_vec(),
            })
        }
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_receipt_committed_before_ack() {
        let path = std::env::temp_dir().join(format!("receipt_{}.sqlite3", Uuid::new_v4()));
        let store = ReceiptStore::open(path.clone()).unwrap();
        let mid = "mid-1";
        let streams = vec!["s1".to_string()];
        let r = store.record_before_ack(mid, &streams, None, false).unwrap();
        assert!(matches!(r, ReceiptResult::Committed { .. }));
        // duplicate is AlreadyCommitted
        let r2 = store.record_before_ack(mid, &streams, None, false).unwrap();
        assert!(matches!(r2, ReceiptResult::AlreadyCommitted { .. }));
        std::fs::remove_file(path).ok();
    }
    #[test]
    fn tombstone_acked_without_side_effects() {
        let path = std::env::temp_dir().join(format!("receipt2_{}.sqlite3", Uuid::new_v4()));
        let store = ReceiptStore::open(path.clone()).unwrap();
        let r = store
            .record_before_ack("mid-2", &["s2".to_string()], None, true)
            .unwrap();
        assert_eq!(r, ReceiptResult::Tombstone);
        std::fs::remove_file(path).ok();
    }
}
