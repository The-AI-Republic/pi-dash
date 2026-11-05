//! Turning the built-in engine on for a project (design §5 "enrol on use",
//! §10 allocation). Shared by the daemon's `EngineRunnerAdd` and by
//! `pidash runner add --agent engine` when no daemon runs.
//!
//! 1. The local cap (`MAX_RUNNERS_PER_DAEMON`) is checked before the server
//!    is called, so it can never leave a half-enrolled runner.
//! 2. The engine must be able to run: signed in, and an instance that can
//!    serve it. Otherwise the add is refused with the reason.
//! 3. The server creates the engine runner (`agent_kind = "engine"`) with
//!    the workspace's daemon credential and names it.
//! 4. The folder is the caller's, or allocated as
//!    `<task root>/<workspace>/<project>/` (numeric suffix on a clash).
//! 5. The block is written under the config lock and validated; any local
//!    failure rolls the server row back.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::api::reason;
use crate::cli::runner_ops::{ApplyEnrollOptions, apply_enroll_response};
use crate::cloud::http::{CreateRunnerRequest, SharedHttpTransport, create_runner};
use crate::config::file;
use crate::config::schema::{AgentKind, MAX_RUNNERS_PER_DAEMON, RunnerConfig};
use crate::ipc::protocol::{EngineRunnerInfo, EngineState, EngineStatusInfo};
use crate::util::paths::Paths;

/// What to turn the engine on for.
#[derive(Debug, Clone)]
pub struct AddEngineRunner {
    pub workspace: String,
    pub project: String,
    pub working_dir: Option<PathBuf>,
}

/// Refuse when the engine cannot run on this machine, with the reason.
pub fn ensure_engine_can_run(status: &EngineStatusInfo) -> Result<()> {
    let reason = status.reason_code.as_deref().unwrap_or("unknown");
    match status.state {
        EngineState::Available | EngineState::Expiring => Ok(()),
        EngineState::SignedOut => anyhow::bail!(
            "{reason}: the built-in engine is not signed in on this computer; run `pidash login --engine` first"
        ),
        // A transient state (the profile not fetched yet, an empty wallet)
        // does not block turning the engine on: the server decides.
        EngineState::Unavailable
            if matches!(
                reason,
                reason::PROFILE_PENDING | reason::WALLET_EMPTY | reason::GATEWAY_SESSION_REVOKED
            ) =>
        {
            Ok(())
        }
        EngineState::Unavailable => anyhow::bail!(
            "{reason}: the built-in engine is not available on this computer{}",
            if reason == reason::ENGINE_NOT_INSTALLED {
                " (run `pidash engine install`)"
            } else {
                ""
            }
        ),
    }
}

/// Enrol an engine runner and write its block. Does not start it: the
/// daemon hot-adds the returned block, the CLI starts the service.
pub async fn add_engine_runner(
    paths: &Paths,
    req: &AddEngineRunner,
    status: &EngineStatusInfo,
) -> Result<(RunnerConfig, EngineRunnerInfo)> {
    let workspace = req.workspace.trim();
    let project = req.project.trim();
    if workspace.is_empty() || project.is_empty() {
        anyhow::bail!("a workspace and a project are required");
    }
    if let Some(dir) = &req.working_dir
        && !dir.is_absolute()
    {
        anyhow::bail!(
            "working dir must be an absolute path, got {}",
            dir.display()
        );
    }
    let cfg = file::load_config(paths)?;
    if cfg.runners.len() >= MAX_RUNNERS_PER_DAEMON {
        anyhow::bail!(
            "this computer already hosts {MAX_RUNNERS_PER_DAEMON} runners, the most one daemon runs; remove one first"
        );
    }
    // One engine runner per machine and project (design §5).
    if let Some(existing) = cfg.runners.iter().find(|r| {
        r.agent.kind == AgentKind::Engine
            && r.workspace_slug.as_deref() == Some(workspace)
            && r.project_slug
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(project))
    }) {
        anyhow::bail!(
            "the built-in engine is already on for {project} in {workspace} (runner {})",
            existing.name
        );
    }
    ensure_engine_can_run(status)?;
    let token = cfg
        .cli
        .as_ref()
        .and_then(|c| c.token_for_workspace(Some(workspace)))
        .filter(|t| t.starts_with("mt_"))
        .map(str::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workspace {workspace:?} is not signed in on this computer; run `pidash auth login --workspace {workspace}`"
            )
        })?;
    let dev_machine_id = cfg
        .daemon
        .dev_machine_id
        .context("this computer has no dev-machine id yet; run `pidash auth login`")?;
    let cloud_url = cfg.daemon.cloud_url.clone();
    let transport = SharedHttpTransport::new(cloud_url.clone())?;
    let host_label = crate::util::hostname::default_hostname();
    let resp = create_runner(
        &transport,
        CreateRunnerRequest {
            api_token: &token,
            dev_machine_id: &dev_machine_id,
            workspace_slug: Some(workspace),
            project,
            host_label: &host_label,
            // The server names engine runners (design §5 "Naming").
            name: None,
            pod: None,
            agent_kind: Some(super::api::ENGINE_AGENT_KIND),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("the server refused to turn the engine on: {e}"))?;

    let local = async {
        let project_id = if resp.project_identifier.is_empty() {
            project.to_string()
        } else {
            resp.project_identifier.clone()
        };
        let ws = if resp.workspace_slug.is_empty() {
            workspace.to_string()
        } else {
            resp.workspace_slug.clone()
        };
        let working_dir = match &req.working_dir {
            Some(dir) => dir.clone(),
            None => super::workdir::allocate_engine_folder(paths, &ws, &project_id)?,
        };

        std::fs::create_dir_all(&working_dir)
            .with_context(|| format!("creating {}", working_dir.display()))?;
        let applied = apply_enroll_response(
            paths,
            &resp,
            &cloud_url,
            ApplyEnrollOptions {
                working_dir: Some(working_dir.clone()),
                agent_kind: AgentKind::Engine,
                model: None,
                reasoning_effort: None,
            },
        )
        .await?;
        anyhow::Ok((applied.runner, ws, project_id, working_dir))
    };
    let local: Result<(RunnerConfig, String, String, PathBuf)> = local.await;
    match local {
        Ok((runner, ws, project_id, working_dir)) => {
            let info = EngineRunnerInfo {
                runner: runner.name.clone(),
                runner_id: runner.runner_id,
                workspace: ws,
                project: project_id,
                working_dir,
            };
            Ok((runner, info))
        }
        Err(e) => {
            let _ = file::mutate_config(paths, |cfg| {
                cfg.runners.retain(|r| r.runner_id != resp.runner_id);
                Ok(())
            });
            if let Err(rollback) =
                crate::cloud::runners::delete_runner(&cloud_url, &token, &resp.runner_id, false)
                    .await
            {
                tracing::warn!(
                    runner_id = %resp.runner_id,
                    "rolling back the engine runner after a local failure also failed: {rollback:#}"
                );
            }
            Err(e.context("setting up the engine runner locally"))
        }
    }
}

/// The engine status when no daemon is running (`pidash runner add --agent
/// engine` on a stopped daemon): the local credential and the server's
/// profile, fetched now.
pub async fn status_without_daemon(paths: &Paths) -> Result<EngineStatusInfo> {
    let cfg = file::load_config(paths)?;
    let tokens = crate::daemon::workspace_tokens::WorkspaceTokens::from_cli(cfg.cli.as_ref());
    let binary = super::locate::resolve(&cfg.engine).ok().map(|e| e.binary);
    let session_token = tokens
        .session_candidates()
        .into_iter()
        .next()
        .map(|(_, t)| t);
    let manager = super::manager::EngineManager::new(
        Some(&cfg.daemon.cloud_url),
        paths.clone(),
        tokens,
        binary,
    );
    manager.refresh_profile().await;
    // The pause state is the server's; read it now (best-effort).
    if let (Some(token), Some(machine_id)) = (session_token, cfg.daemon.dev_machine_id)
        && let Err(e) = manager.refresh_machine(&token, machine_id).await
    {
        tracing::debug!("reading the engine pause state failed: {e}");
    }
    Ok(manager.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{CliSection, Config, DaemonConfig, EngineCredential};
    use crate::engine::test_server::{self, Handler};
    use crate::ipc::protocol::EngineSignInState;
    use std::sync::Arc;

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn available() -> EngineStatusInfo {
        EngineStatusInfo {
            state: EngineState::Available,
            reason_code: None,
            sign_in_state: EngineSignInState::Active,
            credential_expires_at: None,
            model: None,
            binary: None,
            profile_fetched_at: None,
            paused_by_owner: None,
            auto_pause_reason: None,
            pause_checked_at: None,
        }
    }

    fn seed(paths: &Paths, url: &str) {
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: url.into(),
                dev_machine_id: Some(uuid::Uuid::new_v4()),
                ..DaemonConfig::default()
            },
            runners: vec![],
            cli: Some(CliSection {
                token: Some("mt_acme".into()),
                workspace_slug: Some("acme".into()),
                engine_credential: Some(EngineCredential {
                    token: "ec_x".into(),
                    expires_at: None,
                }),
                ..Default::default()
            }),
            engine: Default::default(),
            auth: Default::default(),
        };
        file::write_config(paths, &cfg).unwrap();
    }

    fn enrol_handler() -> Handler {
        Arc::new(|req| {
            if req.path == "/api/v1/runner/runners/" {
                let body = req.json();
                let n = body["project"].as_str().unwrap().to_string();
                let resp = serde_json::json!({
                    "runner_id": uuid::Uuid::new_v4(),
                    "runner_name": format!("builtin-host-acme-{}", n.to_lowercase()),
                    "refresh_token": "", "access_token": "", "access_token_expires_at": "",
                    "refresh_token_generation": 0, "workspace_slug": "acme",
                    "project_identifier": n, "long_poll_interval_secs": 25,
                    "protocol_version": 4
                });
                (201, resp.to_string())
            } else {
                (200, "{}".into())
            }
        })
    }

    #[tokio::test]
    async fn turning_the_engine_on_enrols_and_allocates_the_folder() {
        let server = test_server::start(enrol_handler()).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);

        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let (runner, info) = add_engine_runner(&paths, &req, &available()).await.unwrap();
        assert_eq!(runner.agent.kind, AgentKind::Engine);
        assert_eq!(
            info.working_dir,
            paths.data_dir.join("engine/workdirs/acme/WEB")
        );
        assert!(info.working_dir.is_dir());
        let sent = &server.requests_to("/api/v1/runner/runners/")[0];
        assert_eq!(sent.json()["agent_kind"], "engine");
        assert!(
            sent.json().get("name").is_none(),
            "the server names engines"
        );
        assert_eq!(
            sent.headers.get("x-api-key").map(String::as_str),
            Some("mt_acme")
        );
        // The block holds no engine paths.
        let cfg = file::load_config(&paths).unwrap();
        let block = cfg.runners.iter().find(|r| r.name == info.runner).unwrap();
        assert!(block.codex.codex_home.is_none());

        // Same project again: refused, nothing sent.
        let err = add_engine_runner(&paths, &req, &available())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already on"), "{err}");
        assert_eq!(server.requests_to("/api/v1/runner/runners/").len(), 1);

        // A custom root is used for the next project.
        file::mutate_config(&paths, |cfg| {
            cfg.engine.task_root = Some(tmp.path().join("tasks"));
            Ok(())
        })
        .unwrap();
        let (_, info) = add_engine_runner(
            &paths,
            &AddEngineRunner {
                workspace: "acme".into(),
                project: "API".into(),
                working_dir: None,
            },
            &available(),
        )
        .await
        .unwrap();
        assert_eq!(info.working_dir, tmp.path().join("tasks/acme/API"));
        // The first project's folder did not move.
        assert!(paths.data_dir.join("engine/workdirs/acme/WEB").is_dir());
    }

    #[tokio::test]
    async fn refused_when_the_engine_cannot_run_or_the_workspace_is_signed_out() {
        let server = test_server::start(enrol_handler()).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let mut signed_out = available();
        signed_out.state = EngineState::SignedOut;
        signed_out.reason_code = Some(reason::ENGINE_SIGN_IN_REQUIRED.into());
        let err = add_engine_runner(&paths, &req, &signed_out)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("engine_sign_in_required"), "{err}");
        let mut disabled = available();
        disabled.state = EngineState::Unavailable;
        disabled.reason_code = Some(reason::ENGINE_DISABLED.into());
        let err = add_engine_runner(&paths, &req, &disabled)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("engine_disabled"), "{err}");
        // An unsigned-in workspace.
        let other = AddEngineRunner {
            workspace: "beta".into(),
            ..req.clone()
        };
        let err = add_engine_runner(&paths, &other, &available())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not signed in"), "{err}");
        assert!(server.requests_to("/api/v1/runner/runners/").is_empty());
    }
}
