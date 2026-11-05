//! The daemon side of the engine IPC requests (PIDESKAPP-121): turning the
//! engine on and off for a project, and the engine settings. Built on the
//! in-process hot-add and removal of PIDESKAPP-122, so no other runner is
//! restarted.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::schema::AgentKind;
use crate::daemon::supervisor::RunnerSpawnCtx;
use crate::engine::provision::{AddEngineRunner, add_engine_runner};
use crate::engine::workdir;
use crate::ipc::protocol::{EngineRunnerInfo, EngineSettings};

impl RunnerSpawnCtx {
    /// `EngineRunnerAdd`: enrol, write the block, start it in-process.
    pub(crate) async fn engine_runner_add(
        &self,
        workspace: String,
        project: String,
        working_dir: Option<PathBuf>,
    ) -> Result<EngineRunnerInfo> {
        let _engine_ops = self.engine_ops_lock.lock().await;
        let status = self.engine.status();
        let req = AddEngineRunner {
            workspace,
            project,
            working_dir,
        };
        let (runner, info) = add_engine_runner(&self.paths, &req, &status).await?;
        // A workspace signed in after the daemon started needs its token.
        if let Ok(cfg) = crate::config::file::load_config(&self.paths) {
            self.tokens.reload(cfg.cli.as_ref());
        }
        self.add_runner(runner)
            .await
            .context("starting the engine runner in the running daemon")?;
        Ok(info)
    }

    /// The token and id `EngineRunnerRemove` revokes `runner` with.
    pub(crate) fn engine_runner_for_removal(&self, runner: &str) -> Result<(uuid::Uuid, String)> {
        let cfg = crate::config::file::load_config(&self.paths)?;
        let block = cfg
            .runners
            .iter()
            .find(|r| r.name == runner)
            .ok_or_else(|| anyhow::anyhow!("no configured runner named {runner:?}"))?;
        if block.agent.kind != AgentKind::Engine {
            anyhow::bail!(
                "runner {runner:?} is not a built-in engine runner; use `pidash runner remove {runner}`"
            );
        }
        let token = self
            .tokens
            .token_for(block.workspace_slug.as_deref())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "workspace {:?} is not signed in on this computer",
                    block.workspace_slug.as_deref().unwrap_or("(default)")
                )
            })?;
        Ok((block.runner_id, token))
    }

    /// `EngineSetPaused`: set the owner's pause switch on the server with a
    /// live daemon credential, cache the answer and report the new state.
    pub(crate) async fn engine_set_paused(
        &self,
        paused: bool,
    ) -> Result<crate::ipc::protocol::EngineStatusInfo> {
        let machine_id = self
            .dev_machine_id()
            .context("this computer has no dev-machine id yet; run `pidash auth login`")?;
        let (_, token) = self
            .tokens
            .session_candidates()
            .into_iter()
            .next()
            .context("this computer holds no daemon credential; run `pidash auth login`")?;
        self.engine
            .set_paused(&token, machine_id, paused)
            .await
            .map_err(|e| anyhow::anyhow!("the server did not change the engine pause: {e}"))
    }

    /// `EngineSettingsGet`.
    pub(crate) fn engine_settings(&self) -> Result<EngineSettings> {
        let cfg = crate::config::file::load_config(&self.paths)?;
        Ok(EngineSettings {
            task_root: workdir::task_root(&self.paths, &cfg),
            task_root_is_default: cfg.engine.task_root.is_none(),
            warnings: Vec::new(),
        })
    }

    /// `EngineSettingsSet`: validate the root (design §10) and store it.
    /// Nothing is moved: runners already set up keep their folder.
    pub(crate) async fn set_engine_settings(
        &self,
        task_root: Option<PathBuf>,
    ) -> Result<EngineSettings> {
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || set_task_root(&paths, task_root))
            .await
            .map_err(|e| anyhow::anyhow!("settings task failed: {e}"))?
    }
}

/// Validate and store `[engine] task_root` (`None` = the default). Shared
/// with `pidash config set engine-task-root` when no daemon runs.
pub fn set_task_root(
    paths: &crate::util::paths::Paths,
    task_root: Option<PathBuf>,
) -> Result<EngineSettings> {
    let cfg = crate::config::file::load_config(paths)?;
    let candidate = task_root
        .clone()
        .unwrap_or_else(|| workdir::default_task_root(paths));
    let valid = workdir::validate_task_root(paths, &cfg, &candidate)
        .map_err(|e| anyhow::anyhow!("{e}; the task root was not changed"))?;
    let stored = task_root.map(|_| valid.path.clone());
    crate::config::file::mutate_config(paths, |cfg| {
        cfg.engine.task_root = stored.clone();
        Ok(())
    })?;
    Ok(EngineSettings {
        task_root: valid.path,
        task_root_is_default: stored.is_none(),
        warnings: valid.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Config, DaemonConfig};

    #[test]
    fn task_root_is_validated_before_it_is_stored() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        crate::config::file::write_config(
            &paths,
            &Config {
                version: 2,
                daemon: DaemonConfig::default(),
                runners: vec![],
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            },
        )
        .unwrap();
        let err = set_task_root(&paths, Some(paths.config_dir.join("x"))).unwrap_err();
        assert!(err.to_string().contains("not changed"), "{err}");
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .engine
                .task_root
                .is_none()
        );
        let root = tmp.path().join("tasks");
        let s = set_task_root(&paths, Some(root.clone())).unwrap();
        assert!(!s.task_root_is_default);
        let stored = crate::config::file::load_config(&paths)
            .unwrap()
            .engine
            .task_root
            .unwrap();
        assert_eq!(stored, std::fs::canonicalize(&root).unwrap());
        let s = set_task_root(&paths, None).unwrap();
        assert!(s.task_root_is_default);
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .engine
                .task_root
                .is_none()
        );
    }
}
