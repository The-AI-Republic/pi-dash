//! Containment for chat runtimes (PDASHOSS01-417).
//!
//! A chat session runs its agent in the runner's working copy, like a run,
//! and the agent can start background processes there (a dev server, a
//! watcher). A [`ChatUnit`] is the session's unit of containment, on the
//! same backends as a run ([`crate::util::containment`]): the agents the
//! session spawns inside [`ChatUnit::scoped`] join its process group (or
//! are moved into its cgroup by [`ChatUnit::adopt`]), and
//! [`ChatUnit::finish`] stops what is left when the session ends, with the
//! same ownership proof before every signal. Nothing unproven is signalled.
//!
//! The unit lives as long as the session's runtime, not one turn: what a
//! turn starts may be meant for the next turn. The working copy is handed
//! to a run or another chat runtime only once the unit is finished clean.

use std::future::Future;

use uuid::Uuid;

use crate::util::containment::{self, CleanupOutcome, RunGroup};
use crate::util::shell::{CHAT_ENV, ChatEnv};

/// A chat session's unit of containment.
#[derive(Debug)]
pub struct ChatUnit {
    group: RunGroup,
}

impl ChatUnit {
    /// Create the unit for `chat_session_id`.
    pub fn new(chat_session_id: Uuid) -> Self {
        Self {
            group: containment::create_run_group(chat_session_id),
        }
    }

    /// Run `fut` with every agent it spawns placed in this unit's process
    /// group (process-group backend).
    pub async fn scoped<F: Future>(&self, fut: F) -> F::Output {
        CHAT_ENV.scope(self.env(), fut).await
    }

    /// What [`Self::scoped`] puts in [`CHAT_ENV`].
    pub fn env(&self) -> ChatEnv {
        ChatEnv {
            backend: self.group.backend_kind(),
            process_group: self.group.agent_pgid(),
        }
    }

    /// Record an agent pid just spawned (cgroup backend: moved into the
    /// unit's cgroup).
    pub fn adopt(&mut self, pid: Option<u32>) {
        self.group.move_pid(pid);
    }

    /// Stop what is left in the unit. Safe to call again: a later call
    /// re-proves and re-checks, which is how a caller waits out
    /// [`CleanupOutcome::Unresolved`].
    pub async fn finish(&mut self) -> CleanupOutcome {
        let outcome = self.group.cleanup().await;
        if outcome == CleanupOutcome::Clean {
            self.group.mark_cleaned();
        }
        outcome
    }
}

#[cfg(all(test, unix))]
impl ChatUnit {
    /// A unit over `group` (tests: a group a stranger holds).
    pub(crate) fn from_group(group: RunGroup) -> Self {
        Self { group }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn spawn_sleeper() -> tokio::process::Child {
        crate::util::shell::login_shell_command("sleep", &["300"], None)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    fn alive(pid: u32) -> bool {
        crate::util::procinfo::info(pid as i32).is_some_and(|p| !p.zombie)
    }

    /// What the session's agent leaves running is stopped when the
    /// session's unit is finished.
    #[tokio::test]
    async fn finishing_a_chat_unit_stops_what_its_agent_left_running() {
        let mut unit = ChatUnit::new(Uuid::new_v4());
        let mut child = unit.scoped(async { spawn_sleeper() }).await;
        let pid = child.id().unwrap();
        unit.adopt(Some(pid));
        // Reap the leader once it dies, as a bridge's wait task would.
        let reaper = tokio::spawn(async move { child.wait().await });
        assert_eq!(unit.finish().await, CleanupOutcome::Clean);
        let _ = tokio::time::timeout(Duration::from_secs(5), reaper).await;
        assert!(!alive(pid), "the chat agent outlived its session's unit");
    }

    /// A group the session cannot prove its own is never signalled, and
    /// the unit stays unresolved (the working copy is not handed over).
    #[tokio::test]
    async fn a_chat_unit_over_an_unproven_group_is_unresolved_and_signals_nothing() {
        let mut stranger = tokio::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = stranger.id().unwrap();
        let recorded = crate::util::procinfo::info(pid as i32).unwrap();
        // The recorded identity no longer matches: a stranger now holds it.
        let forged = crate::util::procinfo::ProcInfo {
            start: recorded.start.wrapping_add(1),
            ..recorded
        };
        let mut unit = ChatUnit::from_group(RunGroup::claimed_for_test(&forged));
        assert_eq!(unit.finish().await, CleanupOutcome::Unresolved);
        assert!(alive(pid), "an unproven group was signalled");
        let _ = stranger.kill().await;
    }
}
