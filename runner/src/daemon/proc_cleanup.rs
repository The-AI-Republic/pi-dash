//! Daemon-level process ownership: instance identity, the exclusive instance
//! lock, and crash recovery at start. Design:
//! `.ai_design/runner_cleanup/design.md` §4.1.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::util::instance::{self, InstanceLock};
use crate::util::proctree::{self, ProcContext, ProcRecord};

/// What daemon start decided. Keep it alive for the daemon's lifetime: it
/// holds the instance lock.
pub struct ProcOwnership {
    _lock: Option<InstanceLock>,
    /// Present only in the daemon holding the lock.
    pub ctx: Option<ProcContext>,
    /// This instance's records left by a previous daemon, listed before any
    /// new spawn so recovery never sees a live run's record.
    pub stale: Vec<(PathBuf, ProcRecord)>,
}

/// Take the instance lock and list stale records, without installing
/// anything globally (testable).
pub fn prepare(data_dir: &Path) -> ProcOwnership {
    let id = match instance::load_or_create_id(data_dir) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("no instance id ({e:#}); process records and crash recovery disabled");
            return ProcOwnership {
                _lock: None,
                ctx: None,
                stale: Vec::new(),
            };
        }
    };
    match instance::try_lock(data_dir) {
        Ok(Some(lock)) => {
            let ctx = ProcContext::new(id, data_dir);
            let stale = proctree::list_records(&ctx.proc_dir, &ctx.instance_id);
            ProcOwnership {
                _lock: Some(lock),
                ctx: Some(ctx),
                stale,
            }
        }
        Ok(None) => {
            tracing::warn!(
                "another daemon holds {}; running without process records or crash recovery",
                data_dir.join(instance::INSTANCE_LOCK_FILE).display()
            );
            ProcOwnership {
                _lock: None,
                ctx: None,
                stale: Vec::new(),
            }
        }
        Err(e) => {
            tracing::warn!("instance lock failed ({e:#}); running without crash recovery");
            ProcOwnership {
                _lock: None,
                ctx: None,
                stale: Vec::new(),
            }
        }
    }
}

/// How long daemon start waits for crash recovery. Records are resolved in
/// parallel, so this is about one record's worst case (SIGTERM 3 s, SIGKILL
/// 2 s) plus slack.
pub const RECOVERY_BOUND: Duration = Duration::from_secs(15);

/// Daemon start: [`prepare`], install the context for every later spawn, and
/// recover stale records. Returns only once recovery is done (or `bound`
/// passed), so it must run before any runner can spawn an agent.
pub async fn startup(data_dir: &Path) -> ProcOwnership {
    let mut own = prepare(data_dir);
    if let Some(ctx) = &own.ctx {
        proctree::install_context(ctx.clone());
    }
    recover_stale(&mut own, RECOVERY_BOUND).await;
    own
}

/// Resolve `own.stale` before returning. Recovery must finish before any new
/// spawn: a resumed chat gets the same `PIDASH_PROC_TAG` as its stale record,
/// and a tag scan running alongside it would kill the new agent. A record
/// still unresolved at `bound` stays on disk for the next start.
pub async fn recover_stale(own: &mut ProcOwnership, bound: Duration) {
    let stale = std::mem::take(&mut own.stale);
    if stale.is_empty() {
        return;
    }
    let n = stale.len();
    match tokio::time::timeout(bound, proctree::recover(stale)).await {
        Ok(report) => tracing::info!(
            records = n,
            cleaned = report.cleaned,
            resolved = report.resolved,
            kept = report.kept,
            "crash recovery of agent process groups finished"
        ),
        Err(_) => tracing::warn!(
            records = n,
            "crash recovery timed out; unresolved records are kept for the next start"
        ),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::util::proctree::{ScopeKind, write_record};

    fn stale_record(ctx: &ProcContext) {
        write_record(
            &ctx.proc_dir,
            &ProcRecord {
                record_id: uuid::Uuid::new_v4(),
                instance_id: ctx.instance_id.clone(),
                runner_id: None,
                kind: ScopeKind::Run,
                run_or_chat_id: "r".into(),
                tag: format!("{}:run:r", ctx.instance_id),
                scratch: None,
                pgid: None,
                leader_start_time_ms: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn first_daemon_lists_its_stale_records() {
        let dir = tempfile::tempdir().unwrap();
        let first = prepare(dir.path());
        let ctx = first.ctx.clone().unwrap();
        stale_record(&ctx);
        drop(first);
        let again = prepare(dir.path());
        assert_eq!(again.ctx.as_ref().unwrap().instance_id, ctx.instance_id);
        assert_eq!(again.stale.len(), 1);
    }

    #[tokio::test]
    async fn recovery_finishes_before_startup_returns() {
        // A crash between record and spawn left a tagged agent behind. Startup
        // must resolve it before any new agent can be spawned: a resumed chat
        // gets the same tag, and a background tag scan would kill it.
        let dir = tempfile::tempdir().unwrap();
        let first = prepare(dir.path());
        let ctx = first.ctx.clone().unwrap();
        stale_record(&ctx);
        let tag = format!("{}:run:r", ctx.instance_id);
        drop(first);
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg("exec sleep 300")
            .env(crate::util::proctree::PROC_TAG_ENV, &tag)
            .stdin(std::process::Stdio::null());
        crate::util::shell::new_session(&mut cmd);
        let mut stale_agent = cmd.spawn().unwrap();
        let pid = stale_agent.id().unwrap() as i32;
        let reap = tokio::spawn(async move { stale_agent.wait().await });

        let mut own = prepare(dir.path());
        recover_stale(&mut own, Duration::from_secs(30)).await;
        assert!(own.stale.is_empty());
        assert!(
            !crate::agent::tracked::tests::alive(pid),
            "the stale agent is gone before startup returns"
        );
        assert!(proctree::list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
        let _ = reap.await;
    }

    #[test]
    fn second_daemon_on_the_same_data_dir_does_not_recover() {
        let dir = tempfile::tempdir().unwrap();
        let first = prepare(dir.path());
        stale_record(first.ctx.as_ref().unwrap());
        let second = prepare(dir.path());
        assert!(second.ctx.is_none(), "no records, no tag for the second daemon");
        assert!(second.stale.is_empty(), "the second daemon never recovers");
        drop(first);
    }
}
