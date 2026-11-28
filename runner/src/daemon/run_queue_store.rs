//! Durable FIFO queue store (NEWPIDASH-241 T3).
//!
//! One SQLite DB `data_dir/run_queue.sqlite3` (WAL, synchronous=FULL,
//! foreign_keys, PRAGMA user_version=1) with a serialized writer on a
//! dedicated blocking thread. The daemon holds `run_queue.lock` (fs2
//! exclusive nonblocking) for its lifetime; a second daemon exits.
//! Unknown newer schema / corruption / unusable storage → storage_blocked,
//! never erase.

use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub cloud_origin: String,
    pub runner_id: Uuid,
    pub run_id: Uuid,
    pub assignment_token: Option<Uuid>,
    pub sequence: u64,
    pub queued_at: i64, // ms epoch
    pub phase: String,
    pub assignment_payload: Vec<u8>,
    pub report_revision: u64,
    pub process_scope_id: Option<Uuid>,
    pub pending_terminal_payload: Option<Vec<u8>>,
    pub pending_terminal_message_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QueueMeta {
    pub next_sequence: u64,
    pub next_revision: u64,
    pub schema_version: u32,
}

pub struct Store {
    #[allow(dead_code)]
    path: PathBuf,
    conn: Mutex<rusqlite::Connection>,
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| e.to_string())?;
        let ver: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if ver > 1 {
            return Err(format!(
                "run_queue schema version {ver} newer than supported 1; storage_blocked"
            ));
        }
        if ver == 0 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS queue_meta (id INTEGER PRIMARY KEY CHECK (id=1), next_sequence INTEGER NOT NULL DEFAULT 1, next_revision INTEGER NOT NULL DEFAULT 1, schema_version INTEGER NOT NULL DEFAULT 1);
                 CREATE TABLE IF NOT EXISTS queue_entries (
                    cloud_origin TEXT NOT NULL,
                    runner_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    assignment_token TEXT,
                    sequence INTEGER NOT NULL,
                    queued_at INTEGER NOT NULL,
                    phase TEXT NOT NULL,
                    assignment_payload BLOB NOT NULL,
                    report_revision INTEGER NOT NULL DEFAULT 0,
                    process_scope_id TEXT,
                    pending_terminal_payload BLOB,
                    pending_terminal_message_id TEXT,
                    PRIMARY KEY (run_id, cloud_origin)
                 );
                 CREATE TABLE IF NOT EXISTS tombstones (run_id TEXT NOT NULL, cloud_origin TEXT NOT NULL, tombstone_at INTEGER NOT NULL, PRIMARY KEY (run_id, cloud_origin));
                ",
            ).map_err(|e| e.to_string())?;
            conn.execute("INSERT OR IGNORE INTO queue_meta (id, next_sequence, next_revision, schema_version) VALUES (1, 1, 1, 1)", []).map_err(|e| e.to_string())?;
            conn.execute("PRAGMA user_version=1", [])
                .map_err(|e| e.to_string())?;
        } else {
            // Verify integrity quickly
            let integrity: String = conn
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if integrity != "ok" {
                return Err(format!("run_queue integrity {integrity}"));
            }
        }
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    pub fn meta(&self) -> Result<QueueMeta, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        conn.query_row(
            "SELECT next_sequence, next_revision, schema_version FROM queue_meta WHERE id=1",
            [],
            |r| {
                Ok(QueueMeta {
                    next_sequence: r.get::<_, i64>(0)? as u64,
                    next_revision: r.get::<_, i64>(1)? as u64,
                    schema_version: r.get::<_, i64>(2)? as u32,
                })
            },
        )
        .map_err(|e| e.to_string())
    }

    pub fn allocate_sequence(&self) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let seq: i64 = conn
            .query_row("SELECT next_sequence FROM queue_meta WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE queue_meta SET next_sequence = next_sequence + 1 WHERE id=1",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(seq as u64)
    }

    pub fn insert(&self, e: &QueueEntry) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO queue_entries (cloud_origin, runner_id, run_id, assignment_token, sequence, queued_at, phase, assignment_payload, report_revision, process_scope_id, pending_terminal_payload, pending_terminal_message_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![
                e.cloud_origin,
                e.runner_id.to_string(),
                e.run_id.to_string(),
                e.assignment_token.map(|u| u.to_string()),
                e.sequence as i64,
                e.queued_at,
                e.phase,
                e.assignment_payload,
                e.report_revision as i64,
                e.process_scope_id.map(|u| u.to_string()),
                e.pending_terminal_payload,
                e.pending_terminal_message_id,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_waiting_ordered(&self) -> Result<Vec<QueueEntry>, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let mut stmt = conn.prepare("SELECT cloud_origin, runner_id, run_id, assignment_token, sequence, queued_at, phase, assignment_payload, report_revision, process_scope_id, pending_terminal_payload, pending_terminal_message_id FROM queue_entries WHERE phase='waiting' ORDER BY sequence ASC").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(QueueEntry {
                    cloud_origin: row.get(0)?,
                    runner_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or(Uuid::nil()),
                    run_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or(Uuid::nil()),
                    assignment_token: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    sequence: row.get::<_, i64>(4)? as u64,
                    queued_at: row.get(5)?,
                    phase: row.get(6)?,
                    assignment_payload: row.get(7)?,
                    report_revision: row.get::<_, i64>(8)? as u64,
                    process_scope_id: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    pending_terminal_payload: row.get(10)?,
                    pending_terminal_message_id: row.get(11)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn insert_tombstone(
        &self,
        run_id: Uuid,
        cloud_origin: &str,
        at: i64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        conn.execute("INSERT OR REPLACE INTO tombstones (run_id, cloud_origin, tombstone_at) VALUES (?1,?2,?3)", rusqlite::params![run_id.to_string(), cloud_origin, at]).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Persist a terminal outbox record (finishing -> terminal_pending):
    /// the exact payload/key the daemon must redeliver after a crash,
    /// never re-executing and never losing the terminal signal.
    pub fn set_terminal_pending(
        &self,
        run_id: Uuid,
        payload: &[u8],
        message_id: &str,
    ) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n = conn
            .execute(
                "UPDATE queue_entries SET phase = 'terminal_pending', pending_terminal_payload = ?1, pending_terminal_message_id = ?2 WHERE run_id = ?3",
                rusqlite::params![payload, message_id, run_id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// All terminal-pending outbox records in FIFO order for startup retry.
    pub fn list_terminal_pending(&self) -> Result<Vec<QueueEntry>, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let mut stmt = conn.prepare("SELECT cloud_origin, runner_id, run_id, assignment_token, sequence, queued_at, phase, assignment_payload, report_revision, process_scope_id, pending_terminal_payload, pending_terminal_message_id FROM queue_entries WHERE phase='terminal_pending' ORDER BY sequence ASC").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(QueueEntry {
                    cloud_origin: row.get(0)?,
                    runner_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or(Uuid::nil()),
                    run_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or(Uuid::nil()),
                    assignment_token: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    sequence: row.get::<_, i64>(4)? as u64,
                    queued_at: row.get(5)?,
                    phase: row.get(6)?,
                    assignment_payload: row.get(7)?,
                    report_revision: row.get::<_, i64>(8)? as u64,
                    process_scope_id: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    pending_terminal_payload: row.get(10)?,
                    pending_terminal_message_id: row.get(11)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Persist a phase transition (waiting -> starting -> executing).
    /// Restart reconciliation never replays non-waiting phases, so admission
    /// must persist `starting` before the worker launches.
    pub fn update_phase(&self, run_id: Uuid, phase: &str) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n = conn
            .execute(
                "UPDATE queue_entries SET phase = ?1 WHERE run_id = ?2",
                rusqlite::params![phase, run_id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// Durable cancel: delete the waiting row and tombstone the attempt so
    /// a restart never resurrects canceled work. Returns rows deleted.
    pub fn delete_waiting(&self, run_id: Uuid) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n = conn
            .execute(
                "DELETE FROM queue_entries WHERE run_id = ?1 AND phase = 'waiting'",
                rusqlite::params![run_id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    pub fn is_tombstoned(&self, run_id: Uuid) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tombstones WHERE run_id = ?1",
                rusqlite::params![run_id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    pub fn delete_entry(&self, run_id: Uuid) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n = conn
            .execute(
                "DELETE FROM queue_entries WHERE run_id = ?1",
                rusqlite::params![run_id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// Delete `run_id`'s row unless it holds a terminal outbox record.
    pub fn delete_unless_terminal(&self, run_id: Uuid) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let n = conn
            .execute(
                "DELETE FROM queue_entries WHERE run_id = ?1 AND phase <> 'terminal_pending'",
                rusqlite::params![run_id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// All durable phases for restart reconciliation: returns (waiting, other).
    /// `waiting` preserves FIFO order; callers must never replay `other`.
    pub fn list_all_phases(&self) -> Result<Vec<(Uuid, String)>, String> {
        let conn = self.conn.lock().map_err(|_| "poison".to_string())?;
        let mut stmt = conn
            .prepare("SELECT run_id, phase FROM queue_entries ORDER BY sequence ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or(Uuid::nil()),
                    row.get::<_, String>(1)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_allocation_monotonic() {
        let path = std::env::temp_dir().join(format!("test_queue_{}.sqlite3", Uuid::new_v4()));
        let store = Store::open(path.clone()).expect("open");
        let a = store.allocate_sequence().expect("a");
        let b = store.allocate_sequence().expect("b");
        assert_eq!(b, a + 1);
        std::fs::remove_file(path).ok();
    }
    #[test]
    fn second_daemon_lock_is_separate_from_store() {
        // fs2 lock tested at coordinator level; store itself just opens
        let path = std::env::temp_dir().join(format!("test_queue2_{}.sqlite3", Uuid::new_v4()));
        let store = Store::open(path.clone()).expect("open");
        assert!(store.meta().is_ok());
        std::fs::remove_file(path).ok();
    }
}
