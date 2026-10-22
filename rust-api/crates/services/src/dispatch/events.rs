//! Managed-runner run-creation events (MR-11, PDASHOSS01-119).
//!
//! Port of `apps/api/pi_dash/managed_runner/signals.py`
//! (`log_managed_run_pinning`): the `post_save(AgentRun, created=True)`
//! receiver that logs `managed_runner.run_pinned` /
//! `managed_runner.queued_waiting` (design §15.1).
//!
//! Translation notes:
//!
//! * Signals become explicit calls (porting guide): every `agent_run`
//!   INSERT that can carry a managed run calls [`log_managed_run_created`]
//!   right after the row is written, as `post_save` fires at save time
//!   (inside the transaction, not on commit). The creation kernels
//!   (`orchestration::creation`, `orchestration::entries`) and the direct
//!   create (`api::runner_runs::dispatch::post_direct`) are those sites.
//! * Python logs `"managed_runner.run_pinned run=%s runner=%s"`; the Rust
//!   events keep the event name as the message and carry `run` / `runner`
//!   as fields, like the already-ported `managed_runner.enrolled` /
//!   `.queued_expired` / `.token_issued`. A parked run without a pin logs
//!   `runner=None`, as `%s` of `None` does.
//! * Only ids are logged — never a token or a filesystem path.
//!
//! Ported bugs: none found in this unit on read-through.

use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use uuid::Uuid;

/// Which event a freshly created run emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRunEvent {
    /// Pinned to a live desktop.
    RunPinned,
    /// Automatic run parked until the desktop reconnects
    /// (`error_code = desktop_not_connected`).
    QueuedWaiting,
}

impl ManagedRunEvent {
    /// The event name (the log message).
    pub fn name(self) -> &'static str {
        match self {
            ManagedRunEvent::RunPinned => "managed_runner.run_pinned",
            ManagedRunEvent::QueuedWaiting => "managed_runner.queued_waiting",
        }
    }
}

/// The receiver's branch (`signals.py` `log_managed_run_pinning`): no event
/// for other executor kinds; `queued_waiting` when parked; `run_pinned`
/// when a runner is pinned; otherwise nothing.
pub fn managed_run_created_event(
    executor_kind: AgentExecutorKind,
    error_code: &str,
    pinned_runner_id: Option<Uuid>,
) -> Option<ManagedRunEvent> {
    if executor_kind != AgentExecutorKind::ManagedRunner {
        return None;
    }
    if error_code == ManagedRunnerReason::NOT_CONNECTED {
        Some(ManagedRunEvent::QueuedWaiting)
    } else if pinned_runner_id.is_some() {
        Some(ManagedRunEvent::RunPinned)
    } else {
        None
    }
}

/// `%s` of an optional id: the UUID, or Python's `None`.
fn runner_display(pinned_runner_id: Option<Uuid>) -> String {
    pinned_runner_id.map_or_else(|| "None".to_owned(), |id| id.to_string())
}

/// Emit the creation event for a newly inserted run, if it has one.
pub fn log_managed_run_created(
    run_id: Uuid,
    executor_kind: AgentExecutorKind,
    error_code: &str,
    pinned_runner_id: Option<Uuid>,
) {
    let Some(event) = managed_run_created_event(executor_kind, error_code, pinned_runner_id) else {
        return;
    };
    let runner = runner_display(pinned_runner_id);
    match event {
        ManagedRunEvent::RunPinned => {
            tracing::info!(run = %run_id, runner = %runner, "managed_runner.run_pinned")
        }
        ManagedRunEvent::QueuedWaiting => {
            tracing::info!(run = %run_id, runner = %runner, "managed_runner.queued_waiting")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() -> Option<Uuid> {
        Some(Uuid::from_u128(7))
    }

    #[test]
    fn pinned_managed_run_is_run_pinned() {
        assert_eq!(
            managed_run_created_event(AgentExecutorKind::ManagedRunner, "", pin()),
            Some(ManagedRunEvent::RunPinned)
        );
    }

    #[test]
    fn parked_managed_run_is_queued_waiting_even_with_a_pin() {
        assert_eq!(
            managed_run_created_event(
                AgentExecutorKind::ManagedRunner,
                "desktop_not_connected",
                pin()
            ),
            Some(ManagedRunEvent::QueuedWaiting)
        );
        assert_eq!(
            managed_run_created_event(
                AgentExecutorKind::ManagedRunner,
                "desktop_not_connected",
                None
            ),
            Some(ManagedRunEvent::QueuedWaiting)
        );
    }

    #[test]
    fn unpinned_managed_run_without_reason_is_silent() {
        assert_eq!(
            managed_run_created_event(AgentExecutorKind::ManagedRunner, "", None),
            None
        );
    }

    #[test]
    fn other_executor_kinds_are_silent() {
        for kind in [
            AgentExecutorKind::LocalRunner,
            AgentExecutorKind::CloudAgent,
        ] {
            assert_eq!(managed_run_created_event(kind, "", pin()), None);
            assert_eq!(
                managed_run_created_event(kind, "desktop_not_connected", pin()),
                None
            );
        }
    }

    #[test]
    fn event_names_match_python() {
        assert_eq!(
            ManagedRunEvent::RunPinned.name(),
            "managed_runner.run_pinned"
        );
        assert_eq!(
            ManagedRunEvent::QueuedWaiting.name(),
            "managed_runner.queued_waiting"
        );
    }

    #[test]
    fn missing_runner_renders_as_python_none() {
        assert_eq!(runner_display(None), "None");
        assert_eq!(
            runner_display(pin()),
            "00000000-0000-0000-0000-000000000007"
        );
    }
}
