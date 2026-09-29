//! Assistant Celery tasks (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/tasks.py` (PIDASHCONV-254, F-A6-10):
//!
//! * [`run_turn`] — the turn pipeline: context load, atomic claim,
//!   model/toolset resolution, delta-streaming agent run, complete/fail/
//!   cancel finalizers, error classification (`tasks.py:1-508`).
//! * [`sweep`] — the stale-turn sweep (`tasks.py:511-526`).
//!
//! [`TASK_NAMES`] pins the two Celery wire names; [`register_assistant_tasks`]
//! builds the handler table for the worker [`Registry`][crate::worker::Registry].
//! Like the mail tasks, registration does not flip the live worker — the
//! domain gate flips ownership after the proxy pass, so until then both names
//! still route to `PythonOwned` (see [`crate::worker::route_for`]).

pub mod run_turn;
pub mod sweep;

pub use run_turn::{
    cancel_key, classify_error, context_eligible, register_turn_handler, DriveResult, SeamError,
    SkippedServer, StreamItem, StreamSink, TurnContext, TurnSeam, TurnUsage, RUN_TURN_TASK,
};
pub use sweep::{drive_sweep, register_sweep_handler, SWEEP_TASK};

/// Every Celery task name this domain owns. `assistant.run_turn` gets its
/// handler in [`register_turn_handler`]; `assistant.sweep_stale_turns` in
/// [`register_sweep_handler`] — either side speaks the same wire payloads,
/// so no fan-out is ever dropped or double-run across the handoff.
pub const TASK_NAMES: [&str; 2] = [RUN_TURN_TASK, SWEEP_TASK];

/// Build the handler table for both assistant tasks. The pool feeds the
/// sweep's live store; the seam drives the turn pipeline (see
/// [`run_turn::TurnSeam`] for the port boundary).
pub fn register_assistant_tasks<S: TurnSeam + 'static>(
    registry: &mut crate::worker::Registry,
    pool: sqlx::PgPool,
    seam: std::sync::Arc<S>,
) {
    register_turn_handler(registry, pool.clone(), seam);
    register_sweep_handler(registry, pool);
}

#[cfg(test)]
mod tests {
    use super::run_turn::{AgentError, ModelError, ToolsetHandle};
    use super::*;
    use crate::worker::{route_for, Route};
    use serde_json::Value;
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn task_names_are_the_celery_wire_names() {
        assert_eq!(
            TASK_NAMES,
            ["assistant.run_turn", "assistant.sweep_stale_turns"]
        );
    }

    /// Minimal seam: every method reports "unimplemented" as infra failure.
    /// Registration only needs the table entries, never a live provider.
    struct NoopSeam;

    impl TurnSeam for NoopSeam {
        type Stream = ();
        async fn load_context(&self, _: Uuid) -> Result<Option<TurnContext>, SeamError> {
            Ok(None)
        }
        async fn mark_running(&self, _: &TurnContext) -> Result<bool, SeamError> {
            Ok(false)
        }
        async fn is_cancelled(&self, _: Uuid) -> Result<bool, SeamError> {
            Ok(false)
        }
        async fn resolve_model(&self, _: &TurnContext) -> Result<String, ModelError> {
            Err(ModelError {
                code: "x".to_owned(),
                detail: "x".to_owned(),
            })
        }
        async fn resolve_toolsets(
            &self,
            _: &TurnContext,
        ) -> Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError> {
            Err(SeamError("x".to_owned()))
        }
        async fn emit_skipped(
            &self,
            _: &TurnContext,
            _: &[SkippedServer],
        ) -> Result<(), SeamError> {
            Ok(())
        }
        async fn runtime_failures(
            &self,
            _: &[ToolsetHandle],
        ) -> Result<Vec<SkippedServer>, SeamError> {
            Ok(Vec::new())
        }
        async fn load_history(&self, _: &TurnContext) -> Result<Value, SeamError> {
            Ok(Value::Null)
        }
        async fn model_label(&self, _: &TurnContext) -> Result<String, SeamError> {
            Ok(String::new())
        }
        async fn open_stream(
            &self,
            _: &TurnContext,
            _: &Value,
            _: &[ToolsetHandle],
        ) -> Result<(), AgentError> {
            Err(AgentError::UsageLimit(String::new()))
        }
        async fn next_event(&self, _: &mut ()) -> Result<Option<StreamItem>, AgentError> {
            Ok(None)
        }
        fn now_ms(&self) -> u64 {
            0
        }
        async fn start_row(&self, _: &TurnContext) -> Result<Uuid, SeamError> {
            Err(SeamError("x".to_owned()))
        }
        async fn emit_delta(&self, _: &TurnContext, _: Uuid, _: &str) -> Result<(), SeamError> {
            Ok(())
        }
        async fn finalize_row(
            &self,
            _: &TurnContext,
            _: Uuid,
            _: &str,
            _: &'static str,
        ) -> Result<(), SeamError> {
            Ok(())
        }
        async fn dump_messages(&self, _: &()) -> Result<Value, SeamError> {
            Ok(Value::Null)
        }
        async fn extract_usage(&self, _: &()) -> Result<Option<TurnUsage>, SeamError> {
            Ok(None)
        }
        async fn complete_turn(
            &self,
            _: &TurnContext,
            _: Value,
            _: Value,
            _: String,
        ) -> Result<(), SeamError> {
            Ok(())
        }
        async fn fail_turn(&self, _: &TurnContext, _: &str, _: &str) -> Result<(), SeamError> {
            Ok(())
        }
        async fn cancel_turn(&self, _: &TurnContext) -> Result<(), SeamError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn registered_tasks_route_local() {
        let mut registry = crate::worker::Registry::new();
        for name in TASK_NAMES {
            assert_eq!(route_for(&registry, name), Route::PythonOwned);
        }
        // `connect_lazy` opens no connection: registration is pure table
        // work, so this runs with no database.
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/assistant_contract_unused")
            .expect("lazy pool");
        register_assistant_tasks(&mut registry, pool, Arc::new(NoopSeam));
        for name in TASK_NAMES {
            assert_eq!(
                route_for(&registry, name),
                Route::Local,
                "{name} must run locally"
            );
        }
    }
}
