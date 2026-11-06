//! Local chat on the daemon's own engine profile (design §12.1, PO-8).
//!
//! Local chat used to borrow a project runner: it failed once the daemon
//! hosted two (PIDESKAPP-41), forced chat to enrol a project (PIDESKAPP-14)
//! and was blocked for the length of a run (PIDESKAPP-101). The daemon now
//! owns a machine-level engine profile for it:
//!
//! - the engine binary resolved at daemon start (`engine::locate`);
//! - the account's engine `CODEX_HOME`, written from the model profile by
//!   the engine manager (`engine::agent::launch`);
//! - the model token from the daemon's engine credential, which the engine
//!   asks for through its auth helper (`pidash __engine model-token`).
//!
//! A `Chat*` request with no `runner` selector and an absolute `cwd` runs
//! here, with its own session registry and approval router: no project
//! runner, no shared busy flag. The folder is the caller's (the app's chat
//! folder, PIDESKAPP-42) and is refused when it is inside any runner's
//! working dir, so a chat can never edit a task folder under a run.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use uuid::Uuid;

use super::chat::ChatRegistry;
use crate::agent::AgentBridge;
use crate::approval::router::ApprovalRouter;
use crate::cloud::protocol::ApprovalMode;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, RunnerConfig, WorkspaceSection,
    canonical_for_compare,
};

/// Name the profile reports in approvals and logs. Never a configured
/// runner's name: the server names engine runners `builtin-<machine>-…`.
pub const PROFILE_NAME: &str = "builtin-engine-local-chat";

/// Where the profile's engine comes from.
enum Launch {
    /// The daemon's resolved engine and published `CODEX_HOME`.
    Daemon,
    /// A fixed binary and home (tests).
    #[cfg(test)]
    Fixed {
        binary: PathBuf,
        codex_home: PathBuf,
    },
}

/// The machine-level engine profile local chat runs on.
pub struct EngineChatProfile {
    pub(crate) sessions: ChatRegistry,
    pub(crate) approvals: ApprovalRouter,
    config: RunnerConfig,
    launch: Launch,
}

impl EngineChatProfile {
    /// The profile of a daemon whose dev machine is `machine_id` (the id
    /// approvals are recorded under; nil before the first sign-in).
    pub fn new(machine_id: Option<Uuid>) -> Self {
        Self::with_launch(machine_id, Launch::Daemon)
    }

    /// A profile that launches `binary` with `codex_home` (tests).
    #[cfg(test)]
    pub(crate) fn for_tests(binary: PathBuf, codex_home: PathBuf) -> Self {
        Self::with_launch(None, Launch::Fixed { binary, codex_home })
    }

    fn with_launch(machine_id: Option<Uuid>, launch: Launch) -> Self {
        let config = RunnerConfig {
            name: PROFILE_NAME.to_string(),
            runner_id: machine_id.unwrap_or_else(Uuid::nil),
            workspace_slug: None,
            project_slug: None,
            pod_id: None,
            // Never used: every profile chat names its own absolute folder.
            workspace: WorkspaceSection {
                working_dir: PathBuf::new(),
            },
            agent: AgentSection {
                kind: AgentKind::Engine,
            },
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: ApprovalPolicySection::default(),
        };
        Self {
            sessions: ChatRegistry::new(),
            approvals: ApprovalRouter::new(),
            config,
            launch,
        }
    }

    /// The profile's settings (approval policy, the id approvals carry).
    pub(crate) fn config(&self) -> &RunnerConfig {
        &self.config
    }

    /// The Codex launch settings, or why the engine cannot start (not
    /// installed, no model profile yet — the reason code leads the message).
    fn prepared(&self) -> Result<RunnerConfig> {
        match &self.launch {
            Launch::Daemon => crate::engine::agent::prepare(&self.config),
            #[cfg(test)]
            Launch::Fixed { binary, codex_home } => Ok(crate::engine::agent::engine_runner_config(
                &self.config,
                binary,
                codex_home,
                None,
            )),
        }
    }

    /// Start the engine for one chat session in `cwd`. The model is the
    /// profile's (`CODEX_HOME/config.toml`), as for an engine runner.
    pub(crate) async fn spawn(&self, cwd: &Path, mode: ApprovalMode) -> Result<AgentBridge> {
        let prepared = self.prepared()?;
        let b = crate::codex::bridge::Bridge::spawn_with_env(
            &prepared.codex.binary,
            cwd,
            None,
            None,
            crate::approval::engine_thread_settings(mode),
            &crate::agent::agent_env_for_config(&prepared),
        )
        .await?;
        Ok(AgentBridge::Codex(b))
    }
}

/// The folder a profile chat runs in: `cwd` must be absolute, free of
/// `..`, an existing directory, and not inside (or equal to) any runner's
/// working dir. Returned canonical, so a symlink cannot smuggle a task
/// folder past the check.
pub(crate) fn resolve_engine_chat_folder(
    cwd: Option<&str>,
    runner_dirs: &[(String, PathBuf)],
) -> Result<PathBuf> {
    let Some(raw) = cwd.filter(|s| !s.is_empty()) else {
        anyhow::bail!("chat on the built-in engine needs an absolute chat folder (`cwd`)");
    };
    let requested = PathBuf::from(raw);
    if !requested.is_absolute() {
        anyhow::bail!("chat folder must be an absolute path, got {raw}");
    }
    if requested
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        anyhow::bail!("chat folder must not contain `..`");
    }
    if !requested.is_dir() {
        anyhow::bail!(
            "chat folder {} does not exist (the caller owns this directory and must create it)",
            requested.display()
        );
    }
    let folder = std::fs::canonicalize(&requested)?;
    for (runner, dir) in runner_dirs {
        let dir = canonical_for_compare(dir);
        if folder.starts_with(&dir) {
            anyhow::bail!(
                "chat folder {} is inside runner {runner:?}'s working dir ({}); chat on the built-in engine must use a folder outside every task folder",
                folder.display(),
                dir.display()
            );
        }
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_folder_must_be_absolute_existing_and_outside_every_runner() {
        let tmp = tempfile::tempdir().unwrap();
        let task = tmp.path().join("tasks/acme/WEB");
        let chat = tmp.path().join("chat");
        std::fs::create_dir_all(task.join("src")).unwrap();
        std::fs::create_dir_all(&chat).unwrap();
        let dirs = vec![("builtin-h-acme-web".to_string(), task.clone())];

        let ok = resolve_engine_chat_folder(Some(chat.to_str().unwrap()), &dirs).unwrap();
        assert_eq!(ok, std::fs::canonicalize(&chat).unwrap());
        // A folder that contains task folders is fine; only inside is not.
        assert!(resolve_engine_chat_folder(Some(tmp.path().to_str().unwrap()), &dirs).is_ok());

        for inside in [task.clone(), task.join("src")] {
            let err = resolve_engine_chat_folder(Some(inside.to_str().unwrap()), &dirs)
                .unwrap_err()
                .to_string();
            assert!(err.contains("inside runner"), "{err}");
        }
        assert!(resolve_engine_chat_folder(None, &dirs).is_err());
        assert!(resolve_engine_chat_folder(Some("relative/chat"), &dirs).is_err());
        assert!(
            resolve_engine_chat_folder(Some(&format!("{}/../chat", task.display())), &dirs)
                .is_err()
        );
        assert!(
            resolve_engine_chat_folder(Some(&format!("{}/missing", chat.display())), &dirs)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_a_task_folder_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let task = tmp.path().join("task");
        std::fs::create_dir_all(&task).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&task, &link).unwrap();
        let dirs = vec![("r".to_string(), task)];
        assert!(resolve_engine_chat_folder(Some(link.to_str().unwrap()), &dirs).is_err());
    }

    #[test]
    fn the_profile_is_the_engine_with_no_project() {
        let p = EngineChatProfile::new(None);
        assert_eq!(p.config().agent.kind, AgentKind::Engine);
        assert!(p.config().project_slug.is_none());
        assert_eq!(p.config().runner_id, Uuid::nil());
    }
}
