use anyhow::{Context, Result};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer as IpcStream, ServerOptions};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream as IpcStream};
use uuid::Uuid;

use super::chat;
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
    pub instances: Arc<std::sync::RwLock<HashMap<Uuid, RunnerInstance>>>,
    pub(crate) spawn_ctx: crate::daemon::supervisor::RunnerSpawnCtx,
}

impl IpcServer {
    #[cfg(unix)]
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

    #[cfg(windows)]
    pub async fn run(self) -> Result<()> {
        let pipe_name = crate::ipc::windows_pipe_name(&self.path);
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .with_context(|| format!("creating named pipe {pipe_name}"))?;

        let me = Arc::new(self);
        loop {
            server
                .connect()
                .await
                .with_context(|| format!("accepting named-pipe client at {pipe_name}"))?;
            let connected = server;
            server = ServerOptions::new()
                .create(&pipe_name)
                .with_context(|| format!("creating next named pipe {pipe_name}"))?;
            let server = me.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_conn(connected).await {
                    tracing::warn!("ipc conn error: {e:#}");
                }
            });
        }
    }

    async fn handle_conn(&self, stream: IpcStream) -> Result<()> {
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

    async fn dispatch(&self, req: Request, buf: &mut BufStream<IpcStream>) -> Result<Response> {
        match req {
            Request::Hello => Ok(Response::Hello(self.spawn_ctx.hello())),
            Request::RunnerActivateLocal { runner } => {
                self.spawn_ctx.activate_configured(&runner).await?;
                Ok(Response::Ack)
            }
            Request::RunnerReconfigure { runner, changes } => {
                self.spawn_ctx.reconfigure_runner(&runner, changes).await?;
                Ok(Response::Ack)
            }
            Request::CredentialsReload => {
                self.spawn_ctx.reload_credentials().await?;
                Ok(Response::Ack)
            }
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
                // Protocol promises "redacted of credentials": never hand
                // the IPC socket a live mt_/ec_ token. Non-secret fields
                // stay so the desktop's daemon_runners can still enumerate
                // runners. See Config::redacted.
                let mut json = serde_json::to_value(cfg.redacted())?;
                // Defense in depth: clear any token-like key the typed
                // redaction might not know about (future per-runner fields).
                redact_json_for_ipc(&mut json);
                Ok(Response::Config(json))
            }
            Request::ConfigUpdate { patch, runner } => {
                // ``runner`` is reserved / informational — the patch is
                // deep-merged into the on-disk config (which already
                // contains every `[[runner]]` block) and the daemon's
                // primary state is reloaded. The selector is kept on
                // the wire so a future per-runner live-patch flow can
                // opt in without bumping IPC version again.
                let _ = runner;
                let mut cfg = crate::config::file::load_config(&self.paths)?;
                // Never let ConfigUpdate clobber credentials with the
                // redacted placeholders ConfigGet now returns. Credentials
                // change only via `pidash auth login/logout`. Stripping
                // the keys from the patch plus restoring after the merge
                // keeps the on-disk mt_/ec_ tokens intact through any
                // ConfigGet->ConfigUpdate round-trip.
                let original = cfg.clone();
                let mut patch = patch;
                strip_secrets_from_patch(&mut patch);
                merge_json(&mut cfg, patch)?;
                restore_secrets(&mut cfg, &original);
                crate::config::file::write_config(&self.paths, &cfg)?;
                self.primary_state.set_config(cfg.clone()).await;
                Ok(Response::Ack)
            }
            Request::RunsList { limit, runner } => {
                // ``runner = None`` means "union across every configured
                // runner" — same shape as RunsGet / ApprovalsList. The
                // TUI relies on this so the Runs tab works before the
                // picker has been seeded by the Config refresh branch.
                let candidates: Vec<RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => self.all_instances(),
                };
                let cap = limit.unwrap_or(100);
                let mut all = Vec::new();
                for inst in candidates {
                    let index = crate::history::index::RunsIndex::load(&inst.paths)?;
                    all.extend(index.recent(cap));
                }
                // Newest-first across the union, then trim to the cap so
                // a 100-row request returns the freshest 100 globally —
                // not 100 per runner.
                all.sort_by_key(|run| std::cmp::Reverse(run.started_at));
                all.truncate(cap);
                Ok(Response::Runs(all))
            }
            Request::RunsGet { run_id, runner } => {
                // When a runner is named, scope the lookup to that
                // instance's history. Without one, scan every instance
                // — run IDs are globally unique, so the first match is
                // authoritative.
                let candidates: Vec<RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => self.all_instances(),
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
                let candidates: Vec<RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => {
                        // Local chat approvals on the engine profile too.
                        all.extend(self.spawn_ctx.local_chat.approvals.list_pending().await);
                        self.all_instances()
                    }
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
                let candidates: Vec<RunnerInstance> = match runner.as_deref() {
                    Some(name) => vec![self.resolve_runner(Some(name))?],
                    None => {
                        let resolved = self
                            .spawn_ctx
                            .local_chat
                            .approvals
                            .decide(&approval_id, decision, DecisionSource::Local)
                            .await;
                        if resolved.is_some() {
                            return Ok(Response::Ack);
                        }
                        self.all_instances()
                    }
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
                let report = crate::cli::doctor::execute(&self.paths, runner.as_deref()).await?;
                Ok(Response::Doctor(report))
            }
            Request::RunnerReconnect => {
                self.spawn_ctx.kick_reconnect(None)?;
                Ok(Response::Ack)
            }
            Request::Reconnect { runner } => {
                self.spawn_ctx.kick_reconnect(runner.as_deref())?;
                Ok(Response::Ack)
            }
            Request::RunnerDisconnect => {
                self.primary_state.shutdown();
                Ok(Response::Ack)
            }
            Request::RunnerRemoveLocal { runner } => {
                self.remove_local(&runner).await?;
                Ok(Response::Ack)
            }
            // ---- Built-in engine (IPC v6, PIDESKAPP-121) ----------------
            Request::EngineModelToken => match self.spawn_ctx.engine.model_token().await {
                Ok(t) => Ok(Response::EngineModelToken {
                    token: t.token,
                    expires_at: t.expires_at,
                }),
                // The message starts with the reason code, so the helper
                // (and the engine's log) say why.
                Err(e) => Ok(Response::Error(RpcError {
                    code: 403,
                    message: e.to_string(),
                })),
            },
            Request::EngineStatus => Ok(Response::EngineStatus(self.spawn_ctx.engine.status())),
            Request::EngineSetPaused { paused } => Ok(Response::EngineStatus(
                self.spawn_ctx.engine_set_paused(paused).await?,
            )),
            Request::EngineRunnerAdd {
                workspace,
                project,
                working_dir,
            } => {
                let info = self
                    .spawn_ctx
                    .engine_runner_add(workspace, project, working_dir)
                    .await?;
                Ok(Response::EngineRunnerAdded(info))
            }
            Request::EngineRunnerRemove { runner } => {
                // Revoke in the cloud first, preserving the runner's row
                // and history, then remove its local block. The task
                // folder is kept (built-in engine design §11).
                let (runner_id, token) = self.spawn_ctx.engine_runner_for_removal(&runner)?;
                crate::cloud::runners::revoke_runner(
                    &self.spawn_ctx.cloud_url(),
                    &token,
                    &runner_id,
                )
                .await
                .with_context(|| format!("removing {runner:?} in the cloud"))?;
                self.remove_local(&runner).await?;
                Ok(Response::Ack)
            }
            Request::EngineSettingsGet => {
                Ok(Response::EngineSettings(self.spawn_ctx.engine_settings()?))
            }
            Request::EngineSettingsSet { task_root } => Ok(Response::EngineSettings(
                self.spawn_ctx.set_engine_settings(task_root).await?,
            )),
            // Local chat (PDASHOSS01-159, IPC v4). The desktop host drives
            // the built-in engine directly over this socket — never through
            // the Pi Dash chat relay. Streamed `Response::Chat*` frames are
            // written onto `buf` mid-call (same pattern as StatusSubscribe);
            // the terminal frame is returned. See `super::chat`. With no
            // `runner` selector and an absolute `cwd` the chat runs on the
            // daemon's own engine profile (design §12.1), not on a runner.
            Request::ChatWarm {
                chat_session_id,
                runner,
                cwd,
                model,
                mode,
                local_thread_id,
                local_session_id,
            } => {
                let target = self
                    .chat_target(runner.as_deref(), chat_session_id, cwd.as_deref())
                    .await?;
                let host = self.chat_host(&target);
                let mut sink = chat::SocketSink { buf };
                chat::handle_warm(
                    &host,
                    chat::WarmArgs {
                        chat_session_id,
                        cwd,
                        model,
                        mode,
                        local_thread_id,
                        local_session_id,
                    },
                    &mut sink,
                )
                .await
            }
            Request::ChatSend {
                chat_session_id,
                message_id,
                content,
                runner,
                cwd,
                model,
                mode,
                local_thread_id,
                local_session_id,
            } => {
                let target = self
                    .chat_target(runner.as_deref(), chat_session_id, cwd.as_deref())
                    .await?;
                let host = self.chat_host(&target);
                let mut sink = chat::SocketSink { buf };
                chat::handle_send(
                    &host,
                    chat::SendArgs {
                        chat_session_id,
                        message_id,
                        content,
                        cwd,
                        model,
                        mode,
                        local_thread_id,
                        local_session_id,
                    },
                    &mut sink,
                )
                .await
            }
            Request::ChatCancel {
                chat_session_id,
                runner,
                ..
            } => {
                let target = self
                    .chat_target(runner.as_deref(), chat_session_id, None)
                    .await?;
                chat::handle_cancel(&self.chat_host(&target), chat_session_id).await
            }
            Request::ChatClose {
                chat_session_id,
                runner,
                ..
            } => {
                let target = self
                    .chat_target(runner.as_deref(), chat_session_id, None)
                    .await?;
                let host = self.chat_host(&target);
                let mut sink = chat::SocketSink { buf };
                chat::handle_close(&host, chat_session_id, &mut sink).await
            }
            Request::ChatDecide {
                chat_session_id,
                local_approval_id,
                decision,
                runner,
            } => {
                let target = self
                    .chat_target(runner.as_deref(), chat_session_id, None)
                    .await?;
                chat::handle_decide(&self.chat_host(&target), &local_approval_id, decision).await
            }
        }
    }

    /// Who serves a local chat request (design §12.1):
    ///
    /// 1. a `runner` selector: that runner, as before;
    /// 2. a session the engine profile already holds: the profile;
    /// 3. an absolute `cwd`: the daemon's engine profile, whatever runners
    ///    the daemon hosts (zero, one or many) and whatever they are doing;
    /// 4. a session a runner already holds: that runner;
    /// 5. otherwise the single runner (no `cwd`, or a relative one, narrows
    ///    to a runner's folder), or — with zero or several runners — the
    ///    profile for a request that needs no folder (cancel, close, decide)
    ///    and an error naming the runners for one that does.
    async fn chat_target(
        &self,
        runner: Option<&str>,
        chat_session_id: Uuid,
        cwd: Option<&str>,
    ) -> Result<ChatTarget> {
        if let Some(name) = runner {
            return Ok(ChatTarget::Runner(Box::new(
                self.resolve_runner(Some(name))?,
            )));
        }
        if self
            .spawn_ctx
            .local_chat
            .sessions
            .contains(chat_session_id)
            .await
        {
            return Ok(ChatTarget::Engine);
        }
        if cwd.is_some_and(|c| std::path::Path::new(c).is_absolute()) {
            return Ok(ChatTarget::Engine);
        }
        let instances = self.all_instances();
        for inst in &instances {
            if inst.chat_sessions.contains(chat_session_id).await {
                return Ok(ChatTarget::Runner(Box::new(inst.clone())));
            }
        }
        if instances.len() == 1 {
            return Ok(ChatTarget::Runner(Box::new(
                instances.into_iter().next().expect("one instance"),
            )));
        }
        match cwd {
            // Cancel / close / decide of a session nobody holds: answer it on
            // the profile, which acks or reports "not found" as for any host.
            None => Ok(ChatTarget::Engine),
            Some(_) => Err(self
                .resolve_runner(None)
                .err()
                .unwrap_or_else(|| anyhow::anyhow!("pass an absolute chat folder (`cwd`)"))),
        }
    }

    fn chat_host<'a>(&'a self, target: &'a ChatTarget) -> chat::ChatHost<'a> {
        match target {
            ChatTarget::Runner(inst) => chat::ChatHost::runner(inst),
            ChatTarget::Engine => {
                chat::ChatHost::engine(&self.spawn_ctx.local_chat, self.runner_working_dirs())
            }
        }
    }

    /// `(name, working dir)` of every runner: configured on disk and
    /// running, so a block written but not started yet counts too.
    fn runner_working_dirs(&self) -> Vec<(String, PathBuf)> {
        let mut dirs: Vec<(String, PathBuf)> = crate::config::file::load_config(&self.paths)
            .map(|cfg| {
                cfg.runners
                    .into_iter()
                    .map(|r| (r.name, r.workspace.working_dir))
                    .collect()
            })
            .unwrap_or_default();
        for inst in self.all_instances() {
            if !dirs.iter().any(|(n, _)| *n == inst.name) {
                dirs.push((inst.name.clone(), inst.config.workspace.working_dir.clone()));
            }
        }
        dirs
    }

    /// Resolve a runner-name selector to a concrete `RunnerInstance`.
    /// When `name` is `None` and the daemon has exactly one runner
    /// (the common single-runner install), return it. When `name` is
    /// `None` and there are multiple, refuse with a hint listing the
    /// configured names — the caller must disambiguate.
    /// Local-only removal of `runner` (no cloud call): the same teardown
    /// the cloud's `remove_runner` frame triggers. Returns once it finished.
    async fn remove_local(&self, runner: &str) -> Result<()> {
        // Synthesize a `ServerMsg::RemoveRunner` envelope into
        // the target instance's inbound mailbox. The supervisor
        // already has the full teardown handler for that
        // variant (cancel run + strip config.toml + delete
        // data dir + exit RunnerLoop), so we don't duplicate
        // any of that logic here — we just enter the same
        // code path the cloud's wire frame would.
        let inst = self.resolve_runner(Some(runner))?;
        self.spawn_ctx
            .remove_runner_locally(&inst, "local removal via pidash CLI")
            .await
    }

    fn resolve_runner(&self, name: Option<&str>) -> Result<RunnerInstance> {
        let instances = self.all_instances();
        let mut names: Vec<_> = instances.iter().map(|i| i.name.clone()).collect();
        names.sort();
        match name {
            Some(n) => instances.into_iter().find(|i| i.name == n).ok_or_else(|| {
                anyhow::anyhow!(
                    "no runner named {:?}; configured: [{}]",
                    n,
                    names.join(", ")
                )
            }),
            None => {
                if instances.len() == 1 {
                    Ok(instances.into_iter().next().unwrap())
                } else {
                    anyhow::bail!(
                        "this daemon hosts {} runners ([{}]); pass --runner <name>",
                        instances.len(),
                        names.join(", "),
                    )
                }
            }
        }
    }

    fn all_instances(&self) -> Vec<RunnerInstance> {
        self.instances
            .read()
            .expect("runner registry poisoned")
            .values()
            .cloned()
            .collect()
    }

    async fn status_snapshot(&self) -> StatusSnapshot {
        let daemon = self.primary_state.daemon_info().await;
        let instances = self.all_instances();
        let mut runners = Vec::with_capacity(instances.len());
        for inst in instances {
            let mut snap = inst.state.runner_snapshot().await;
            snap.workspace_signed_out = self.spawn_ctx.is_workspace_signed_out(&inst);
            runners.push(snap);
        }
        // Stable order so successive snapshots don't churn rendering.
        runners.sort_by(|a, b| a.name.cmp(&b.name));
        StatusSnapshot { daemon, runners }
    }
}

/// Who serves a local chat request: see [`IpcServer::chat_target`].
enum ChatTarget {
    Runner(Box<RunnerInstance>),
    /// The daemon's machine-level engine profile.
    Engine,
}

async fn write_line(buf: &mut BufStream<IpcStream>, resp: &Response) -> Result<()> {
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

/// Remove credential keys from a `ConfigUpdate` patch (recursive).
/// ConfigGet now returns `cli.token`, `cli.workspaces[].token` and
/// `cli.engine_credential.token` as absent; if a client round-trips
/// that value back through ConfigUpdate, the merge would otherwise
/// clear the on-disk tokens. Stripping the keys here makes the round-
/// trip a no-op for secrets — they stay on disk and change only via
/// `pidash auth login/logout`.
///
/// Keys are matched exactly (`token`, `api_token`, `connection_secret`)
/// so non-secret paths like `model_token_file` survive.
fn strip_secrets_from_patch(patch: &mut serde_json::Value) {
    const SECRET_KEYS: &[&str] = &["token", "api_token", "connection_secret"];
    match patch {
        serde_json::Value::Object(map) => {
            for key in SECRET_KEYS {
                map.remove(*key);
            }
            for value in map.values_mut() {
                strip_secrets_from_patch(value);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_secrets_from_patch(item);
            }
        }
        _ => {}
    }
}

/// Restore credentials that a `ConfigUpdate` merge might have blanked
/// via the redacted `ConfigGet` value or a partial `workspaces` array.
/// If the merged config lost a token that the on-disk config had, and
/// the patch did not carry an explicit new token, restore the original.
/// Tokens change only via `pidash auth login/logout`, not IPC.
fn restore_secrets(
    merged: &mut crate::config::schema::Config,
    original: &crate::config::schema::Config,
) {
    let Some(merged_cli) = merged.cli.as_mut() else {
        return;
    };
    let Some(orig_cli) = original.cli.as_ref() else {
        return;
    };
    // cli.token: restore when merged is empty.
    if merged_cli.token.as_deref().is_none_or(str::is_empty)
        && let Some(t) = orig_cli.token.as_deref().filter(|s| !s.is_empty())
    {
        merged_cli.token = Some(t.to_string());
    }
    // engine credential.
    if merged_cli
        .engine_credential
        .as_ref()
        .is_some_and(|ec| ec.token.is_empty())
        && let Some(ec) = orig_cli
            .engine_credential
            .as_ref()
            .filter(|ec| !ec.token.is_empty())
    {
        merged_cli
            .engine_credential
            .get_or_insert_with(crate::config::schema::EngineCredential::default)
            .token = ec.token.clone();
        if merged_cli
            .engine_credential
            .as_ref()
            .unwrap()
            .expires_at
            .is_none()
        {
            merged_cli.engine_credential.as_mut().unwrap().expires_at = ec.expires_at;
        }
    }
    // Workspace tokens: match by slug; restore when the merged entry lost it.
    let orig_by_slug: std::collections::HashMap<&str, &str> = orig_cli
        .workspaces
        .iter()
        .map(|w| (w.slug.as_str(), w.token.as_str()))
        .collect();
    for w in &mut merged_cli.workspaces {
        if w.token.is_empty()
            && let Some(&orig_token) = orig_by_slug.get(w.slug.as_str())
            && !orig_token.is_empty()
        {
            w.token = orig_token.to_string();
        }
    }
}

/// Defense-in-depth for `ConfigGet`: clear any token-like key that
/// remains in the serialized JSON after the typed `Config::redacted`
/// pass. `Config::redacted` already clears the known `mt_`/`ec_` fields;
/// this walk catches future per-runner secret fields that might be added
/// to `Config` without updating the typed redaction. Exact-key match
/// keeps `model_token_file` (a path, not a secret) intact.
fn redact_json_for_ipc(value: &mut serde_json::Value) {
    const SECRET_KEYS: &[&str] = &["token", "api_token", "connection_secret"];
    match value {
        serde_json::Value::Object(map) => {
            for key in SECRET_KEYS {
                if let Some(v) = map.get_mut(*key)
                    && v.as_str().is_some_and(|s| !s.is_empty())
                {
                    *v = serde_json::Value::String(String::new());
                }
            }
            for v in map.values_mut() {
                redact_json_for_ipc(v);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_for_ipc(item);
            }
        }
        _ => {}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::schema::{CliSection, Config, EngineCredential, WorkspaceCredential};
    use crate::daemon::supervisor::RunnerSpawnCtx;
    use crate::daemon::workspace_tokens::WorkspaceTokens;
    use crate::engine::test_server::{self, Handler};
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn daemon_with(
        cli: CliSection,
        url: &str,
    ) -> (tempfile::TempDir, crate::util::paths::Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("c"),
            data_dir: tmp.path().join("d"),
            runtime_dir: tmp.path().join("r"),
            machine_context_dir: tmp.path().join("c"),
        };
        let tokens = WorkspaceTokens::from_cli(Some(&cli));
        let transport = crate::cloud::http::SharedHttpTransport::new(url.to_string()).unwrap();
        let spawn_ctx = RunnerSpawnCtx::for_tests(Some(transport), tokens, &paths, &[]);
        let server = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: crate::daemon::state::StateHandle::new(Config {
                version: 2,
                daemon: Default::default(),
                runners: vec![],
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            }),
            paths: paths.clone(),
            instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            spawn_ctx,
        };
        tokio::spawn(async move {
            let _ = server.run().await;
        });
        for _ in 0..200 {
            if paths.ipc_socket_path().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        (tmp, paths)
    }

    fn cli(engine: bool) -> CliSection {
        CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            engine_credential: engine.then(|| EngineCredential {
                token: "ec_live".into(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(10)),
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn engine_auth_helper_gets_the_in_memory_model_token_over_ipc() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(req.bearer(), Some("ec_live"));
            c.fetch_add(1, Ordering::SeqCst);
            let body = serde_json::json!({
                "token": "gw_token",
                "expires_at": (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
            });
            (200, body.to_string())
        });
        let server = test_server::start(handler).await;
        let (_tmp, paths) = daemon_with(cli(true), &server.url).await;

        // What `pidash __engine model-token` prints.
        let token = crate::cli::engine_cmd::fetch_model_token(&paths)
            .await
            .unwrap();
        assert_eq!(token, "gw_token");
        let again = crate::cli::engine_cmd::fetch_model_token(&paths)
            .await
            .unwrap();
        assert_eq!(again, "gw_token");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "served from daemon memory");

        let mut client = crate::ipc::client::Client::connect(paths.ipc_socket_path())
            .await
            .unwrap();
        match client.call(Request::EngineStatus).await.unwrap() {
            Response::EngineStatus(st) => {
                assert_eq!(
                    st.sign_in_state,
                    crate::ipc::protocol::EngineSignInState::Active
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn engine_auth_helper_reports_why_there_is_no_token() {
        let handler: Handler = Arc::new(|_| (500, String::new()));
        let server = test_server::start(handler).await;
        let (_tmp, paths) = daemon_with(cli(false), &server.url).await;
        let err = crate::cli::engine_cmd::fetch_model_token(&paths)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("engine_sign_in_required"), "{err}");
        assert!(server.requests.lock().unwrap().is_empty());
    }

    // ---- Local chat on the daemon's engine profile (design §12.1) -------

    use crate::config::schema::{AgentSection, DaemonConfig, RunnerConfig, WorkspaceSection};
    use crate::ipc::engine_chat::EngineChatProfile;

    /// A fake `pidash-agent-engine app-server`: records the folder it was
    /// started in under `$CODEX_HOME/cwd`, then answers one turn.
    const FAKE_ENGINE: &str = r#"#!/bin/sh
pwd -P > "$CODEX_HOME/cwd"
read _
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
read _
read _
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_engine"}}'
read _
printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"hi from the engine"}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","summary":"done"}}}'
sleep 1
"#;

    struct ChatDaemon {
        tmp: tempfile::TempDir,
        paths: crate::util::paths::Paths,
        insts: Vec<RunnerInstance>,
        codex_home: PathBuf,
    }

    fn task_runner(tmp: &std::path::Path, name: &str) -> RunnerConfig {
        let dir = tmp.join("tasks").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        RunnerConfig {
            name: name.into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some(name.to_uppercase()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir: dir },
            agent: AgentSection::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    /// A daemon hosting `runners` (offline), whose engine profile launches
    /// [`FAKE_ENGINE`]. Nothing outside the temp dir is touched.
    async fn chat_daemon(runners: &[&str]) -> ChatDaemon {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("c"),
            data_dir: tmp.path().join("d"),
            runtime_dir: tmp.path().join("r"),
            machine_context_dir: tmp.path().join("c"),
        };
        let blocks: Vec<RunnerConfig> =
            runners.iter().map(|n| task_runner(tmp.path(), n)).collect();
        crate::config::file::write_config(
            &paths,
            &Config {
                version: 2,
                daemon: Default::default(),
                runners: blocks.clone(),
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            },
        )
        .unwrap();
        let insts: Vec<RunnerInstance> = blocks
            .iter()
            .map(|b| RunnerInstance::new_offline(b.clone(), &paths, DaemonConfig::default()))
            .collect();
        let engine = tmp.path().join("engine/0.153.4/pidash-agent-engine");
        std::fs::create_dir_all(engine.parent().unwrap()).unwrap();
        std::fs::write(&engine, FAKE_ENGINE).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let codex_home = tmp.path().join("engine-home");
        std::fs::create_dir_all(&codex_home).unwrap();

        let mut spawn_ctx = RunnerSpawnCtx::for_tests(
            None,
            WorkspaceTokens::default(),
            &paths,
            &insts.iter().collect::<Vec<_>>(),
        );
        spawn_ctx.local_chat = Arc::new(EngineChatProfile::for_tests(engine, codex_home.clone()));
        let server = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: crate::daemon::state::StateHandle::new(Config {
                version: 2,
                daemon: Default::default(),
                runners: vec![],
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            }),
            paths: paths.clone(),
            instances: Arc::new(std::sync::RwLock::new(
                insts.iter().map(|i| (i.runner_id, i.clone())).collect(),
            )),
            spawn_ctx,
        };
        tokio::spawn(async move {
            let _ = server.run().await;
        });
        for _ in 0..200 {
            if paths.ipc_socket_path().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        ChatDaemon {
            tmp,
            paths,
            insts,
            codex_home,
        }
    }

    /// Send one `ChatSend` with no runner selector unless `runner` is
    /// given, and return every frame up to the terminal one.
    async fn chat_send(
        d: &ChatDaemon,
        cwd: &std::path::Path,
        runner: Option<&str>,
    ) -> Vec<Response> {
        let mut client = crate::ipc::client::Client::connect(d.paths.ipc_socket_path())
            .await
            .unwrap();
        let first = client
            .call(Request::ChatSend {
                chat_session_id: Uuid::new_v4(),
                message_id: Uuid::new_v4(),
                content: "hello".into(),
                runner: runner.map(str::to_string),
                cwd: Some(cwd.display().to_string()),
                model: None,
                mode: None,
                local_thread_id: None,
                local_session_id: None,
            })
            .await
            .unwrap();
        let mut frames = vec![first];
        let terminal = |f: &Response| {
            matches!(
                f,
                Response::ChatMessageCompleted { .. }
                    | Response::ChatFailed { .. }
                    | Response::Error(_)
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while !terminal(frames.last().unwrap()) {
                match client.read_next().await.unwrap() {
                    Some(f) => frames.push(f),
                    None => break,
                }
            }
        })
        .await
        .expect("the chat turn must end");
        frames
    }

    fn completed(frames: &[Response]) -> bool {
        matches!(
            frames.last(),
            Some(Response::ChatMessageCompleted { status, .. }) if status == "completed"
        ) && frames.iter().any(|f| {
            matches!(
                f,
                Response::ChatEvent { kind, .. } if kind == "assistant_delta"
            )
        })
    }

    fn chat_folder(d: &ChatDaemon) -> PathBuf {
        let dir = d.tmp.path().join("chat/acct/workdir");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn selector_less_chat_runs_on_the_engine_profile_with_no_runner() {
        let d = chat_daemon(&[]).await;
        let folder = chat_folder(&d);
        let frames = chat_send(&d, &folder, None).await;
        assert!(completed(&frames), "frames: {frames:?}");
        // The engine ran in the chat folder with the profile's CODEX_HOME.
        let ran_in = std::fs::read_to_string(d.codex_home.join("cwd")).unwrap();
        assert_eq!(
            ran_in.trim(),
            std::fs::canonicalize(&folder)
                .unwrap()
                .display()
                .to_string()
        );
    }

    #[tokio::test]
    async fn selector_less_chat_runs_on_the_engine_profile_with_two_runners() {
        let d = chat_daemon(&["web", "api"]).await;
        let folder = chat_folder(&d);
        let frames = chat_send(&d, &folder, None).await;
        assert!(completed(&frames), "frames: {frames:?}");
        // No runner was claimed: neither runner's busy flag was touched.
        for inst in &d.insts {
            assert!(!inst.chat_active.load(std::sync::atomic::Ordering::SeqCst));
        }
        // With a selector the runner path is kept: an unknown name is an
        // error naming the configured runners, as before.
        let frames = chat_send(&d, &folder, Some("nope")).await;
        match frames.last() {
            Some(Response::Error(e)) => assert!(e.message.contains("no runner named"), "{e:?}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn engine_chat_refuses_a_folder_inside_a_runner_working_dir() {
        let d = chat_daemon(&["web"]).await;
        let inside = d.insts[0].config.workspace.working_dir.join("src");
        std::fs::create_dir_all(&inside).unwrap();
        for folder in [
            inside.clone(),
            d.insts[0].config.workspace.working_dir.clone(),
        ] {
            let frames = chat_send(&d, &folder, None).await;
            match frames.last() {
                Some(Response::ChatFailed { code, detail, .. }) => {
                    assert_eq!(code, "engine_start_failed");
                    let detail = detail.as_deref().unwrap_or_default();
                    assert!(detail.contains("inside runner \"web\""), "{detail}");
                }
                other => panic!("expected ChatFailed, got {other:?}"),
            }
        }
        assert!(
            !d.codex_home.join("cwd").exists(),
            "the engine never started in a task folder"
        );
    }

    #[tokio::test]
    async fn engine_chat_is_not_blocked_by_a_running_run() {
        use pidash_ipc::protocol::CurrentRunSummary;
        let d = chat_daemon(&["web"]).await;
        d.insts[0]
            .state
            .set_current_run(Some(CurrentRunSummary {
                run_id: Uuid::new_v4(),
                thread_id: None,
                status: "running".into(),
                started_at: chrono::Utc::now(),
                events: 0,
            }))
            .await;
        let folder = chat_folder(&d);
        // Chat on the runner itself still yields to its run (AC6)...
        let frames = chat_send(&d, &folder, Some("web")).await;
        assert!(
            matches!(frames.last(), Some(Response::ChatFailed { code, .. }) if code == "runner_busy"),
            "frames: {frames:?}"
        );
        // ...but chat on the engine profile does not wait for it.
        let frames = chat_send(&d, &folder, None).await;
        assert!(completed(&frames), "frames: {frames:?}");
    }

    #[tokio::test]
    async fn hello_and_engine_status_report_the_engine_pause_state() {
        let machine_id = Uuid::new_v4();
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(req.method, "PATCH");
            assert_eq!(req.bearer(), Some("mt_acme"));
            let paused = req.json()["engine_paused_by_owner"].as_bool().unwrap();
            let body = serde_json::json!({
                "engine_paused_by_owner": paused,
                "engine_auto_pause_reason": "",
            });
            (200, body.to_string())
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("c"),
            data_dir: tmp.path().join("d"),
            runtime_dir: tmp.path().join("r"),
            machine_context_dir: tmp.path().join("c"),
        };
        let transport = crate::cloud::http::SharedHttpTransport::new(server.url.clone()).unwrap();
        let spawn_ctx = RunnerSpawnCtx::for_tests_with_daemon(
            Some(transport),
            WorkspaceTokens::from_cli(Some(&cli(true))),
            &paths,
            DaemonConfig {
                cloud_url: server.url.clone(),
                dev_machine_id: Some(machine_id),
                ..DaemonConfig::default()
            },
            &[],
        );
        // Unknown until the server says.
        assert_eq!(spawn_ctx.hello().engine_detail.unwrap().paused(), None);
        // The automatic reason, as the session welcome carries it.
        spawn_ctx.engine.note_machine_payload(
            &serde_json::from_value(serde_json::json!({
                "engine_paused_by_owner": false,
                "engine_auto_pause_reason": "wallet_empty",
            }))
            .unwrap(),
        );
        let detail = spawn_ctx.hello().engine_detail.unwrap();
        assert_eq!(detail.paused_by_owner, Some(false));
        assert_eq!(detail.auto_pause_reason.as_deref(), Some("wallet_empty"));
        let ipc = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: crate::daemon::state::StateHandle::new(Config {
                version: 2,
                daemon: Default::default(),
                runners: vec![],
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            }),
            paths: paths.clone(),
            instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            spawn_ctx,
        };
        tokio::spawn(async move {
            let _ = ipc.run().await;
        });
        for _ in 0..200 {
            if paths.ipc_socket_path().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let mut client = crate::ipc::client::Client::connect(paths.ipc_socket_path())
            .await
            .unwrap();
        // `pidash engine pause` through the daemon: the answer is cached.
        match client
            .call(Request::EngineSetPaused { paused: true })
            .await
            .unwrap()
        {
            Response::EngineStatus(st) => {
                assert_eq!(st.paused_by_owner, Some(true));
                assert_eq!(st.auto_pause_reason, None, "the server cleared it");
            }
            other => panic!("unexpected {other:?}"),
        }
        match client.call(Request::Hello).await.unwrap() {
            Response::Hello(h) => {
                let d = h.engine_detail.unwrap();
                assert_eq!(d.paused_by_owner, Some(true));
                assert_eq!(d.paused(), Some(true));
            }
            other => panic!("unexpected {other:?}"),
        }
        match client.call(Request::EngineStatus).await.unwrap() {
            Response::EngineStatus(st) => assert_eq!(st.paused_by_owner, Some(true)),
            other => panic!("unexpected {other:?}"),
        }
        let sent = server.requests_to(&crate::engine::api::dev_machine_path(machine_id));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].json()["engine_paused_by_owner"], true);
    }

    // ---- ConfigGet redaction (PIDESKAPP-220) ---------------------------

    fn config_with_secrets(slug: &str) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig::default(),
            runners: vec![RunnerConfig {
                name: "web".into(),
                runner_id: Uuid::new_v4(),
                workspace_slug: Some(slug.into()),
                project_slug: Some("TEST".into()),
                pod_id: None,
                workspace: WorkspaceSection {
                    working_dir: std::path::PathBuf::from("/tmp/wd"),
                },
                agent: AgentSection::default(),
                codex: Default::default(),
                claude_code: Default::default(),
                cursor_agent: Default::default(),
                openclaw: Default::default(),
                grok: Default::default(),
                muse_code: Default::default(),
                approval_policy: Default::default(),
            }],
            cli: Some(CliSection {
                token: Some("mt_default_secret".into()),
                workspace_slug: Some("acme".into()),
                workspaces: vec![
                    WorkspaceCredential {
                        slug: "beta".into(),
                        token: "mt_beta_secret".into(),
                    },
                    WorkspaceCredential {
                        slug: "gamma".into(),
                        token: "mt_gamma_secret".into(),
                    },
                ],
                engine_credential: Some(EngineCredential {
                    token: "ec_secret_live".into(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(10)),
                }),
                ..Default::default()
            }),
            engine: Default::default(),
            auth: Default::default(),
        }
    }

    #[test]
    fn redacted_clears_every_workspace_and_engine_token() {
        let cfg = config_with_secrets("acme");
        let redacted = cfg.redacted();
        let json = serde_json::to_value(&redacted).unwrap();
        let text = json.to_string();
        assert!(
            !text.contains("mt_default_secret"),
            "cli.token leaked: {text}"
        );
        assert!(
            !text.contains("mt_beta_secret"),
            "beta token leaked: {text}"
        );
        assert!(
            !text.contains("mt_gamma_secret"),
            "gamma token leaked: {text}"
        );
        assert!(
            !text.contains("ec_secret_live"),
            "engine credential leaked: {text}"
        );
        // Non-secret fields survive for desktop daemon_runners.
        assert!(text.contains("acme"), "workspace slug lost: {text}");
        assert!(text.contains("beta"), "workspace slug lost: {text}");
        assert!(text.contains("web"), "runner lost: {text}");
        assert!(json["runner"].is_array(), "runner blocks lost: {json}");
        // Original is untouched.
        assert_eq!(
            cfg.cli.as_ref().unwrap().token.as_deref(),
            Some("mt_default_secret")
        );
    }

    #[test]
    fn strip_secrets_from_patch_removes_tokens_but_keeps_other_keys() {
        let mut patch = serde_json::json!({
            "cli": {
                "token": "mt_evil",
                "workspace_slug": "hacked",
                "workspaces": [{"slug": "beta", "token": "mt_beta"}],
                "engine_credential": {"token": "ec_evil"}
            },
            "daemon": {"log_level": "debug"},
            "runner": [{"name": "web", "model_token_file": "/tmp/file", "token": "runner_token"}]
        });
        strip_secrets_from_patch(&mut patch);
        assert_eq!(patch["daemon"]["log_level"], "debug");
        assert_eq!(patch["cli"]["workspace_slug"], "hacked");
        assert!(
            patch["cli"].get("token").is_none(),
            "cli.token not stripped"
        );
        assert!(patch["cli"]["workspaces"][0].get("token").is_none());
        assert!(patch["cli"]["engine_credential"].get("token").is_none());
        // model_token_file is a path, not a secret — must survive.
        assert_eq!(patch["runner"][0]["model_token_file"], "/tmp/file");
        // Exact-key match: `token` removed even under runner.
        assert!(patch["runner"][0].get("token").is_none());
    }

    #[test]
    fn redact_json_for_ipc_clears_future_token_fields() {
        // Simulates a future per-runner token field added to Config without
        // updating typed redaction: the JSON walk must still clear it.
        let mut json = serde_json::json!({
            "runner": [{"name": "web", "api_token": "future_secret"}],
            "cli": {"token": "mt_leak"}
        });
        redact_json_for_ipc(&mut json);
        assert_eq!(json["runner"][0]["api_token"], "");
        assert_eq!(json["cli"]["token"], "");
        // model_token_file untouched
        let mut json2 = serde_json::json!({"runner": [{"codex": {"model_token_file": "/tmp/p"}}]});
        redact_json_for_ipc(&mut json2);
        assert_eq!(json2["runner"][0]["codex"]["model_token_file"], "/tmp/p");
    }

    #[tokio::test]
    async fn config_get_over_ipc_never_returns_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("c"),
            data_dir: tmp.path().join("d"),
            runtime_dir: tmp.path().join("r"),
            machine_context_dir: tmp.path().join("c"),
        };
        let cfg = config_with_secrets("acme");
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let spawn_ctx = RunnerSpawnCtx::for_tests(None, WorkspaceTokens::default(), &paths, &[]);
        let server = IpcServer {
            path: paths.ipc_socket_path(),
            primary_state: crate::daemon::state::StateHandle::new(Config {
                version: 2,
                daemon: Default::default(),
                runners: vec![],
                cli: None,
                engine: Default::default(),
                auth: Default::default(),
            }),
            paths: paths.clone(),
            instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            spawn_ctx,
        };
        tokio::spawn(async move {
            let _ = server.run().await;
        });
        for _ in 0..200 {
            if paths.ipc_socket_path().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let mut client = crate::ipc::client::Client::connect(paths.ipc_socket_path())
            .await
            .unwrap();
        match client.call(Request::ConfigGet).await.unwrap() {
            Response::Config(v) => {
                let s = v.to_string();
                assert!(
                    !s.contains("mt_default_secret"),
                    "cli.token leaked over IPC: {s}"
                );
                assert!(
                    !s.contains("mt_beta_secret"),
                    "workspace token leaked over IPC: {s}"
                );
                assert!(
                    !s.contains("mt_gamma_secret"),
                    "workspace token leaked over IPC: {s}"
                );
                assert!(
                    !s.contains("ec_secret_live"),
                    "engine credential leaked over IPC: {s}"
                );
                // Non-secret fields still arrive.
                assert!(s.contains("acme"), "missing slug: {s}");
                assert!(s.contains("web"), "missing runner: {s}");
                // Runner blocks usable by desktop daemon_runners.
                let blocks = v["runner"].as_array().unwrap();
                assert_eq!(blocks.len(), 1);
                assert_eq!(blocks[0]["name"], "web");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Round-trip: sending the redacted value back via ConfigUpdate must not clear disk tokens.
        let raw = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(
            raw.cli.as_ref().unwrap().token.as_deref(),
            Some("mt_default_secret")
        );
        // Simulate client sending redacted JSON back as patch.
        let redacted_json = serde_json::to_value(raw.redacted()).unwrap();
        let mut client2 = crate::ipc::client::Client::connect(paths.ipc_socket_path())
            .await
            .unwrap();
        // Need a patch that would clear tokens if not stripped.
        let mut patch = redacted_json.clone();
        // Ensure patch contains explicit null/empty tokens (what a naive client might send).
        if let Some(cli) = patch.get_mut("cli").and_then(|v| v.as_object_mut()) {
            cli.insert("token".into(), serde_json::Value::Null);
        }
        match client2
            .call(Request::ConfigUpdate {
                patch,
                runner: None,
            })
            .await
            .unwrap()
        {
            Response::Ack => {}
            other => panic!("unexpected {other:?}"),
        }
        let after = crate::config::file::load_config(&paths).unwrap();
        assert_eq!(
            after.cli.as_ref().unwrap().token.as_deref(),
            Some("mt_default_secret"),
            "ConfigUpdate clobbered disk token via redacted round-trip"
        );
        assert_eq!(
            after.cli.as_ref().unwrap().workspaces[0].token,
            "mt_beta_secret",
            "ConfigUpdate clobbered workspace token"
        );
        assert_eq!(
            after
                .cli
                .as_ref()
                .unwrap()
                .engine_credential
                .as_ref()
                .unwrap()
                .token,
            "ec_secret_live"
        );
    }
}
