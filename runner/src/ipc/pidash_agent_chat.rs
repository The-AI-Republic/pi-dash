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
//! here, on the daemon's shared engine app-server
//! ([`super::chat_server::EngineChatServer`], NEWPIDASH-333): one child
//! process, one thread per chat session. The folder is the caller's (the
//! app's chat folder, PIDESKAPP-42) and is refused when it is inside any
//! runner's working dir, so a chat can never edit a task folder under a run.

use anyhow::Result;
use std::path::{Component, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::approval::router::ApprovalRouter;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, RunnerConfig, WorkspaceSection,
    canonical_for_compare,
};
use crate::pidash_agent::chat_server::{
    AvailabilitySource, EngineChatServer, ServerLifecycle, spawn_production_child,
};
#[cfg(test)]
use crate::pidash_agent::chat_server::{ChatAvailability, SpawnFuture, SpawnSource};

/// Name the profile reports in approvals and logs. Never a configured
/// runner's name: the server names engine runners `builtin-<machine>-…`.
pub const PROFILE_NAME: &str = "builtin-engine-local-chat";

/// The machine-level engine profile local chat runs on.
pub struct PidashAgentChatProfile {
    pub(crate) server: EngineChatServer,
    pub(crate) approvals: ApprovalRouter,
    config: RunnerConfig,
}

impl PidashAgentChatProfile {
    /// The profile of a daemon whose dev machine is `machine_id` (the id
    /// approvals are recorded under; nil before the first sign-in).
    /// `availability` decides whether the shared child may run; the daemon
    /// wires it to the engine manager (a weak handle, so the profile never
    /// keeps the manager alive).
    pub fn new(machine_id: Option<Uuid>, availability: AvailabilitySource) -> Self {
        let config = base_config(machine_id);
        let server = EngineChatServer::new(spawn_production_child(config.clone()), availability);
        Self {
            server,
            approvals: ApprovalRouter::new(),
            config,
        }
    }

    /// The profile's settings (approval policy, the id approvals carry).
    pub(crate) fn config(&self) -> &RunnerConfig {
        &self.config
    }

    /// The shared engine app-server this profile's chats run on.
    pub(crate) fn server(&self) -> &EngineChatServer {
        &self.server
    }

    /// The server's lifecycle hooks for the engine manager.
    pub(crate) fn server_lifecycle(&self) -> Arc<dyn ServerLifecycle> {
        Arc::new(self.server.clone())
    }

    /// A profile that launches `binary` with `codex_home` (tests): the
    /// daemon-side of the engine IPC tests in `ipc::server`.
    #[cfg(test)]
    pub(crate) fn for_tests(binary: PathBuf, codex_home: PathBuf) -> Self {
        use crate::util::shell::AgentEnv;
        let spawn: SpawnSource = {
            let codex_home = codex_home.clone();
            Arc::new(move || {
                let (binary, codex_home) = (binary.clone(), codex_home.clone());
                Box::pin(async move {
                    crate::util::fs::create_dir_all_private(&codex_home)?;
                    let env = AgentEnv {
                        codex_home: Some(codex_home.clone()),
                        path_prepend: None,
                        config_dir: None,
                        data_dir: None,
                        model_token_file: None,
                    };
                    crate::codex::app_server::AppServer::spawn_with_env(
                        &binary.to_string_lossy(),
                        &codex_home,
                        &env,
                    )
                    .await
                }) as SpawnFuture
            })
        };
        let availability: AvailabilitySource = Arc::new(|| ChatAvailability {
            can_run: true,
            code: "ok".to_string(),
        });
        let server = EngineChatServer::with_backoff(
            spawn,
            availability,
            std::time::Duration::ZERO,
            std::time::Duration::ZERO,
        );
        Self {
            server,
            approvals: ApprovalRouter::new(),
            config: base_config(None),
        }
    }
}

fn base_config(machine_id: Option<Uuid>) -> RunnerConfig {
    RunnerConfig {
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
            kind: AgentKind::PidashAgent,
        },
        codex: Default::default(),
        claude_code: Default::default(),
        cursor_agent: Default::default(),
        openclaw: Default::default(),
        grok: Default::default(),
        muse_code: Default::default(),
        approval_policy: ApprovalPolicySection::default(),
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
        anyhow::bail!("chat on the Pi Dash Agent needs an absolute chat folder (`cwd`)");
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
                "chat folder {} is inside runner {runner:?}'s working dir ({}); chat on the Pi Dash Agent must use a folder outside every task folder",
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

    // The profile owns the shared server, whose background task needs a
    // runtime at construction.
    #[tokio::test]
    async fn the_profile_is_the_engine_with_no_project() {
        let p = PidashAgentChatProfile::new(
            None,
            Arc::new(|| ChatAvailability {
                can_run: true,
                code: "ok".to_string(),
            }),
        );
        assert_eq!(p.config().agent.kind, AgentKind::PidashAgent);
        assert!(p.config().project_slug.is_none());
        assert_eq!(p.config().runner_id, Uuid::nil());
    }
}
