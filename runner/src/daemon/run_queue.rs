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
    pub held: usize,
    pub revision: u64,
    pub blocked_reason: Option<String>,
}

pub struct Coordinator {
    store: Arc<Store>,
    inner: Mutex<Inner>,
    /// Woken when capacity frees (complete / set_limit increase / cancel)
    /// so the oldest waiting worker can claim the slot.
    admission: Arc<tokio::sync::Notify>,
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
            admission: Arc::new(tokio::sync::Notify::new()),
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

    /// Enqueue a fresh assignment durably: allocate the durable sequence,
    /// persist the payload, then hold as waiting until admitted. Called by
    /// RunnerLoop the moment an Assign lands, before any workspace prep.
    pub fn enqueue_assign(
        &self,
        runner_id: Uuid,
        run_id: Uuid,
        assignment_token: Option<Uuid>,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        let sequence = self.store.allocate_sequence()?;
        let now = chrono::Utc::now().timestamp_millis();
        let entry = QueueEntry {
            cloud_origin: "cloud".to_string(),
            runner_id,
            run_id,
            assignment_token,
            sequence,
            queued_at: now,
            phase: "waiting".to_string(),
            assignment_payload: payload,
            report_revision: 0,
            process_scope_id: None,
            pending_terminal_payload: None,
            pending_terminal_message_id: None,
        };
        self.enqueue(entry)
    }

    /// All waiting entries in FIFO order (for restart reconciliation).
    pub fn waiting_entries(&self) -> Vec<QueueEntry> {
        self.inner
            .lock()
            .map(|g| g.entries.clone())
            .unwrap_or_default()
    }

    /// Oldest waiting entry for one runner (restart re-hydration).
    pub fn waiting_for(&self, runner_id: Uuid) -> Option<QueueEntry> {
        let inner = self.inner.lock().ok()?;
        inner
            .entries
            .iter()
            .find(|e| e.runner_id == runner_id)
            .cloned()
    }

    /// True when this runner already holds a nonterminal attempt
    /// (waiting or admitted) — a fresh Assign must NACK, not enqueue twice.
    pub fn runner_held(&self, runner_id: Uuid) -> bool {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        inner.entries.iter().any(|e| e.runner_id == runner_id)
            || inner.active.values().any(|e| e.runner_id == runner_id)
    }

    /// Attempt token for a held run (waiting or admitted), for poll.
    pub fn held_token(&self, run_id: Uuid) -> Option<Uuid> {
        let inner = self.inner.lock().ok()?;
        if let Some(e) = inner.active.get(&run_id) {
            return e.assignment_token;
        }
        inner
            .entries
            .iter()
            .find(|e| e.run_id == run_id)
            .and_then(|e| e.assignment_token)
    }

    /// Held phase for poll: `waiting` (blocked) vs `starting` (admitted).
    pub fn held_phase(&self, run_id: Uuid) -> Option<&'static str> {
        let inner = self.inner.lock().ok()?;
        if inner.active.contains_key(&run_id) {
            return Some("starting");
        }
        if inner.entries.iter().any(|e| e.run_id == run_id) {
            return Some("waiting");
        }
        None
    }

    /// Queued-at as RFC3339 for poll capacity snapshots.
    pub fn queued_at_rfc3339(&self, run_id: Uuid) -> Option<String> {
        let inner = self.inner.lock().ok()?;
        let ms = inner
            .entries
            .iter()
            .find(|e| e.run_id == run_id)
            .or_else(|| inner.active.get(&run_id))
            .map(|e| e.queued_at)?;
        chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.to_rfc3339())
    }

    pub fn cancel_waiting(&self, run_id: Uuid) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        // Durable-first: delete from SQLite + tombstone, then drop in-memory.
        // A canceled waiting run must never resurrect on restart nor start
        // workspace preparation or an agent.
        let now = chrono::Utc::now().timestamp_millis();
        let origin = inner
            .entries
            .iter()
            .find(|e| e.run_id == run_id)
            .map(|e| e.cloud_origin.clone())
            .unwrap_or_else(|| "unknown".to_string());
        self.store.delete_waiting(run_id)?;
        let _ = self.store.insert_tombstone(run_id, &origin, now);
        let removed = if let Some(pos) = inner.entries.iter().position(|e| e.run_id == run_id) {
            inner.entries.remove(pos);
            inner.revision += 1;
            true
        } else {
            false
        };
        drop(inner);
        // Wake the next oldest waiter (its turn may now be next).
        self.admission.notify_waiters();
        Ok(removed)
    }

    pub fn counts(&self) -> CoordinatorState {
        let inner = self.inner.lock().unwrap();
        let active = inner.active.len();
        let waiting = inner.entries.len();
        // One nonterminal held attempt per runner, so held = active + waiting
        // (distinct runners). Active includes starting/executing/finishing.
        CoordinatorState {
            limit: inner.limit,
            active,
            waiting,
            held: active + waiting,
            revision: inner.revision,
            blocked_reason: inner.blocked_reason.clone(),
        }
    }

    pub fn snapshot(&self) -> pidash_ipc::RunQueueSnapshot {
        let s = self.counts();
        pidash_ipc::RunQueueSnapshot {
            mode: "enabled".to_string(),
            limit: s.limit,
            active: s.active,
            waiting: s.waiting,
            held: s.held,
            revision: s.revision,
            blocked_reason: s.blocked_reason.clone(),
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
        drop(inner);
        // Increases admit more live without daemon restart.
        self.admission.notify_waiters();
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
        Ok(self.try_admit_fifo(run_id))
    }

    /// FIFO admission: only the oldest waiting entry is eligible when at
    /// least one machine slot is free, and `starting` is persisted before
    /// the caller launches the worker.
    pub fn try_admit_fifo(&self, run_id: Uuid) -> bool {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        if (inner.active.len() as u16) >= inner.limit {
            return false;
        }
        // Strict FIFO: only the oldest waiting entry may claim a slot.
        let Some(first) = inner.entries.first() else {
            return false;
        };
        if first.run_id != run_id {
            return false;
        }
        let e = inner.entries.remove(0);
        inner.active.insert(run_id, e);
        inner.revision += 1;
        // Persist starting before launch so a crash cannot replay it.
        // Failure here fails closed: drop the admission.
        let admitted = self.store.update_phase(run_id, "starting");
        if admitted.is_err() {
            inner.active.remove(&run_id);
            inner.revision += 1;
            return false;
        }
        true
    }

    /// Block until this run is admitted (oldest-first) or cancelled.
    /// Returns true when admitted, false when cancelled while waiting.
    /// Caller (AssignWorker) must not start workspace prep on false.
    pub async fn admit_blocking(
        &self,
        run_id: Uuid,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<bool, String> {
        loop {
            if self.try_admit_fifo(run_id) {
                return Ok(true);
            }
            // If the entry is gone (cancel_waiting ran), stop waiting.
            {
                let inner = self.inner.lock().map_err(|_| "poison".to_string())?;
                let still_queued = inner.entries.iter().any(|e| e.run_id == run_id);
                let already_active = inner.active.contains_key(&run_id);
                if already_active {
                    return Ok(true);
                }
                if !still_queued {
                    return Ok(false);
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => return Ok(false),
                _ = self.admission.notified() => {}
            }
        }
    }

    pub fn complete(&self, run_id: Uuid) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| "poison".to_string())?;
        let removed = if inner.active.remove(&run_id).is_some() {
            inner.revision += 1;
            true
        } else {
            false
        };
        drop(inner);
        if removed {
            // Terminal: delete durably so restart never resurrects it.
            // (Terminal-outbox exact-payload retry is ExecutionGate's
            // layer; the coordinator's job is slot release, which must
            // not wait on cloud acknowledgment.)
            let _ = self.store.delete_entry(run_id);
            self.admission.notify_waiters();
        }
        Ok(removed)
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

    #[test]
    fn cancel_waiting_is_durable_across_restart() {
        let dir = std::env::temp_dir().join(format!("coord_cancel_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 3).expect("coord");
        let e1 = entry(Uuid::new_v4(), 1);
        let e2 = entry(Uuid::new_v4(), 2);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        assert!(coord.cancel_waiting(e1.run_id).unwrap());
        assert_eq!(coord.counts().waiting, 1);
        drop(coord);
        // Reopen: canceled entry must not resurrect; tombstone persists.
        let coord2 = Coordinator::new(dir.clone(), 3).expect("coord2");
        assert_eq!(coord2.counts().waiting, 1);
        assert_eq!(coord2.position(e2.run_id), Some(1));
        assert_eq!(coord2.position(e1.run_id), None);
        let _ = coord2.store.is_tombstoned(e1.run_id).unwrap();
        assert!(coord2.store.is_tombstoned(e1.run_id).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn second_daemon_fails_on_fs2_lock() {
        let dir = std::env::temp_dir().join(format!("coord_lock_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let _coord1 = Coordinator::new(dir.clone(), 3).expect("coord1");
        // Second daemon for same data dir exits with actionable error.
        let second = Coordinator::new(dir.clone(), 3);
        assert!(second.is_err(), "second daemon must fail on fs2 lock");
        let msg = second.err().unwrap().to_string();
        assert!(
            msg.contains("run_queue.lock"),
            "error should name the lock: {msg}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshot_carries_live_counts_and_held() {
        let dir = std::env::temp_dir().join(format!("coord_snap_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 2).expect("coord");
        let e1 = entry(Uuid::new_v4(), 1);
        let e2 = entry(Uuid::new_v4(), 2);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        coord.admit(e1.run_id).unwrap();
        let snap = coord.snapshot();
        assert_eq!(snap.mode, "enabled");
        assert_eq!(snap.limit, 2);
        assert_eq!(snap.active, 1);
        assert_eq!(snap.waiting, 1);
        assert_eq!(snap.held, 2);
        assert!(snap.revision > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn held_helpers_for_poll() {
        let dir = std::env::temp_dir().join(format!("coord_held_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 2).expect("coord");
        let tok = Uuid::new_v4();
        let e1 = QueueEntry {
            cloud_origin: "cloud".to_string(),
            runner_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            assignment_token: Some(tok),
            sequence: 1,
            queued_at: 1_700_000_000_000,
            phase: "waiting".to_string(),
            assignment_payload: vec![],
            report_revision: 0,
            process_scope_id: None,
            pending_terminal_payload: None,
            pending_terminal_message_id: None,
        };
        coord.enqueue(e1.clone()).unwrap();
        assert_eq!(coord.held_token(e1.run_id), Some(tok));
        assert_eq!(coord.held_phase(e1.run_id), Some("waiting"));
        let q = coord.queued_at_rfc3339(e1.run_id).unwrap();
        assert!(q.contains("2023-11-14"), "queued_at RFC3339: {q}");
        assert!(coord.admit(e1.run_id).unwrap());
        assert_eq!(coord.held_phase(e1.run_id), Some("starting"));
        assert_eq!(coord.held_token(e1.run_id), Some(tok));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn waiting_for_and_runner_held_for_restore() {
        let dir = std::env::temp_dir().join(format!("coord_restore_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 3).expect("coord");
        let r1 = Uuid::new_v4();
        let e1 = entry(r1, 1);
        let e2 = entry(Uuid::new_v4(), 2);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        // Re-hydration finds the runner's oldest waiting entry.
        assert_eq!(coord.waiting_for(r1).map(|e| e.run_id), Some(e1.run_id));
        assert!(coord.runner_held(r1));
        assert!(coord.admit(e1.run_id).unwrap());
        // Admitted still counts as held; after complete it does not.
        assert!(coord.runner_held(r1));
        coord.complete(e1.run_id).unwrap();
        assert!(!coord.runner_held(r1));
        assert_eq!(coord.waiting_for(r1).map(|e| e.run_id), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fifo_rejects_out_of_order_admit() {
        let dir = std::env::temp_dir().join(format!("coord_fifo_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Coordinator::new(dir.clone(), 3).expect("coord");
        let e1 = entry(Uuid::new_v4(), 1);
        let e2 = entry(Uuid::new_v4(), 2);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        // Newer entry cannot jump ahead of the oldest.
        assert!(!coord.admit(e2.run_id).unwrap());
        assert!(coord.admit(e1.run_id).unwrap());
        assert!(coord.admit(e2.run_id).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn blocking_admit_wakes_oldest_first() {
        let dir = std::env::temp_dir().join(format!("coord_block_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let coord = Arc::new(Coordinator::new(dir.clone(), 1).expect("coord"));
        let e1 = entry(Uuid::new_v4(), 1);
        let e2 = entry(Uuid::new_v4(), 2);
        coord.enqueue(e1.clone()).unwrap();
        coord.enqueue(e2.clone()).unwrap();
        let _cancel1 = tokio_util::sync::CancellationToken::new();
        let cancel2 = tokio_util::sync::CancellationToken::new();
        // Fill the single slot with e1; e2 must block.
        assert!(coord.admit(e1.run_id).unwrap());
        let c2 = coord.clone();
        let run2 = e2.run_id;
        let blocked = tokio::spawn(async move { c2.admit_blocking(run2, &cancel2).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!blocked.is_finished(), "second waiter must block at limit");
        coord.complete(e1.run_id).unwrap();
        let admitted = tokio::time::timeout(std::time::Duration::from_secs(2), blocked)
            .await
            .expect("blocked admit should resolve")
            .expect("join should succeed")
            .expect("blocked admit inner");
        assert!(admitted);
        std::fs::remove_dir_all(&dir).ok();
    }
}
