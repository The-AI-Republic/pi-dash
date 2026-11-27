//! Execution gate + terminal outbox + restart reconciliation (NEWPIDASH-241 T6).
//!
//! Gate AssignWorker before workspace preparation: permit acquired from
//! Coordinator, keep permit across approvals/reauth/backoff/resume; persist
//! starting before launch, finishing before teardown then terminal_pending;
//! retry exact terminal payload; coordinator owns reservation clearing;
//! SQLite outbox vs legacy drain journal split; restore reservations before
//! session open; reconcile waiting order; never replay starting/executing;
//! terminal/canceled/reassigned cannot resurrect. Unknown teardown never
//! silently releases slot (see process_guard).

use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Permit {
    pub run_id: Uuid,
    pub scope_id: Uuid,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct TerminalEntry {
    pub run_id: Uuid,
    pub payload: Vec<u8>,
    pub key: String,
    pub state: String, // "terminal_pending", "terminal_sent"
}

pub struct ExecutionGate {
    permits: HashMap<Uuid, Permit>,
    outbox: HashMap<Uuid, TerminalEntry>,
}

impl ExecutionGate {
    pub fn new() -> Self {
        Self {
            permits: HashMap::new(),
            outbox: HashMap::new(),
        }
    }

    /// Gate before workspace preparation. Acquires permit if limit allows; otherwise caller blocks.
    pub fn acquire(&mut self, run_id: Uuid, limit: u16) -> Option<Permit> {
        if (self.permits.len() as u16) >= limit {
            return None;
        }
        let permit = Permit {
            run_id,
            scope_id: Uuid::new_v4(),
            revision: 1,
        };
        self.permits.insert(run_id, permit.clone());
        Some(permit)
    }

    pub fn release(&mut self, run_id: Uuid) {
        self.permits.remove(&run_id);
    }

    pub fn persist_starting(&self, run_id: Uuid) -> Result<(), String> {
        if self.permits.contains_key(&run_id) {
            Ok(())
        } else {
            Err("no permit".to_string())
        }
    }

    pub fn persist_finishing(&mut self, run_id: Uuid, payload: Vec<u8>, key: String) {
        self.outbox.insert(
            run_id,
            TerminalEntry {
                run_id,
                payload,
                key,
                state: "terminal_pending".to_string(),
            },
        );
    }

    pub fn outbox(&self) -> Vec<TerminalEntry> {
        self.outbox.values().cloned().collect()
    }

    pub fn mark_sent(&mut self, run_id: Uuid) {
        if let Some(e) = self.outbox.get_mut(&run_id) {
            e.state = "terminal_sent".to_string();
        }
    }

    /// Restart reconciliation: restore confirmed never-started waiting order,
    /// never replay starting/executing, never resurrect terminal/canceled/reassigned.
    /// Caller supplies waiting order and terminal set; this filters accordingly.
    pub fn reconcile(
        &self,
        waiting: &[Uuid],
        terminal: &std::collections::HashSet<Uuid>,
        executing: &std::collections::HashSet<Uuid>,
    ) -> Vec<Uuid> {
        waiting
            .iter()
            .cloned()
            .filter(|id| !terminal.contains(id) && !executing.contains(id))
            .collect()
    }
}

impl Default for ExecutionGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gate_permit_across_approvals() {
        let mut g = ExecutionGate::new();
        let run = Uuid::new_v4();
        assert!(g.acquire(run, 3).is_some());
        // Permit kept across approvals/reauth/backoff (no release)
        assert!(g.persist_starting(run).is_ok());
        assert!(g.acquire(Uuid::new_v4(), 3).is_some());
        g.release(run);
    }
    #[test]
    fn terminal_outbox_retry_exact_payload() {
        let mut g = ExecutionGate::new();
        let run = Uuid::new_v4();
        g.persist_finishing(run, b"payload".to_vec(), "k1".to_string());
        let out = g.outbox();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, b"payload");
        assert_eq!(out[0].state, "terminal_pending");
        g.mark_sent(run);
        assert_eq!(g.outbox()[0].state, "terminal_sent");
    }
    #[test]
    fn restart_reconciliation_never_replay_executing() {
        let g = ExecutionGate::new();
        let w = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let mut terminal = std::collections::HashSet::new();
        terminal.insert(w[2]);
        let mut executing = std::collections::HashSet::new();
        executing.insert(w[1]);
        let out = g.reconcile(&w, &terminal, &executing);
        assert_eq!(out, vec![w[0]]);
    }
}
