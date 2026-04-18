use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};
use tokio::net::{UnixListener, UnixStream};

use super::protocol::{Request, Response, RpcError, StatusSnapshot};
use crate::approval::router::{ApprovalRouter, DecisionSource};
use crate::daemon::state::StateHandle;

pub struct IpcServer {
    pub path: PathBuf,
    pub state: StateHandle,
    pub approvals: ApprovalRouter,
    pub paths: crate::util::paths::Paths,
}

impl IpcServer {
    pub async fn run(self) -> Result<()> {
        // Clean up stale socket.
        let _ = tokio::fs::remove_file(&self.path).await;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {parent:?}"))?;
        }
        let listener = UnixListener::bind(&self.path)
            .with_context(|| format!("binding unix socket {:?}", self.path))?;
        // Socket 0600.
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
                let mut rx = self.state.subscribe();
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
            Request::ConfigUpdate { patch } => {
                let mut cfg = crate::config::file::load_config(&self.paths)?;
                merge_json(&mut cfg, patch)?;
                crate::config::file::write_config(&self.paths, &cfg)?;
                self.state.set_config(cfg.clone()).await;
                Ok(Response::Ack)
            }
            Request::RunsList { limit } => {
                let index = crate::history::index::RunsIndex::load(&self.paths)?;
                Ok(Response::Runs(index.recent(limit.unwrap_or(100))))
            }
            Request::RunsGet { run_id } => {
                let index = crate::history::index::RunsIndex::load(&self.paths)?;
                let summary = index
                    .runs
                    .get(&run_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("run not found"))?;
                let path = self.paths.runs_dir().join(format!("{run_id}.jsonl"));
                let events = if path.exists() {
                    crate::history::jsonl::read_all(&path)
                        .await?
                        .into_iter()
                        .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                        .collect()
                } else {
                    Vec::new()
                };
                Ok(Response::Run { summary, events })
            }
            Request::ApprovalsList => {
                let pending = self.approvals.list_pending().await;
                Ok(Response::Approvals(pending))
            }
            Request::ApprovalsDecide {
                approval_id,
                decision,
            } => {
                let resolved = self
                    .approvals
                    .decide(&approval_id, decision, DecisionSource::Local)
                    .await;
                if resolved.is_none() {
                    anyhow::bail!("approval not found or already resolved");
                }
                Ok(Response::Ack)
            }
            Request::DoctorRun => {
                let report = crate::cli::doctor::execute(&self.paths).await?;
                Ok(Response::Doctor(report))
            }
            Request::RunnerReconnect => {
                self.state.force_reconnect();
                Ok(Response::Ack)
            }
            Request::RunnerDisconnect => {
                self.state.shutdown();
                Ok(Response::Ack)
            }
        }
    }

    async fn status_snapshot(&self) -> StatusSnapshot {
        self.state.snapshot().await
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
