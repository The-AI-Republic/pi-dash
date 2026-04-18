use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, broadcast};
use uuid::Uuid;

use crate::cloud::protocol::{ApprovalDecision, ApprovalKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Local,
    Cloud,
    Policy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub run_id: Uuid,
    pub kind: ApprovalKind,
    pub payload: serde_json::Value,
    pub reason: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: ApprovalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Resolved {
        decision: ApprovalDecision,
        source: DecisionSource,
        decided_at: DateTime<Utc>,
    },
    Expired,
}

#[derive(Clone)]
pub struct ApprovalRouter {
    inner: Arc<Mutex<State>>,
    updated: Arc<Notify>,
    events: broadcast::Sender<ApprovalRecord>,
}

struct State {
    pending: HashMap<String, ApprovalRecord>,
}

impl ApprovalRouter {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(Mutex::new(State {
                pending: HashMap::new(),
            })),
            updated: Arc::new(Notify::new()),
            events: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalRecord> {
        self.events.subscribe()
    }

    pub async fn open(&self, rec: ApprovalRecord) {
        {
            let mut s = self.inner.lock().await;
            s.pending.insert(rec.approval_id.clone(), rec.clone());
        }
        let _ = self.events.send(rec);
        self.updated.notify_waiters();
    }

    pub async fn list_pending(&self) -> Vec<ApprovalRecord> {
        let s = self.inner.lock().await;
        s.pending.values().cloned().collect()
    }

    pub async fn decide(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
        source: DecisionSource,
    ) -> Option<ApprovalRecord> {
        let mut s = self.inner.lock().await;
        let rec = s.pending.remove(approval_id)?;
        let resolved = ApprovalRecord {
            status: ApprovalStatus::Resolved {
                decision,
                source,
                decided_at: Utc::now(),
            },
            ..rec
        };
        let _ = self.events.send(resolved.clone());
        self.updated.notify_waiters();
        Some(resolved)
    }

    pub async fn expire(&self, approval_id: &str) -> Option<ApprovalRecord> {
        let mut s = self.inner.lock().await;
        let rec = s.pending.remove(approval_id)?;
        let expired = ApprovalRecord {
            status: ApprovalStatus::Expired,
            ..rec
        };
        let _ = self.events.send(expired.clone());
        self.updated.notify_waiters();
        Some(expired)
    }

    pub fn notified(&self) -> std::sync::Arc<Notify> {
        self.updated.clone()
    }
}

impl Default for ApprovalRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> ApprovalRecord {
        ApprovalRecord {
            approval_id: "a1".into(),
            run_id: Uuid::new_v4(),
            kind: ApprovalKind::CommandExecution,
            payload: serde_json::json!({ "command": "rm /x" }),
            reason: None,
            requested_at: Utc::now(),
            expires_at: None,
            status: ApprovalStatus::Pending,
        }
    }

    #[tokio::test]
    async fn first_decision_wins() {
        let r = ApprovalRouter::new();
        r.open(rec()).await;
        let a = r
            .decide("a1", ApprovalDecision::Accept, DecisionSource::Local)
            .await;
        let b = r
            .decide("a1", ApprovalDecision::Decline, DecisionSource::Cloud)
            .await;
        assert!(a.is_some());
        assert!(b.is_none());
    }
}
