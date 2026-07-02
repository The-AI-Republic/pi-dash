use std::collections::BTreeMap;
use std::time::Duration;

use crate::cloud::protocol::{ClientMsg, RunEventRecord};
use crate::daemon::runner_out::RunnerOut;

const RUN_EVENT_BATCH_SIZE: usize = 32;
const RUN_EVENT_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const RUN_EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const RUN_EVENT_LIFECYCLE_FLUSH_ATTEMPTS: usize = 3;
const RUN_EVENT_LIFECYCLE_RETRY_DELAY: Duration = Duration::from_millis(250);
const RUN_EVENT_MAX_SEQ: u32 = i32::MAX as u32;
const RUN_EVENT_COMPACT_KIND: &str = "agent/stream_activity";

#[derive(Debug, Default)]
struct CompactRunEventBuffer {
    count: u64,
    first_kind: Option<String>,
    last_kind: Option<String>,
    kind_counts: BTreeMap<String, u64>,
}

impl CompactRunEventBuffer {
    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn push(&mut self, kind: String) {
        if self.is_empty() {
            self.first_kind = Some(kind.clone());
        }
        self.last_kind = Some(kind.clone());
        self.count = self.count.saturating_add(1);
        self.kind_counts
            .entry(kind)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }

    fn take_payload(&mut self) -> Option<serde_json::Value> {
        if self.is_empty() {
            return None;
        }
        let compacted = std::mem::take(self);
        let event_word = if compacted.count == 1 {
            "event"
        } else {
            "events"
        };
        Some(serde_json::json!({
            "schema": "runner_event_compact_v1",
            "summary": format!("{} low-signal agent {event_word}", compacted.count),
            "compacted": true,
            "event_count": compacted.count,
            "first_kind": compacted.first_kind.unwrap_or_default(),
            "last_kind": compacted.last_kind.unwrap_or_default(),
            "kind_counts": compacted.kind_counts,
        }))
    }
}

fn is_compactable_run_event(kind: &str) -> bool {
    kind.starts_with("stream_event/")
        || matches!(
            kind,
            "assistant/message"
                | "system/status"
                | "system/task_progress"
                | "system/task_updated"
                | "unknown"
                | "user/toolResult"
        )
}

pub(crate) struct RunEventMirror {
    run_id: uuid::Uuid,
    next_seq: u32,
    pending: Vec<RunEventRecord>,
    compact: CompactRunEventBuffer,
    flush_deadline: Option<tokio::time::Instant>,
    seq_exhausted_warned: bool,
}

impl RunEventMirror {
    pub(crate) fn new(run_id: uuid::Uuid) -> Self {
        Self {
            run_id,
            next_seq: 0,
            pending: Vec::with_capacity(RUN_EVENT_BATCH_SIZE),
            compact: CompactRunEventBuffer::default(),
            flush_deadline: None,
            seq_exhausted_warned: false,
        }
    }

    pub(crate) fn flush_deadline(&self) -> Option<tokio::time::Instant> {
        self.flush_deadline
    }

    fn ensure_flush_deadline(&mut self) {
        if self.flush_deadline.is_none() {
            self.flush_deadline = Some(tokio::time::Instant::now() + RUN_EVENT_FLUSH_INTERVAL);
        }
    }

    fn warn_seq_exhausted_once(&mut self) {
        if !self.seq_exhausted_warned {
            self.seq_exhausted_warned = true;
            tracing::warn!(
                run_id = %self.run_id,
                max_seq = RUN_EVENT_MAX_SEQ,
                "agent run event sequence exhausted; dropping further mirrored events"
            );
        }
    }

    fn push_record(&mut self, kind: String, payload: serde_json::Value) -> bool {
        if self.next_seq >= RUN_EVENT_MAX_SEQ {
            self.warn_seq_exhausted_once();
            return false;
        }
        self.ensure_flush_deadline();
        self.next_seq += 1;
        self.pending.push(RunEventRecord {
            seq: self.next_seq,
            kind,
            payload,
        });
        self.pending.len() >= RUN_EVENT_BATCH_SIZE
    }

    fn flush_compact_to_pending(&mut self) -> bool {
        if self.compact.is_empty() {
            return false;
        }
        // Check seq availability before taking the buffer: `take_payload` resets
        // it via `mem::take`, so if `push_record` then rejected due to seq
        // exhaustion the compacted events would be silently dropped.
        if self.next_seq >= RUN_EVENT_MAX_SEQ {
            self.warn_seq_exhausted_once();
            return false;
        }
        let payload = self
            .compact
            .take_payload()
            .expect("compact buffer non-empty checked above");
        self.push_record(RUN_EVENT_COMPACT_KIND.to_string(), payload)
    }

    /// Buffer a run event. Returns `true` if the pending batch reached
    /// `RUN_EVENT_BATCH_SIZE` (either via this event or via the compact-buffer
    /// flush that a milestone event triggers) and the caller should flush.
    pub(crate) fn push(&mut self, kind: String, summary: String) -> bool {
        if is_compactable_run_event(&kind) {
            if self.next_seq >= RUN_EVENT_MAX_SEQ {
                self.warn_seq_exhausted_once();
                return false;
            }
            self.ensure_flush_deadline();
            self.compact.push(kind);
            return false;
        }
        let should_flush = self.flush_compact_to_pending();
        self.push_record(
            kind,
            serde_json::json!({
                "schema": "runner_event_summary_v1",
                "summary": summary,
            }),
        ) || should_flush
    }

    /// Buffer a full-content run event (e.g. the agent's narrative message)
    /// that must reach the cloud verbatim rather than being folded into the
    /// count-only compaction summary. Like a milestone event it first flushes
    /// any pending compact buffer so ordering relative to surrounding
    /// low-signal events is preserved, then records the payload as-is under
    /// `kind`. Returns `true` if the caller should flush.
    pub(crate) fn push_content(&mut self, kind: String, payload: serde_json::Value) -> bool {
        let should_flush = self.flush_compact_to_pending();
        self.push_record(kind, payload) || should_flush
    }

    pub(crate) async fn flush(&mut self, out: &RunnerOut) -> bool {
        self.flush_compact_to_pending();
        if self.pending.is_empty() {
            self.flush_deadline = None;
            return true;
        }
        let mut events = Vec::with_capacity(RUN_EVENT_BATCH_SIZE);
        std::mem::swap(&mut events, &mut self.pending);
        let count = events.len();
        let msg = ClientMsg::RunEvents {
            run_id: self.run_id,
            events: events.clone(),
        };
        match tokio::time::timeout(RUN_EVENT_SEND_TIMEOUT, out.send(msg)).await {
            Ok(Ok(())) => {
                self.flush_deadline = None;
                true
            }
            Ok(Err(err)) => {
                self.pending = events;
                self.flush_deadline = Some(tokio::time::Instant::now() + RUN_EVENT_FLUSH_INTERVAL);
                tracing::warn!(
                    run_id = %self.run_id,
                    count,
                    error = %err,
                    "failed to mirror agent run events"
                );
                false
            }
            Err(_) => {
                self.pending = events;
                self.flush_deadline = Some(tokio::time::Instant::now() + RUN_EVENT_FLUSH_INTERVAL);
                tracing::warn!(
                    run_id = %self.run_id,
                    count,
                    timeout_ms = RUN_EVENT_SEND_TIMEOUT.as_millis(),
                    "timed out mirroring agent run events"
                );
                false
            }
        }
    }

    pub(crate) async fn flush_before_lifecycle(&mut self, out: &RunnerOut) -> bool {
        for attempt in 0..RUN_EVENT_LIFECYCLE_FLUSH_ATTEMPTS {
            if self.flush(out).await {
                return true;
            }
            if attempt + 1 < RUN_EVENT_LIFECYCLE_FLUSH_ATTEMPTS {
                tokio::time::sleep(RUN_EVENT_LIFECYCLE_RETRY_DELAY).await;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::protocol::Envelope;
    use tokio::sync::mpsc;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_event_mirror_flushes_sanitized_batch() {
        let runner_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let out = RunnerOut::new(runner_id, out_tx);
        let mut mirror = RunEventMirror::new(run_id);

        assert!(!mirror.push(
            "assistant/message".into(),
            "raw method=assistant/message".into()
        ));
        assert!(!mirror.push("run/completed".into(), "run completed".into()));
        assert!(mirror.flush(&out).await);

        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for RunEvents")
            .expect("channel closed before RunEvents arrived");
        assert_eq!(env.runner_id, Some(runner_id));
        match env.body {
            ClientMsg::RunEvents {
                run_id: seen_run_id,
                events,
            } => {
                assert_eq!(seen_run_id, run_id);
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].seq, 1);
                assert_eq!(events[0].kind, RUN_EVENT_COMPACT_KIND);
                assert_eq!(
                    events[0].payload,
                    serde_json::json!({
                        "schema": "runner_event_compact_v1",
                        "summary": "1 low-signal agent event",
                        "compacted": true,
                        "event_count": 1,
                        "first_kind": "assistant/message",
                        "last_kind": "assistant/message",
                        "kind_counts": {
                            "assistant/message": 1,
                        },
                    })
                );
                assert_eq!(events[1].seq, 2);
                assert_eq!(events[1].kind, "run/completed");
                assert_eq!(
                    events[1].payload,
                    serde_json::json!({
                        "schema": "runner_event_summary_v1",
                        "summary": "run completed",
                    })
                );
            }
            other => panic!("expected RunEvents, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_event_mirror_compacts_contiguous_low_signal_events_before_milestones() {
        let runner_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let out = RunnerOut::new(runner_id, out_tx);
        let mut mirror = RunEventMirror::new(run_id);

        assert!(!mirror.push("system/status".into(), "raw method=system/status".into()));
        assert!(!mirror.push(
            "stream_event/message_start".into(),
            "raw method=stream_event/message_start".into()
        ));
        assert!(!mirror.push(
            "user/toolResult".into(),
            "raw method=user/toolResult".into()
        ));
        assert!(!mirror.push(
            "system/api_retry".into(),
            "raw method=system/api_retry".into()
        ));
        assert!(mirror.flush(&out).await);

        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for RunEvents")
            .expect("channel closed before RunEvents arrived");
        match env.body {
            ClientMsg::RunEvents { events, .. } => {
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].seq, 1);
                assert_eq!(events[0].kind, RUN_EVENT_COMPACT_KIND);
                assert_eq!(
                    events[0].payload,
                    serde_json::json!({
                        "schema": "runner_event_compact_v1",
                        "summary": "3 low-signal agent events",
                        "compacted": true,
                        "event_count": 3,
                        "first_kind": "system/status",
                        "last_kind": "user/toolResult",
                        "kind_counts": {
                            "stream_event/message_start": 1,
                            "system/status": 1,
                            "user/toolResult": 1,
                        },
                    })
                );
                assert_eq!(events[1].seq, 2);
                assert_eq!(events[1].kind, "system/api_retry");
                assert_eq!(
                    events[1].payload,
                    serde_json::json!({
                        "schema": "runner_event_summary_v1",
                        "summary": "raw method=system/api_retry",
                    })
                );
            }
            other => panic!("expected RunEvents, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_event_mirror_retains_batch_after_send_failure() {
        let runner_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();
        let (failed_tx, failed_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        drop(failed_rx);
        let failed_out = RunnerOut::new(runner_id, failed_tx);
        let mut mirror = RunEventMirror::new(run_id);

        mirror.push(
            "assistant/message".into(),
            "raw method=assistant/message".into(),
        );
        assert!(!mirror.flush(&failed_out).await);
        assert_eq!(mirror.pending.len(), 1);
        assert!(mirror.flush_deadline().is_some());

        let (ok_tx, mut ok_rx) = mpsc::channel::<Envelope<ClientMsg>>(1);
        let ok_out = RunnerOut::new(runner_id, ok_tx);
        assert!(mirror.flush(&ok_out).await);
        assert!(mirror.pending.is_empty());
        assert!(mirror.flush_deadline().is_none());

        let env = tokio::time::timeout(std::time::Duration::from_secs(2), ok_rx.recv())
            .await
            .expect("timed out waiting for retry")
            .expect("channel closed before retry arrived");
        match env.body {
            ClientMsg::RunEvents { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].seq, 1);
                assert_eq!(events[0].kind, RUN_EVENT_COMPACT_KIND);
            }
            other => panic!("expected RunEvents, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_event_mirror_forwards_content_verbatim_after_compacting_prior_low_signal() {
        let runner_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();
        let (out_tx, mut out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
        let out = RunnerOut::new(runner_id, out_tx);
        let mut mirror = RunEventMirror::new(run_id);

        // A couple of low-signal deltas, then the agent's narrative message.
        assert!(!mirror.push(
            "stream_event/content_block_delta".into(),
            "raw method=stream_event/content_block_delta".into()
        ));
        let content = serde_json::json!({
            "schema": "runner_agent_message_v1",
            "text": "Let me first explore the original color of the button.",
        });
        assert!(!mirror.push_content("agent/message".into(), content.clone()));
        assert!(mirror.flush(&out).await);

        let env = tokio::time::timeout(std::time::Duration::from_secs(2), out_rx.recv())
            .await
            .expect("timed out waiting for RunEvents")
            .expect("channel closed before RunEvents arrived");
        match env.body {
            ClientMsg::RunEvents { events, .. } => {
                // First the compacted delta summary, then the verbatim message.
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].seq, 1);
                assert_eq!(events[0].kind, RUN_EVENT_COMPACT_KIND);
                assert_eq!(events[1].seq, 2);
                assert_eq!(events[1].kind, "agent/message");
                assert_eq!(events[1].payload, content);
            }
            other => panic!("expected RunEvents, got {other:?}"),
        }
    }

    #[test]
    fn run_event_mirror_keeps_flush_deadline_until_flush() {
        let run_id = uuid::Uuid::new_v4();
        let mut mirror = RunEventMirror::new(run_id);

        assert!(mirror.flush_deadline().is_none());
        assert!(!mirror.push("raw".into(), "raw method=a".into()));
        let first_deadline = mirror.flush_deadline().expect("deadline after first event");
        assert!(!mirror.push("raw".into(), "raw method=b".into()));
        assert_eq!(mirror.flush_deadline(), Some(first_deadline));
    }

    #[test]
    fn run_event_mirror_sets_flush_deadline_for_compacted_events() {
        let run_id = uuid::Uuid::new_v4();
        let mut mirror = RunEventMirror::new(run_id);

        assert!(mirror.flush_deadline().is_none());
        assert!(!mirror.push(
            "stream_event/content_block_delta".into(),
            "raw method=stream_event/content_block_delta".into()
        ));
        assert!(mirror.flush_deadline().is_some());
        assert!(mirror.pending.is_empty());
        assert!(!mirror.compact.is_empty());
    }

    #[test]
    fn run_event_mirror_auto_flushes_at_batch_size() {
        let run_id = uuid::Uuid::new_v4();
        let mut mirror = RunEventMirror::new(run_id);

        for i in 0..RUN_EVENT_BATCH_SIZE {
            let should_flush = mirror.push("raw".into(), format!("raw method={i}"));
            assert_eq!(should_flush, i + 1 == RUN_EVENT_BATCH_SIZE);
        }
        assert_eq!(mirror.pending.len(), RUN_EVENT_BATCH_SIZE);
    }

    #[test]
    fn run_event_mirror_preserves_compact_buffer_when_seq_exhausted() {
        let run_id = uuid::Uuid::new_v4();
        let mut mirror = RunEventMirror::new(run_id);

        assert!(!mirror.push(
            "stream_event/message_start".into(),
            "raw method=stream_event/message_start".into()
        ));
        assert!(!mirror.compact.is_empty());

        mirror.next_seq = RUN_EVENT_MAX_SEQ;

        // Milestone event with seq exhausted: neither the milestone nor the
        // compact summary can be persisted, but the compact buffer's contents
        // must not silently disappear via `mem::take`.
        assert!(!mirror.push(
            "system/api_retry".into(),
            "raw method=system/api_retry".into()
        ));
        assert!(!mirror.compact.is_empty());
        assert!(mirror.pending.is_empty());
    }

    #[test]
    fn run_event_mirror_caps_seq_at_cloud_column_limit() {
        let run_id = uuid::Uuid::new_v4();
        let mut mirror = RunEventMirror::new(run_id);
        mirror.next_seq = RUN_EVENT_MAX_SEQ - 1;

        assert!(!mirror.push("raw".into(), "raw method=last".into()));
        assert_eq!(
            mirror.pending.last().map(|event| event.seq),
            Some(RUN_EVENT_MAX_SEQ)
        );
        assert!(!mirror.push("raw".into(), "raw method=dropped".into()));
        assert_eq!(mirror.pending.len(), 1);
        assert_eq!(mirror.next_seq, RUN_EVENT_MAX_SEQ);
    }
}
