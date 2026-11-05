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
    // Read before the create, so a local failure below rolls back only a
    // runner this call made (see `created_by_this_call`).
    let before: std::collections::HashSet<uuid::Uuid> =
        cfg.runners.iter().map(|r| r.runner_id).collect();
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
        // The server returned a runner this computer already has (a
        // repeated or concurrent add): it is the answer, folder and all.
        // Nothing is allocated or rewritten.
        let cfg = file::load_config(paths)?;
        if let Some(existing) = cfg.runners.iter().find(|r| r.runner_id == resp.runner_id) {
            let dir = existing.workspace.working_dir.clone();
            return anyhow::Ok((existing.clone(), ws, project_id, dir));
        }
        let working_dir = match &req.working_dir {
            Some(dir) => dir.clone(),
            None => {
                let root = super::workdir::task_root(paths, &cfg);
                super::workdir::allocate_working_dir(&root, &ws, &project_id, &cfg)?
            }
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
        // The runner was already on this computer, or the server returned
        // one it already had (with no block here after a local sign-out):
        // it is a working runner, so leave its block and its server row
        // alone. Only a runner this call made is rolled back.
        Err(e) if !crate::cli::runner_ops::may_roll_back_create(&e, &resp, &before) => {
            Err(e.context("setting up the engine runner locally"))
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
    let manager = super::manager::EngineManager::new(
        Some(&cfg.daemon.cloud_url),
        paths.clone(),
        tokens,
        binary,
    );
    manager.refresh_profile().await;
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

    /// Like the server: one engine runner per machine and project, so a
    /// second create for the same project returns the same `runner_id`.
    fn enrol_handler() -> Handler {
        let ids: Arc<std::sync::Mutex<std::collections::HashMap<String, uuid::Uuid>>> =
            Arc::default();
        Arc::new(move |req| {
            if req.path == "/api/v1/runner/runners/" {
                let body = req.json();
                let n = body["project"].as_str().unwrap().to_string();
                let mut ids = ids.lock().unwrap();
                let created = !ids.contains_key(&n.to_lowercase());
                let id = *ids
                    .entry(n.to_lowercase())
                    .or_insert_with(uuid::Uuid::new_v4);
                let resp = serde_json::json!({
                    "runner_id": id,
                    "runner_name": format!("builtin-host-acme-{}", n.to_lowercase()),
                    "refresh_token": "", "access_token": "", "access_token_expires_at": "",
                    "refresh_token_generation": 0, "workspace_slug": "acme",
                    "project_identifier": n, "long_poll_interval_secs": 25,
                    "protocol_version": 4, "created": created
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

    #[tokio::test]
    async fn two_adds_for_one_project_at_once_leave_one_engine_runner() {
        let server = test_server::start(enrol_handler()).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let status = available();
        let (a, b) = tokio::join!(
            add_engine_runner(&paths, &req, &status),
            add_engine_runner(&paths, &req, &status)
        );
        // Both passed the early check and enrolled. The server answers the
        // second create with the same runner, so both adds succeed and
        // name the one runner.
        assert_eq!(server.requests_to("/api/v1/runner/runners/").len(), 2);
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.0.runner_id, b.0.runner_id);
        let cfg = file::load_config(&paths).unwrap();
        let engines: Vec<_> = cfg
            .runners
            .iter()
            .filter(|r| r.agent.kind == AgentKind::Engine)
            .collect();
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].runner_id, a.0.runner_id);
        // Nothing was rolled back: no delete reached the server.
        let deletes: Vec<_> = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "DELETE")
            .cloned()
            .collect();
        assert!(
            deletes.is_empty(),
            "the working runner was deleted: {deletes:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_repeat_add_never_deletes_the_runner_that_is_there() {
        // The server returns the existing runner, and the per-runner
        // credentials can't be written. The block and the server row of
        // the working runner must stay.
        let id = uuid::Uuid::new_v4();
        let server = test_server::start(Arc::new(move |req| {
            if req.path == "/api/v1/runner/runners/" {
                let resp = serde_json::json!({
                    "runner_id": id,
                    "runner_name": "builtin-host-acme-web",
                    "refresh_token": "rt_x", "access_token": "", "access_token_expires_at": "",
                    "refresh_token_generation": 1, "workspace_slug": "acme",
                    "project_identifier": "WEB", "long_poll_interval_secs": 25,
                    "protocol_version": 4
                });
                (201, resp.to_string())
            } else {
                (200, "{}".into())
            }
        }))
        .await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let (first, _) = add_engine_runner(&paths, &req, &available()).await.unwrap();
        // Rename the project slug so the early per-project check misses it
        // (as a block written by an older build would), and put a folder
        // where the per-runner credentials go so the local half fails
        // after the server call.
        file::mutate_config(&paths, |cfg| {
            cfg.runners[0].project_slug = Some("web-old".into());
            Ok(())
        })
        .unwrap();
        let resp_id = first.runner_id;
        let creds = paths.for_runner(resp_id).credentials_path();
        let _ = std::fs::remove_file(&creds);
        std::fs::create_dir_all(&creds).unwrap();
        // The repeat finds the runner in the config and returns it without
        // rewriting anything, so the broken credentials path is not touched.
        let (again, _) = add_engine_runner(&paths, &req, &available()).await.unwrap();
        assert_eq!(again.runner_id, resp_id);
        let cfg = file::load_config(&paths).unwrap();
        assert!(
            cfg.runners.iter().any(|r| r.runner_id == resp_id),
            "the existing block was stripped"
        );
        let deletes = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "DELETE")
            .count();
        assert_eq!(deletes, 0, "the existing server row was deleted");
    }

    fn fixed_id_handler(id: uuid::Uuid, created: Option<bool>) -> Handler {
        Arc::new(move |req| {
            if req.path == "/api/v1/runner/runners/" {
                let mut resp = serde_json::json!({
                    "runner_id": id,
                    "runner_name": "builtin-host-acme-web",
                    "refresh_token": "", "access_token": "", "access_token_expires_at": "",
                    "refresh_token_generation": 0, "workspace_slug": "acme",
                    "project_identifier": "WEB", "long_poll_interval_secs": 25,
                    "protocol_version": 4
                });
                if let Some(c) = created {
                    resp["created"] = serde_json::json!(c);
                }
                (201, resp.to_string())
            } else {
                (200, "{}".into())
            }
        })
    }

    fn deletes(server: &test_server::MockServer) -> usize {
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "DELETE")
            .count()
    }

    /// A working dir that `create_dir_all` cannot make: under a file.
    fn uncreatable_dir(root: &std::path::Path) -> PathBuf {
        let file = root.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        file.join("work")
    }

    #[tokio::test]
    async fn an_existing_server_runner_is_never_deleted_on_an_early_local_failure() {
        // The server answers with a runner it already had (after a local
        // sign-out it has no block here), and the local half fails before
        // the block is written: creating the folder fails.
        let server = test_server::start(fixed_id_handler(uuid::Uuid::new_v4(), Some(false))).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: Some(uncreatable_dir(tmp.path())),
        };
        add_engine_runner(&paths, &req, &available())
            .await
            .expect_err("creating the folder must fail");
        assert_eq!(deletes(&server), 0, "the existing server row was deleted");
    }

    #[tokio::test]
    async fn a_runner_this_call_created_is_rolled_back_on_a_local_failure() {
        for created in [Some(true), None] {
            let id = uuid::Uuid::new_v4();
            let server = test_server::start(fixed_id_handler(id, created)).await;
            let tmp = tempfile::tempdir().unwrap();
            let paths = paths(tmp.path());
            seed(&paths, &server.url);
            let req = AddEngineRunner {
                workspace: "acme".into(),
                project: "WEB".into(),
                working_dir: Some(uncreatable_dir(tmp.path())),
            };
            add_engine_runner(&paths, &req, &available())
                .await
                .expect_err("creating the folder must fail");
            assert_eq!(deletes(&server), 1, "created = {created:?}: no rollback");
            let cfg = file::load_config(&paths).unwrap();
            assert!(!cfg.runners.iter().any(|r| r.runner_id == id));
        }
    }

    #[tokio::test]
    async fn a_same_id_create_for_a_runner_in_the_config_is_never_rolled_back() {
        // A server without the `created` field returns an id the config
        // already holds, and a later local step would fail: the runner in
        // the config is returned as it is, nothing is deleted.
        let id = uuid::Uuid::new_v4();
        let server = test_server::start(fixed_id_handler(id, None)).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let ok = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let (first, _) = add_engine_runner(&paths, &ok, &available()).await.unwrap();
        // Hide it from the early per-project check, as a block from an older
        // build with another project slug would be.
        file::mutate_config(&paths, |cfg| {
            cfg.runners[0].project_slug = Some("web-old".into());
            Ok(())
        })
        .unwrap();
        let failing = AddEngineRunner {
            working_dir: Some(uncreatable_dir(tmp.path())),
            ..ok
        };
        let (again, info) = add_engine_runner(&paths, &failing, &available())
            .await
            .unwrap();
        assert_eq!(again.runner_id, first.runner_id);
        assert_eq!(info.working_dir, first.workspace.working_dir);
        assert_eq!(deletes(&server), 0);
    }

    #[tokio::test]
    async fn a_repeated_add_reuses_the_folder_and_allocates_no_other() {
        let server = test_server::start(enrol_handler()).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        seed(&paths, &server.url);
        let req = AddEngineRunner {
            workspace: "acme".into(),
            project: "WEB".into(),
            working_dir: None,
        };
        let (first, _) = add_engine_runner(&paths, &req, &available()).await.unwrap();
        file::mutate_config(&paths, |cfg| {
            cfg.runners[0].project_slug = Some("web-old".into());
            Ok(())
        })
        .unwrap();
        let (again, info) = add_engine_runner(&paths, &req, &available()).await.unwrap();
        assert_eq!(again.runner_id, first.runner_id);
        assert_eq!(info.working_dir, first.workspace.working_dir);
        let parent = first.workspace.working_dir.parent().unwrap();
        let siblings: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            1,
            "an extra folder was allocated: {siblings:?}"
        );
    }
}
