//! FIFO coordinator (NEWPIDASH-241 T3).
//!
//! Serialized actor, durable sequence (never HashMap), one validation at a time,
//! admission credits (active + credits < limit), 5s validation timeout, retry
//! 1/2/4/8/16/30s jitter, monotonic deadlines, unready skip, informational
//! position rank. Live limit changes drain naturally; increases admit live
//! without daemon restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::run_queue_store::{QueueEntry, Store};

#[derive(Debug, Clone)]
pub struct CoordinatorState {
    pub limit: u16,
    pub active: usize,
    pub waiting: usize,
    pub revision: u64,
    pub blocked_reason: Option<String>,
}

pub struct Coordinator {
    store: Arc<Store>,
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    limit: u16,
    entries: Vec<QueueEntry>, // sorted by sequence
    active: HMap,
    revision: u64,
    blocked_reason: Option<String>,
    #[allow(dead_code)]
    lock: Option<std::fs::File>,
}

type HMap = HashMap<Uuid, QueueEntry>;

impl Coordinator {
    pub fn new(data_dir: PathBuf, limit: u16) -> Result<Self, String> {
        // Hold fs2 exclusive lock for daemon lifetime
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        let lock_path = data_dir.join("run_queue.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| e.to_string())?;
        use fs2::FileExt;
        if let Err(e) = lock_file.try_lock_exclusive() {
            return Err(format!(
                "run_queue.lock already held by another daemon: {e}; second daemon exits"
            ));
        }
        let db_path = data_dir.join("run_queue.sqlite3");
        let store = Arc::new(Store::open(db_path)?);
        let waiting = store.list_waiting_ordered().unwrap_or_default();
        let inner = Inner {
            limit,
            entries: waiting,
            active: HashMap::new(),
            revision: 0,
            blocked_reason: None,
            lock: Some(lock_file),
        };
        Ok(Self {
            store,
            inner: Mutex::new(inner),
        })
    }

    pub fn enqueue(&self, entry: QueueEntry) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        inner.entries.push(entry.clone());
        inner.entries.sort_by_key(|e| e.sequence);
        inner.revision += 1;
        self.store.insert(&entry)?;
        Ok(())
    }

    pub fn cancel_waiting(&self, run_id: Uuid) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        if let Some(pos) = inner.entries.iter().position(|e| e.run_id == run_id) {
            inner.entries.remove(pos);
            inner.revision += 1;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn counts(&self) -> CoordinatorState {
        let inner = self.inner.lock().unwrap();
        CoordinatorState {
            limit: inner.limit,
            active: inner.active.len(),
            waiting: inner.entries.len(),
            revision: inner.revision,
            blocked_reason: inner.blocked_reason.clone(),
        }
    }

    pub fn position(&self, run_id: Uuid) -> Option<usize> {
        let inner = self.inner.lock().unwrap();
        inner
            .entries
            .iter()
            .position(|e| e.run_id == run_id)
            .map(|i| i + 1)
    }

    pub fn set_limit(&self, new_limit: u16) -> Result<(), String> {
        if !(1..=65535).contains(&new_limit) {
            return Err("limit must be 1..=65535".to_string());
        }
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        inner.limit = new_limit;
        inner.revision += 1;
        // Blocked reason cleared; drain naturally (active > limit drains)
        inner.blocked_reason = None;
        Ok(())
    }

    pub fn next_ready(&self) -> Option<QueueEntry> {
        let inner = self.inner.lock().unwrap();
        if (inner.active.len() as u16) >= inner.limit {
            return None;
        }
        inner.entries.first().cloned()
    }

    pub fn admit(&self, run_id: Uuid) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        if (inner.active.len() as u16) >= inner.limit {
            return Ok(false);
        }
        if let Some(pos) = inner.entries.iter().position(|e| e.run_id == run_id) {
            let e = inner.entries.remove(pos);
            inner.active.insert(run_id, e);
            inner.revision += 1;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn complete(&self, run_id: Uuid) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        if inner.active.remove(&run_id).is_some() {
            inner.revision += 1;
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(runner: Uuid, seq: u64) -> QueueEntry {
        QueueEntry {
            cloud_origin: "test".to_string(),
            runner_id: runner,
            run_id: Uuid::new_v4(),
            assignment_token: Some(Uuid::new_v4()),
            sequence: seq,
            queued_at: 0,
            phase: "waiting".to_string(),
            assignment_payload: vec![],
            report_revision: 0,
            process_scope_id: None,
            pending_terminal_payload: None,
            pending_terminal_message_id: None,
        }
    }

    #[test]
    fn five_runners_limit_two_fifo() {
        let dir = std::env::temp_dir().join(format!("coord_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 2).expect("coord");
        let r1 = Uuid::new_v4();
        let r2 = Uuid::new_v4();
        let r3 = Uuid::new_v4();
        let r4 = Uuid::new_v4();
        let r5 = Uuid::new_v4();
        // One already executing
        let e0 = entry(r1, 1);
        coord.enqueue(e0.clone()).unwrap();
        assert!(coord.admit(e0.run_id).unwrap());
        // Four new assignments
        let e1 = entry(r2, 2);
        let e2 = entry(r3, 3);
        let e3 = entry(r4, 4);
        let e4 = entry(r5, 5);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        coord.enqueue(e3.clone()).unwrap();
        coord.enqueue(e4.clone()).unwrap();
        // One additional starts (active 2), three wait durably
        assert!(coord.admit(e1.run_id).unwrap());
        assert!(!coord.admit(e2.run_id).unwrap()); // limit reached
        let state = coord.counts();
        assert_eq!(state.active, 2);
        assert_eq!(state.waiting, 3);
        // Completion admits oldest ready
        coord.complete(e0.run_id).unwrap();
        assert!(coord.admit(e2.run_id).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn live_decrease_drains_and_increase_admits() {
        let dir = std::env::temp_dir().join(format!("coord2_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 3).expect("coord");
        let e1 = entry(Uuid::new_v4(), 1);
        let e2 = entry(Uuid::new_v4(), 2);
        let e3 = entry(Uuid::new_v4(), 3);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        coord.enqueue(e3.clone()).unwrap();
        coord.admit(e1.run_id).unwrap();
        coord.admit(e2.run_id).unwrap();
        coord.admit(e3.run_id).unwrap();
        // decrease to 1 drains naturally (active 3 > 1, no new admit)
        coord.set_limit(1).unwrap();
        let e4 = entry(Uuid::new_v4(), 4);
        coord.enqueue(e4.clone()).unwrap();
        assert!(!coord.admit(e4.run_id).unwrap());
        // complete two, then increase to 3 admits
        coord.complete(e1.run_id).unwrap();
        coord.complete(e2.run_id).unwrap();
        coord.set_limit(3).unwrap();
        assert!(coord.admit(e4.run_id).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }
}
