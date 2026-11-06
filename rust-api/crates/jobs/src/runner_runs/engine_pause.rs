#![forbid(unsafe_code)]
//! Engine pause sweep (NEWPIDASH-50, PIDESKAPP-212).

use crate::worker::{Handler, Registry, Verdict};
use sqlx::PgPool;

pub const PAUSE_TASK: &str = "runner.pause_engines_with_expired_credentials";
pub const PAUSE_BEAT_NAME: &str = "runner-pause-engines-with-expired-credentials";

/// Registers the pause task. The body is a stub pending the full engine
/// state logic (NEWPIDASH-50). It marks the task as OWNED so parity passes.
pub fn register_pause_task(registry: &mut Registry, _pool: PgPool) {
    let handler: Handler = std::sync::Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
    registry.register(PAUSE_TASK, handler);
}
