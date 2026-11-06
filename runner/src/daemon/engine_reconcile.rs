//! Keeping the daemon's engine runner blocks in step with the server
//! (design §5 "Removal exists"), and removing old engine versions
//! (design §4.5).
//!
//! At daemon start and every [`RECONCILE_INTERVAL`] the daemon reads its
//! dev machine with each workspace's daemon credential
//! (`GET /api/v1/runner/dev-machines/<id>/`, [`MachinePayload`]):
//!
//! - an engine block whose server row is revoked ("Remove from project" on
//!   the web) is removed locally — its folder on disk is kept;
//! - a live engine runner row of this machine with no local block (a merged
//!   machine, a daemon whose config was restored from an old copy) is
//!   re-added, with a folder allocated as in design §10;
//!   A row the user removed on this computer only (`runner remove
//!   --local-only`, or offline) is tombstoned and not re-added
//!   ([`crate::engine::tombstones`]);
//! - the machine's engine pause state is cached for `Hello` and
//!   `pidash engine status`.
//!
//! The same tick removes engine version directories older than the one the
//! daemon resolved at start, once no run is in flight and no process runs a
//! binary from them.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::cli::runner_ops::{ApplyEnrollOptions, apply_enroll_response};
use crate::cloud::http::EnrollResponse;
use crate::config::schema::{AgentKind, MAX_RUNNERS_PER_DAEMON};
use crate::daemon::supervisor::RunnerSpawnCtx;
use crate::engine::api::MachineRunnerRow;

/// How often the daemon reconciles (design §5).
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// What one reconcile pass did, by runner name.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReconcileReport {
    pub removed: Vec<String>,
    pub added: Vec<String>,
    /// `runner: why` for every row that could not be handled.
    pub failed: Vec<String>,
}

/// The reconcile loop: at start, then every [`RECONCILE_INTERVAL`] until
/// shutdown.
pub(crate) async fn run(ctx: RunnerSpawnCtx, shutdown: Arc<Notify>) {
    let mut engines_cleaned = false;
    loop {
        let stop = shutdown.notified();
        tokio::pin!(stop);
        stop.as_mut().enable();
        let report = ctx.reconcile_engine_runners().await;
        if report != ReconcileReport::default() {
            tracing::info!(
                removed = ?report.removed,
                added = ?report.added,
                failed = ?report.failed,
                "engine runners reconciled with the server"
            );
        }
        if !engines_cleaned {
            engines_cleaned = ctx.cleanup_old_engine_versions();
        }
        tokio::select! {
            _ = &mut stop => return,
            _ = tokio::time::sleep(RECONCILE_INTERVAL) => {}
        }
    }
}

impl RunnerSpawnCtx {
    /// One reconcile pass over every signed-in workspace. A workspace whose
    /// machine read fails is left alone (nothing is removed on a guess).
    pub(crate) async fn reconcile_engine_runners(&self) -> ReconcileReport {
        let mut report = ReconcileReport::default();
        let Some(machine_id) = self.dev_machine_id() else {
            return report;
        };
        // Never race `EngineRunnerAdd` / machine control's create: a row
        // they just created has no block yet.
        let _engine_ops = self.engine_ops_lock.lock().await;
        // Live engine rows seen, the workspaces whose rows were read, and
        // whether every signed-in workspace was read. A tombstone is pruned
        // only after its own workspace was read (a signed-out workspace's
        // removed runners must not come back at its next sign-in).
        let mut live = BTreeSet::new();
        let mut read_workspaces = BTreeSet::new();
        let mut complete = true;
        let mut any_read = false;
        for (workspace, token) in self.tokens.session_candidates() {
            let payload = match self.engine.refresh_machine(&token, machine_id).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        workspace = workspace.as_deref().unwrap_or("(default)"),
                        "engine reconcile skipped: reading the machine failed: {e}"
                    );
                    complete = false;
                    continue;
                }
            };
            let Some(rows) = payload.runners else {
                complete = false;
                continue;
            };
            any_read = true;
            live.extend(
                rows.iter()
                    .filter(|r| r.is_engine() && !r.is_revoked())
                    .map(|r| r.id),
            );
            read_workspaces.extend(workspace.clone().filter(|w| !w.is_empty()));
            read_workspaces.extend(
                rows.iter()
                    .filter(|r| !r.workspace_slug.is_empty())
                    .map(|r| r.workspace_slug.clone()),
            );
            self.reconcile_rows(workspace.as_deref(), &rows, &mut report)
                .await;
        }
        // With no workspace read at all there is no view to prune from.
        let complete = complete && any_read;
        if let Err(e) =
            crate::engine::tombstones::prune(&self.paths, &read_workspaces, complete, &live)
        {
            tracing::warn!("pruning engine runner tombstones failed: {e:#}");
        }
        report
    }

    async fn reconcile_rows(
        &self,
        workspace: Option<&str>,
        rows: &[MachineRunnerRow],
        report: &mut ReconcileReport,
    ) {
        let cfg = match crate::config::file::load_config(&self.paths) {
            Ok(cfg) => cfg,
            Err(e) => {
                report.failed.push(format!("config: {e:#}"));
                return;
            }
        };
        // Read after the config: `runner remove --local-only` writes the
        // tombstone before it strips the block, so a block missing from
        // this config always has its tombstone in this read, even when
        // the removal landed during the machine GET.
        let tombstones = crate::engine::tombstones::load(&self.paths);
        for row in rows.iter().filter(|r| r.is_engine()) {
            let block = cfg.runners.iter().find(|b| b.runner_id == row.id);
            match block {
                Some(b) if row.is_revoked() && b.agent.kind == AgentKind::Engine => {
                    match self.remove_engine_block(row.id, &b.name).await {
                        Ok(()) => report.removed.push(b.name.clone()),
                        Err(e) => report.failed.push(format!("{}: {e:#}", b.name)),
                    }
                }
                // Removed on this computer only (`runner remove
                // --local-only`, or offline): the user doesn't want it here.
                None if !row.is_revoked() && !tombstones.contains(&row.id) => {
                    let label = if row.name.is_empty() {
                        row.id.to_string()
                    } else {
                        row.name.clone()
                    };
                    match self.readd_engine_block(workspace, row).await {
                        Ok(name) => report.added.push(name),
                        Err(e) => report.failed.push(format!("{label}: {e:#}")),
                    }
                }
                _ => {}
            }
        }
    }

    /// Remove the engine block `runner_id` locally: through the runner's
    /// own teardown when it runs, else by stripping the block.
    async fn remove_engine_block(&self, runner_id: Uuid, name: &str) -> Result<()> {
        if let Some(inst) = self
            .running_instances()
            .into_iter()
            .find(|i| i.runner_id == runner_id)
        {
            return self
                .remove_runner_locally(&inst, "engine runner revoked on the server")
                .await;
        }
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::file::mutate_config(&paths, |cfg| {
                cfg.runners.retain(|r| r.runner_id != runner_id);
                Ok(())
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("config write task failed: {e}"))??;
        tracing::info!(runner = %name, "removed the block of a revoked engine runner");
        Ok(())
    }

    /// Write a block for a live engine row with none, in a freshly
    /// allocated folder, and start it. Returns the runner's name.
    async fn readd_engine_block(
        &self,
        workspace: Option<&str>,
        row: &MachineRunnerRow,
    ) -> Result<String> {
        if row.name.is_empty() {
            anyhow::bail!("the server row has no name");
        }
        let ws = if row.workspace_slug.is_empty() {
            workspace
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("the server row names no workspace"))?
                .to_string()
        } else {
            row.workspace_slug.clone()
        };
        if row.project_identifier.is_empty() {
            anyhow::bail!("the server row names no project");
        }
        let cfg = crate::config::file::load_config(&self.paths)?;
        if cfg.runners.len() >= MAX_RUNNERS_PER_DAEMON {
            anyhow::bail!("this computer already hosts {MAX_RUNNERS_PER_DAEMON} runners");
        }
        let working_dir = crate::engine::workdir::allocate_engine_folder(
            &self.paths,
            &ws,
            &row.project_identifier,
        )?;
        // Engine runners authenticate with their workspace's daemon
        // credential, so the block needs no per-runner credentials.
        let resp = EnrollResponse {
            runner_id: row.id,
            runner_name: row.name.clone(),
            refresh_token: String::new(),
            access_token: String::new(),
            access_token_expires_at: String::new(),
            refresh_token_generation: 0,
            workspace_slug: ws,
            pod_slug: String::new(),
            project_identifier: row.project_identifier.clone(),
            long_poll_interval_secs: 25,
            protocol_version: crate::cloud::protocol::WIRE_VERSION,
            machine_token: None,
            machine_token_minted: false,
            created: None,
        };
        let applied = apply_enroll_response(
            &self.paths,
            &resp,
            &self.cloud_url(),
            ApplyEnrollOptions {
                working_dir: Some(working_dir),
                agent_kind: AgentKind::Engine,
                model: None,
                reasoning_effort: None,
            },
        )
        .await?;
        let name = applied.runner.name.clone();
        self.add_runner(applied.runner).await?;
        tracing::info!(runner = %name, "re-added an engine runner the server lists for this machine");
        Ok(name)
    }

    /// Remove engine versions older than the one this daemon resolved at
    /// start (design §4.5). Returns `true` once there is nothing left to
    /// wait for: done, or nothing to clean (an override or no engine).
    /// Returns `false` while a run or chat turn is in flight, or while an
    /// old version is still in use or failed to go, so the next tick tries
    /// again.
    pub(crate) fn cleanup_old_engine_versions(&self) -> bool {
        let Some(engine) = crate::engine::locate::resolved() else {
            return true;
        };
        let busy = self.running_instances().iter().any(|i| {
            i.state.rx_in_flight.borrow().is_some()
                || i.chat_active.load(std::sync::atomic::Ordering::SeqCst)
        });
        match crate::engine::cleanup::cleanup_after_start(
            engine,
            busy,
            crate::engine::cleanup::dir_in_use_by_a_process,
        ) {
            None => !busy,
            Some(Ok(out)) => {
                for dir in &out.removed {
                    tracing::info!(dir = %dir.display(), "removed an old engine version");
                }
                if !out.left.is_empty() {
                    tracing::info!(
                        dirs = ?out.left,
                        "old engine versions kept for now (in use or not removable); retrying on the next tick"
                    );
                }
                out.left.is_empty()
            }
            Some(Err(e)) => {
                tracing::warn!("removing old engine versions failed: {e}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        AgentSection, CliSection, Config, DaemonConfig, EngineCredential, RunnerConfig,
        WorkspaceSection,
    };
    use crate::daemon::workspace_tokens::WorkspaceTokens;
    use crate::engine::test_server::{self, Handler};
    use crate::util::paths::Paths;

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn cli() -> CliSection {
        CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            engine_credential: Some(EngineCredential {
                token: "ec_live".into(),
                expires_at: None,
            }),
            ..Default::default()
        }
    }

    fn engine_block(name: &str, project: &str, working_dir: std::path::PathBuf) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some(project.into()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir },
            agent: AgentSection {
                kind: AgentKind::Engine,
            },
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    /// A spawn context whose engine talks to `url`, with no runner
    /// transport: runners it starts are offline, so nothing but the mock
    /// is ever contacted.
    fn ctx(paths: &Paths, url: &str, machine_id: Uuid) -> RunnerSpawnCtx {
        ctx_signed_in(paths, url, machine_id, cli())
    }

    /// [`ctx`] signed in with `cli`'s workspaces.
    fn ctx_signed_in(
        paths: &Paths,
        url: &str,
        machine_id: Uuid,
        cli: CliSection,
    ) -> RunnerSpawnCtx {
        let tokens = WorkspaceTokens::from_cli(Some(&cli));
        let mut ctx = RunnerSpawnCtx::for_tests_with_daemon(
            None,
            tokens.clone(),
            paths,
            DaemonConfig {
                cloud_url: url.into(),
                dev_machine_id: Some(machine_id),
                ..DaemonConfig::default()
            },
            &[],
        );
        ctx.engine =
            crate::engine::manager::EngineManager::new(Some(url), paths.clone(), tokens, None);
        ctx
    }

    fn seed(paths: &Paths, url: &str, runners: Vec<RunnerConfig>) {
        crate::config::file::write_config(
            paths,
            &Config {
                version: 2,
                daemon: DaemonConfig {
                    cloud_url: url.into(),
                    ..DaemonConfig::default()
                },
                runners,
                cli: Some(cli()),
                engine: Default::default(),
                auth: Default::default(),
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn reconcile_removes_revoked_blocks_and_re_adds_live_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        let machine_id = Uuid::new_v4();

        let revoked = engine_block("builtin-h-acme-old", "OLD", tmp.path().join("tasks/old"));
        let kept = engine_block("builtin-h-acme-web", "WEB", tmp.path().join("tasks/web"));
        std::fs::create_dir_all(&revoked.workspace.working_dir).unwrap();
        std::fs::write(revoked.workspace.working_dir.join("notes.md"), "keep me").unwrap();
        let readd_id = Uuid::new_v4();
        let rows = serde_json::json!({
            "engine_paused_by_owner": true,
            "engine_auto_pause_reason": "",
            "runners": [
                {"id": revoked.runner_id, "name": revoked.name, "workspace_slug": "acme",
                 "project_identifier": "OLD", "agent_kind": "engine",
                 "revoked_at": "2026-11-04T10:00:00Z"},
                {"id": kept.runner_id, "name": kept.name, "workspace_slug": "acme",
                 "project_identifier": "WEB", "agent_kind": "engine"},
                {"id": readd_id, "name": "builtin-h-acme-api", "workspace_slug": "acme",
                 "project_identifier": "API", "provisioning": "desktop_bundled"},
                // Not an engine, or revoked with no block: left alone.
                {"id": Uuid::new_v4(), "name": "my-claude", "workspace_slug": "acme",
                 "project_identifier": "WEB", "agent_kind": "claude_code"},
                {"id": Uuid::new_v4(), "name": "builtin-h-acme-gone", "workspace_slug": "acme",
                 "project_identifier": "GONE", "agent_kind": "engine", "status": "revoked"},
            ],
        })
        .to_string();
        let path = crate::engine::api::dev_machine_path(machine_id);
        let handler: Handler = Arc::new(move |req| {
            if req.path == path && req.method == "GET" {
                assert_eq!(req.bearer(), Some("mt_acme"));
                (200, rows.clone())
            } else {
                (404, String::new())
            }
        });
        let server = test_server::start(handler).await;
        seed(&paths, &server.url, vec![revoked.clone(), kept.clone()]);
        // The API folder exists and is not empty: the re-add takes `API-2`.
        let taken = paths.data_dir.join("engine/workdirs/acme/API");
        std::fs::create_dir_all(&taken).unwrap();
        std::fs::write(taken.join("x"), "x").unwrap();

        let ctx = ctx(&paths, &server.url, machine_id);
        ctx.add_runner(revoked.clone()).await.unwrap();
        ctx.add_runner(kept.clone()).await.unwrap();

        let report = ctx.reconcile_engine_runners().await;
        assert_eq!(report.removed, vec![revoked.name.clone()]);
        assert_eq!(report.added, vec!["builtin-h-acme-api".to_string()]);
        assert!(report.failed.is_empty(), "{:?}", report.failed);

        let cfg = crate::config::file::load_config(&paths).unwrap();
        let names: Vec<&str> = cfg.runners.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec![kept.name.as_str(), "builtin-h-acme-api"]);
        let api = cfg
            .runners
            .iter()
            .find(|r| r.runner_id == readd_id)
            .unwrap();
        assert_eq!(api.agent.kind, AgentKind::Engine);
        assert_eq!(api.project_slug.as_deref(), Some("API"));
        assert_eq!(
            api.workspace.working_dir,
            paths.data_dir.join("engine/workdirs/acme/API-2")
        );
        assert!(api.workspace.working_dir.is_dir());
        // The revoked runner's folder is kept on disk.
        assert!(revoked.workspace.working_dir.join("notes.md").exists());
        // The registry follows: the revoked one is gone, the re-added runs.
        let running: Vec<Uuid> = ctx
            .running_instances()
            .iter()
            .map(|i| i.runner_id)
            .collect();
        assert!(!running.contains(&revoked.runner_id));
        assert!(running.contains(&readd_id) && running.contains(&kept.runner_id));
        // The machine payload's pause state was cached.
        assert_eq!(ctx.engine.status().paused_by_owner, Some(true));

        // A second pass changes nothing.
        let again = ctx.reconcile_engine_runners().await;
        assert_eq!(again, ReconcileReport::default());
    }

    #[tokio::test]
    async fn a_runner_removed_locally_is_not_re_added_and_its_tombstone_is_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        let machine_id = Uuid::new_v4();
        let block = engine_block("builtin-h-acme-web", "WEB", tmp.path().join("tasks/web"));
        let live = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let row = serde_json::json!({"id": block.runner_id, "name": block.name,
            "workspace_slug": "acme", "project_identifier": "WEB", "agent_kind": "engine"});
        let path = crate::engine::api::dev_machine_path(machine_id);
        let live_h = live.clone();
        let handler: Handler = Arc::new(move |req| {
            if req.path == path && req.method == "GET" {
                let runners = if live_h.load(std::sync::atomic::Ordering::SeqCst) {
                    vec![row.clone()]
                } else {
                    vec![]
                };
                (200, serde_json::json!({ "runners": runners }).to_string())
            } else {
                (404, String::new())
            }
        });
        let server = test_server::start(handler).await;
        seed(&paths, &server.url, vec![block.clone()]);

        // `pidash runner remove --local-only`: the block goes, the server
        // row stays live, a tombstone is written.
        crate::cli::runner::remove(
            crate::cli::runner::RemoveArgs {
                name: block.name.clone(),
                local_only: true,
                yes: true,
            },
            &paths,
        )
        .await
        .unwrap();
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .runners
                .is_empty()
        );
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));

        let ctx = ctx(&paths, &server.url, machine_id);
        // The live row is not re-added while the tombstone stands.
        assert_eq!(
            ctx.reconcile_engine_runners().await,
            ReconcileReport::default()
        );
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .runners
                .is_empty()
        );
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));

        // Once the server no longer lists the row as live, the tombstone goes.
        live.store(false, std::sync::atomic::Ordering::SeqCst);
        ctx.reconcile_engine_runners().await;
        assert!(crate::engine::tombstones::load(&paths).is_empty());
    }

    /// `remove --local-only` on workspace beta's runner, then `auth logout
    /// --workspace beta`, then `auth login --workspace beta`: the runner
    /// must not come back. Reconciling while beta is signed out reads only
    /// acme, so beta's tombstone has to survive it.
    #[tokio::test]
    async fn a_signed_out_workspace_keeps_its_tombstones_until_it_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        let machine_id = Uuid::new_v4();
        let mut block = engine_block("builtin-h-beta-web", "WEB", tmp.path().join("tasks/web"));
        block.workspace_slug = Some("beta".into());
        let row = serde_json::json!({"id": block.runner_id, "name": block.name,
            "workspace_slug": "beta", "project_identifier": "WEB", "agent_kind": "engine"});
        let path = crate::engine::api::dev_machine_path(machine_id);
        let handler: Handler = Arc::new(move |req| {
            if req.path != path || req.method != "GET" {
                return (404, String::new());
            }
            // The payload lists only the token's workspace.
            let runners = match req.bearer() {
                Some("mt_beta") => vec![row.clone()],
                _ => vec![],
            };
            (200, serde_json::json!({ "runners": runners }).to_string())
        });
        let server = test_server::start(handler).await;
        let mut both = cli();
        both.workspaces = vec![crate::config::schema::WorkspaceCredential {
            slug: "beta".into(),
            token: "mt_beta".into(),
        }];
        seed(&paths, &server.url, vec![block.clone()]);

        crate::cli::runner::remove(
            crate::cli::runner::RemoveArgs {
                name: block.name.clone(),
                local_only: true,
                yes: true,
            },
            &paths,
        )
        .await
        .unwrap();
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));

        // Signed out of beta: only acme is read, beta's tombstone stays.
        let acme_only = ctx_signed_in(&paths, &server.url, machine_id, cli());
        assert_eq!(
            acme_only.reconcile_engine_runners().await,
            ReconcileReport::default()
        );
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));

        // Signed in to beta again: its live row is still not re-added.
        let signed_in = ctx_signed_in(&paths, &server.url, machine_id, both);
        assert_eq!(
            signed_in.reconcile_engine_runners().await,
            ReconcileReport::default()
        );
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .runners
                .is_empty()
        );
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));
    }

    /// A `remove --local-only` that lands while reconcile waits on the
    /// machine GET: the block is gone from the config reconcile reads, so
    /// the tombstone written before it must be seen too.
    #[tokio::test]
    async fn a_removal_during_the_machine_read_is_not_re_added() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        let machine_id = Uuid::new_v4();
        let block = engine_block("builtin-h-acme-web", "WEB", tmp.path().join("tasks/web"));
        let row = serde_json::json!({"id": block.runner_id, "name": block.name,
            "workspace_slug": "acme", "project_identifier": "WEB", "agent_kind": "engine"});
        let path = crate::engine::api::dev_machine_path(machine_id);
        let removed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (paths_h, removed_h, id) = (paths.clone(), removed.clone(), block.runner_id);
        let handler: Handler = Arc::new(move |req| {
            if req.path != path || req.method != "GET" {
                return (404, String::new());
            }
            // What `runner remove --local-only` does: tombstone, then strip.
            if !removed_h.swap(true, std::sync::atomic::Ordering::SeqCst) {
                crate::engine::tombstones::add(&paths_h, id, Some("acme")).unwrap();
                crate::config::file::mutate_config(&paths_h, |c| {
                    c.runners.retain(|r| r.runner_id != id);
                    Ok(())
                })
                .unwrap();
            }
            (
                200,
                serde_json::json!({ "runners": [row.clone()] }).to_string(),
            )
        });
        let server = test_server::start(handler).await;
        seed(&paths, &server.url, vec![block.clone()]);

        let ctx = ctx(&paths, &server.url, machine_id);
        assert_eq!(
            ctx.reconcile_engine_runners().await,
            ReconcileReport::default()
        );
        assert!(removed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            crate::config::file::load_config(&paths)
                .unwrap()
                .runners
                .is_empty()
        );
        assert!(crate::engine::tombstones::load(&paths).contains(&block.runner_id));
    }

    #[tokio::test]
    async fn adding_a_runner_again_forgets_its_tombstone() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        seed(&paths, "http://127.0.0.1:9", vec![]);
        let id = Uuid::new_v4();
        crate::engine::tombstones::add(&paths, id, Some("acme")).unwrap();
        let resp = EnrollResponse {
            runner_id: id,
            runner_name: "builtin-h-acme-web".into(),
            refresh_token: String::new(),
            access_token: String::new(),
            access_token_expires_at: String::new(),
            refresh_token_generation: 0,
            workspace_slug: "acme".into(),
            pod_slug: String::new(),
            project_identifier: "WEB".into(),
            long_poll_interval_secs: 25,
            protocol_version: crate::cloud::protocol::WIRE_VERSION,
            machine_token: None,
            machine_token_minted: false,
            created: None,
        };
        apply_enroll_response(
            &paths,
            &resp,
            "http://127.0.0.1:9",
            ApplyEnrollOptions {
                working_dir: Some(tmp.path().join("tasks/web")),
                agent_kind: AgentKind::Engine,
                model: None,
                reasoning_effort: None,
            },
        )
        .await
        .unwrap();
        assert!(crate::engine::tombstones::load(&paths).is_empty());
    }

    #[tokio::test]
    async fn reconcile_leaves_everything_alone_when_the_machine_read_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        paths.ensure().unwrap();
        let handler: Handler = Arc::new(|_| (503, String::new()));
        let server = test_server::start(handler).await;
        let block = engine_block("builtin-h-acme-web", "WEB", tmp.path().join("tasks/web"));
        seed(&paths, &server.url, vec![block.clone()]);
        let ctx = ctx(&paths, &server.url, Uuid::new_v4());
        assert_eq!(
            ctx.reconcile_engine_runners().await,
            ReconcileReport::default()
        );
        let cfg = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 1);
    }
}
