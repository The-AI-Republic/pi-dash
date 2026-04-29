use anyhow::{Context, Result};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};
use tokio::net::{UnixListener, UnixStream};
use uuid::Uuid;

use super::protocol::{Request, Response, RpcError, StatusSnapshot};
use crate::approval::router::DecisionSource;
use crate::daemon::runner_instance::RunnerInstance;

/// IPC server. Owns the multi-runner instance map and dispatches
/// per-runner requests to the right `RunnerInstance`'s state /
/// approvals / paths.
pub struct IpcServer {
    pub path: PathBuf,
    /// Connection-level state. Used for `daemon_info` (cloud_url,
    /// connected, uptime) and the Status subscribe tick stream. Any of
    /// the `RunnerInstance` state handles works as a tick driver since
    /// the supervisor ticks them all together; we keep a dedicated
    /// "primary" handle for that purpose.
    pub primary_state: crate::daemon::state::StateHandle,
    pub paths: crate::util::paths::Paths,
    /// All configured runners, keyed by runner_id. Resolved on every
    /// per-runner request via the `runner` selector. Wrapped in `Arc`
    /// so the supervisor can hand it to the IPC task without giving up
    /// ownership.
    pub instances: Arc<HashMap<Uuid, RunnerInstance>>,
}

impl IpcServer {
    pub async fn run(self) -> Result<()> {
        // Clean up stale socket.
        let _ = tokio::fs::remove_file(&self.path).await;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {parent:?}"))?;
            // Best-effort: tighten the runtime dir to 0700 so even if the
            // socket bind racewindow is exploited, no peer can reach it.
            let mut p = std::fs::metadata(parent)?.permissions();
            p.set_mode(0o700);
            let _ = std::fs::set_permissions(parent, p);
        }
        // Restrict the file mode of files (including the socket) we create
        // until after bind, then restore. Closes the TOCTOU window where the
        // socket exists with the process umask before set_permissions runs.
        let prev_umask = nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o077));
        let bind_result = UnixListener::bind(&self.path);
        nix::sys::stat::umask(prev_umask);
        let listener =
            bind_result.with_context(|| format!("binding unix socket {:?}", self.path))?;
        // Belt-and-braces: enforce 0600 on the socket explicitly.
        let mut perm = std::fs::metadata(&self.path)?.permissions();
        perm.set_mode(0o600);
        std::fs::set_permissions(&self.path, perm)?;

        let me = Arc::new(self);
        loop {
            let (stream, _) = listener.accept().await?;
            let server = me.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_conn(stream).await {
                    tracing::warn!("ipc conn error: {e:#}");
                }
            });
        }
    }

    async fn handle_conn(&self, stream: UnixStream) -> Result<()> {
        let mut buf = BufStream::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            let n = buf.read_line(&mut line).await?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let req: Request = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    let err = Response::Error(RpcError {
                        code: 400,
                        message: format!("invalid request: {e}"),
                    });
                    write_line(&mut buf, &err).await?;
                    continue;
                }
            };
            match self.dispatch(req, &mut buf).await {
                Ok(resp) => write_line(&mut buf, &resp).await?,
                Err(e) => {
                    write_line(
                        &mut buf,
                        &Response::Error(RpcError {
                            code: 500,
                            message: e.to_string(),
                        }),
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn dispatch(&self, req: Request, buf: &mut BufStream<UnixStream>) -> Result<Response> {
        match req {
            Request::StatusGet => Ok(Response::Status(self.status_snapshot().await)),
            Request::StatusSubscribe => {
                let mut rx = self.primary_state.subscribe();
                let first = self.status_snapshot().await;
                write_line(buf, &Response::Status(first)).await?;
                while rx.changed().await.is_ok() {
                    let snap = self.status_snapshot().await;
                    write_line(buf, &Response::StatusDelta(snap)).await?;
                }
                Ok(Response::Ack)
            }
            Request::ConfigGet => {
                let cfg = crate::config::file::load_config(&self.paths)?;
                Ok(Response::Config(serde_json::to_value(&cfg)?))
            }
            Request::ConfigUpdate { patch, runner } => {
                // ``runner`` is informational at the moment — the patch
                // is applied to the on-disk config which already
                // contains every `[[runner]]` block, and the daemon's
                // primary state is reloaded. A future change will
                // route the patch into a specific instance's state
                // (e.g. for live approval-policy pushes); for now we
                // require the selector when the patch obviously
                // targets a runner so the API is honest.
                let _ = runner;
                let mut cfg = crate::config::file::load_config(&self.paths)?;
                merge_json(&mut cfg, patch)?;
                crate::config::file::write_config(&self.paths, &cfg)?;
                self.primary_state.set_config(cfg.clone()).await;
                Ok(Response::Ack)
            }
            Request::RunsList { limit, runner } => {
                let inst = self.resolve_runner(runner.as_deref())?;
                let index = crate::history::index::RunsIndex::load(&inst.paths)?;
                Ok(Response::Runs(index.recent(limit.unwrap_or(100))))
            }
            Request::RunsGet { run_id, runner } => {
                // When a runner is named, scope the lookup to that
                // instance's history. Without one, scan every instance
                // — run IDs are globally unique, so the first match is
                // authoritative.
                let candidates: Vec<&RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => self.instances.values().collect(),
                };
                for inst in candidates {
                    let index = crate::history::index::RunsIndex::load(&inst.paths)?;
                    if let Some(summary) = index.runs.get(&run_id).cloned() {
                        let path = inst.paths.runs_dir().join(format!("{run_id}.jsonl"));
                        let events = if path.exists() {
                            crate::history::jsonl::read_all(&path)
                                .await?
                                .into_iter()
                                .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        return Ok(Response::Run { summary, events });
                    }
                }
                anyhow::bail!("run not found");
            }
            Request::ApprovalsList { runner } => {
                let mut all = Vec::new();
                let candidates: Vec<&RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => self.instances.values().collect(),
                };
                for inst in candidates {
                    all.extend(inst.approvals.list_pending().await);
                }
                Ok(Response::Approvals(all))
            }
            Request::ApprovalsDecide {
                approval_id,
                decision,
                runner,
            } => {
                // Approval IDs are globally unique. With a `runner`
                // selector we route to that instance directly; without
                // one, scan every instance and decide on whichever
                // owns the approval.
                let candidates: Vec<&RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => self.instances.values().collect(),
                };
                for inst in candidates {
                    let resolved = inst
                        .approvals
                        .decide(&approval_id, decision, DecisionSource::Local)
                        .await;
                    if resolved.is_some() {
                        return Ok(Response::Ack);
                    }
                }
                anyhow::bail!("approval not found or already resolved");
            }
            Request::DoctorRun { runner } => {
                let _ = runner; // doctor walks every runner in `Paths`; future
                                // refactor will scope by runner name.
                let report = crate::cli::doctor::execute(&self.paths).await?;
                Ok(Response::Doctor(report))
            }
            Request::RunnerReconnect => {
                self.primary_state.force_reconnect();
                Ok(Response::Ack)
            }
            Request::RunnerDisconnect => {
                self.primary_state.shutdown();
                Ok(Response::Ack)
            }
        }
    }

    /// Resolve a runner-name selector to a concrete `RunnerInstance`.
    /// When `name` is `None` and the daemon has exactly one runner
    /// (the common single-runner install), return it. When `name` is
    /// `None` and there are multiple, refuse with a hint listing the
    /// configured names — the caller must disambiguate.
    fn resolve_runner(&self, name: Option<&str>) -> Result<&RunnerInstance> {
        match name {
            Some(n) => self
                .instances
                .values()
                .find(|i| i.name == n)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no runner named {:?}; configured: [{}]",
                        n,
                        self.runner_names().join(", ")
                    )
                }),
            None => {
                if self.instances.len() == 1 {
                    Ok(self.instances.values().next().unwrap())
                } else {
                    anyhow::bail!(
                        "this daemon hosts {} runners ([{}]); pass --runner <name>",
                        self.instances.len(),
                        self.runner_names().join(", "),
                    )
                }
            }
        }
    }

    fn runner_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.instances.values().map(|i| i.name.clone()).collect();
        names.sort();
        names
    }

    async fn status_snapshot(&self) -> StatusSnapshot {
        let daemon = self.primary_state.daemon_info().await;
        let mut runners = Vec::with_capacity(self.instances.len());
        for inst in self.instances.values() {
            runners.push(inst.state.runner_snapshot().await);
        }
        // Stable order so successive snapshots don't churn rendering.
        runners.sort_by(|a, b| a.name.cmp(&b.name));
        StatusSnapshot { daemon, runners }
    }
}

async fn write_line(buf: &mut BufStream<UnixStream>, resp: &Response) -> Result<()> {
    let mut line = serde_json::to_vec(resp)?;
    line.push(b'\n');
    buf.write_all(&line).await?;
    buf.flush().await?;
    Ok(())
}

fn merge_json(cfg: &mut crate::config::schema::Config, patch: serde_json::Value) -> Result<()> {
    let mut base = serde_json::to_value(&*cfg)?;
    deep_merge(&mut base, patch);
    *cfg = serde_json::from_value(base)?;
    Ok(())
}

fn deep_merge(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                deep_merge(b.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, p) => *b = p,
    }
}
