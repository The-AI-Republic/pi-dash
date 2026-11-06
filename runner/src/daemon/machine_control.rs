//! Per-dev-machine control session.
//!
//! Opens `POST /api/v1/runner/dev-machines/<mid>/sessions/` with any live
//! workspace `mt_` MachineToken (one session per machine, design §4.9)
//! and long-polls it for machine-scoped
//! control messages. This channel exists even when the machine hosts
//! zero runners — it is how the cloud's "Add runner" modal creates a
//! runner on a connected machine without the operator pasting a
//! `pidash runner add` command.
//!
//! Handled messages (cloud-side allowlist in
//! `services/machine_outbox.py`):
//! - `create_runner` — register with the cloud (X-Api-Key path), write
//!   the `[[runner]]` config block, hot-add the runner in-process, and
//!   report the outcome back so the web modal's status poll completes.
//! - `welcome` / `ping` / `config_push` — ack-only for now.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::cli::runner_ops::{ApplyEnrollOptions, apply_enroll_response};
use crate::cloud::http::{
    CreateRunnerRequest, MachineClient, PollMessage, SharedHttpTransport, TransportError,
    create_runner,
};
use crate::cloud::protocol::{CreateRunnerCmd, MachineMsg};
use crate::cloud::reconnect::{Backoff, jittered};
use crate::config::schema::AgentKind;
use crate::daemon::state::StateHandle;
use crate::daemon::supervisor::RunnerSpawnCtx;
use crate::daemon::workspace_tokens::WorkspaceTokens;

/// Server default; the open response's welcome frame can override it.
const DEFAULT_LONG_POLL_SECS: u64 = 25;
/// Bound on the in-memory duplicate-delivery guard.
const SEEN_CAP: usize = 256;

pub(crate) struct MachineControl {
    transport: SharedHttpTransport,
    dev_machine_id: Uuid,
    spawn_ctx: RunnerSpawnCtx,
    daemon_state: StateHandle,
}

impl MachineControl {
    pub(crate) fn new(
        transport: SharedHttpTransport,
        dev_machine_id: Uuid,
        spawn_ctx: RunnerSpawnCtx,
        daemon_state: StateHandle,
    ) -> Self {
        Self {
            transport,
            dev_machine_id,
            spawn_ctx,
            daemon_state,
        }
    }

    /// Session lifecycle: pick a live workspace token → open with backoff
    /// → poll until evicted or an auth error → reopen. Uses the same
    /// schedule as `HttpLoop` (PDASHOSS01-306): opens back off 30 s → 6 h,
    /// polls 1 s → 6 h, both jittered, reset on success or a reconnect kick
    /// (wake, network change, `pidash reconnect`).
    ///
    /// One session per machine, opened with any workspace's token (design
    /// §4.9). When the cloud answers that the session's token is revoked
    /// (or the user left that workspace), that token is marked revoked and
    /// the session is re-opened at once with another workspace's token.
    /// With no live token left the session waits until a sign-in adds one
    /// (`CredentialsReload`) instead of ending. Only a revoked dev machine
    /// ends it.
    pub async fn run(self) {
        let shutdown = self.daemon_state.shutdown_notified();
        let shutdown_fut = shutdown.notified();
        tokio::pin!(shutdown_fut);
        let kick = self.daemon_state.reconnect_notified();
        let tokens = self.spawn_ctx.tokens.clone();
        let tokens_changed = tokens.changed();

        // At-least-once delivery guard: a message handled but not yet
        // acked (crash / eviction between poll cycles) redelivers on the
        // next session. Executing `create_runner` twice would mint a
        // duplicate runner, so remember handled mids for this process's
        // lifetime. (A daemon restart clears it — acceptable, because
        // restart re-polls the PEL only for messages never acked, and
        // the cloud-side result key means the operator sees the first
        // outcome either way.)
        let mut seen_mids: HashSet<String> = HashSet::new();
        let mut open_backoff = Backoff::bootstrap();
        let mut poll_backoff = Backoff::poll();

        loop {
            let token_change = tokens_changed.notified();
            tokio::pin!(token_change);
            let Some((workspace, token)) = tokens.session_candidates().into_iter().next() else {
                tracing::warn!(
                    "machine control session idle: no workspace is signed in on this machine"
                );
                tokio::select! {
                    biased;
                    _ = &mut shutdown_fut => return,
                    _ = &mut token_change => {}
                    _ = kick.notified() => {}
                }
                continue;
            };
            let workspace_label = workspace.clone().unwrap_or_else(|| "(default)".to_string());
            let client = MachineClient::new(self.transport.clone(), self.dev_machine_id, token);

            let opened = tokio::select! {
                biased;
                _ = &mut shutdown_fut => return,
                r = client.open_session() => r,
            };
            let session_id = match opened {
                Ok(resp) => {
                    open_backoff.reset();
                    poll_backoff.reset();
                    // The welcome may carry the machine's engine pause
                    // state (design §8); cache it for Hello / status.
                    self.note_welcome(&resp.welcome);
                    tracing::info!(
                        session_id = %resp.session_id,
                        workspace = %workspace_label,
                        "machine control session open"
                    );
                    resp.session_id
                }
                Err(e) if is_token_fatal(&e) => {
                    tokens.mark_revoked(client.machine_token());
                    tracing::warn!(
                        workspace = %workspace_label,
                        "machine session token rejected ({e}); reopening with another workspace's token"
                    );
                    continue;
                }
                Err(e) => {
                    if is_machine_fatal(&e) {
                        tracing::error!("machine control session unrecoverable: {e}");
                        return;
                    }
                    let delay = jittered(open_backoff.next_base());
                    tracing::warn!(
                        "machine session open failed (retrying in {}s): {e}",
                        delay.as_secs()
                    );
                    tokio::select! {
                        biased;
                        _ = &mut shutdown_fut => return,
                        _ = kick.notified() => open_backoff.reset(),
                        _ = tokio::time::sleep(delay) => {}
                    }
                    continue;
                }
            };

            let mut acks: Vec<String> = Vec::new();
            'poll: loop {
                let polled = tokio::select! {
                    biased;
                    _ = &mut shutdown_fut => {
                        client.close_session(&session_id).await;
                        return;
                    }
                    r = client.poll_with(
                        &session_id,
                        crate::cloud::protocol::MachinePollBody {
                            ack: std::mem::take(&mut acks),
                            engine_state: Some((&self.spawn_ctx.engine.status()).into()),
                        },
                        DEFAULT_LONG_POLL_SECS,
                    ) => r,
                };
                match polled {
                    Ok(resp) => {
                        poll_backoff.reset();
                        for msg in resp.messages {
                            acks.push(msg.stream_id.clone());
                            if !msg.mid.is_empty() && !seen_mids.insert(msg.mid.clone()) {
                                tracing::debug!(mid = %msg.mid, "skipping duplicate machine message");
                                continue;
                            }
                            if seen_mids.len() > SEEN_CAP {
                                seen_mids.clear();
                            }
                            self.handle_message(&client, msg).await;
                        }
                    }
                    Err(TransportError::SessionEvicted { reason }) => {
                        tracing::info!("machine session evicted ({reason}); reopening");
                        break 'poll;
                    }
                    Err(e) if is_token_fatal(&e) => {
                        tokens.mark_revoked(client.machine_token());
                        tracing::warn!(
                            workspace = %workspace_label,
                            "machine session token revoked ({e}); reopening with another workspace's token"
                        );
                        break 'poll;
                    }
                    Err(e) => {
                        if is_machine_fatal(&e) {
                            tracing::error!("machine control session unrecoverable: {e}");
                            return;
                        }
                        let delay = jittered(poll_backoff.next_base());
                        tracing::warn!(
                            "machine poll failed (retrying in {}s): {e}",
                            delay.as_secs()
                        );
                        tokio::select! {
                            biased;
                            _ = &mut shutdown_fut => return,
                            _ = kick.notified() => poll_backoff.reset(),
                            _ = tokio::time::sleep(delay) => {}
                        }
                    }
                }
            }
        }
    }

    /// Cache the engine pause fields a welcome frame carries, if any.
    fn note_welcome(&self, welcome: &serde_json::Value) {
        if let Ok(payload) =
            serde_json::from_value::<crate::engine::api::MachinePayload>(welcome.clone())
        {
            self.spawn_ctx.engine.note_machine_payload(&payload);
        }
    }

    async fn handle_message(&self, client: &MachineClient, msg: PollMessage) {
        if msg.kind == "welcome" {
            self.note_welcome(&msg.body);
        }
        match serde_json::from_value::<MachineMsg>(msg.body) {
            Ok(MachineMsg::CreateRunner(cmd)) => self.handle_create_runner(client, cmd).await,
            Ok(MachineMsg::Welcome { .. }) | Ok(MachineMsg::Ping {}) => {}
            Ok(MachineMsg::ConfigPush {}) => {
                tracing::debug!("config_push received; not implemented yet — acked");
            }
            Err(e) => {
                tracing::warn!(kind = %msg.kind, "unparseable machine message (acked): {e}");
            }
        }
    }

    async fn handle_create_runner(&self, client: &MachineClient, cmd: CreateRunnerCmd) {
        let request_id = cmd.request_id.clone();
        tracing::info!(
            %request_id,
            project = %cmd.project,
            name = %cmd.name,
            "cloud-driven create_runner received"
        );
        let outcome = self.create_runner_inner(client, cmd).await;
        let body = match &outcome {
            Ok((runner_id, runner_name)) => serde_json::json!({
                "status": "ok",
                "runner_id": runner_id.to_string(),
                "runner_name": runner_name,
            }),
            Err(e) => serde_json::json!({
                "status": "error",
                "error": format!("{e:#}"),
            }),
        };
        if let Err(e) = &outcome {
            tracing::error!(%request_id, "create_runner failed: {e:#}");
        }
        if request_id.is_empty() {
            return;
        }
        if let Err(e) = client.post_command_result(&request_id, &body).await {
            tracing::warn!(%request_id, "posting command result failed: {e}");
        }
    }

    async fn create_runner_inner(
        &self,
        session: &MachineClient,
        cmd: CreateRunnerCmd,
    ) -> Result<(Uuid, String)> {
        if cmd.project.is_empty() {
            anyhow::bail!("create_runner command missing project");
        }
        let agent_kind = parse_agent_kind(&cmd.agent)?;
        let is_engine = agent_kind == AgentKind::Engine;
        // Held to the end: the engine reconcile must not see this row
        // before its block is written.
        let _engine_ops = if is_engine {
            Some(self.spawn_ctx.engine_ops_lock.lock().await)
        } else {
            None
        };
        // One engine runner per machine and project (design §5): when this
        // computer already runs the engine for the project, the command is
        // already satisfied. Answer with that runner instead of creating
        // (and possibly rolling back) another.
        if agent_kind == AgentKind::Engine
            && !cmd.workspace_slug.is_empty()
            && let Some(existing) =
                existing_engine_runner(&self.spawn_ctx.paths, &cmd.workspace_slug, &cmd.project)?
        {
            tracing::info!(
                runner_id = %existing.runner_id,
                project = %cmd.project,
                "create_runner: the engine is already on for this project; reusing it"
            );
            self.spawn_ctx
                .add_runner(existing.clone())
                .await
                .context("bringing up the existing engine runner")?;
            return Ok((existing.runner_id, existing.name));
        }
        if is_engine {
            // Same checks as `EngineRunnerAdd`, before the server is called.
            crate::engine::provision::ensure_engine_can_run(&self.spawn_ctx.engine.status())?;
        }
        // The per-computer runner limit fails before anything is registered
        // (design §5): a cloud row created first and rolled back on the
        // local `validate()` failure is left behind if the rollback fails.
        ensure_below_runner_cap(&self.spawn_ctx.paths)?;
        let host_label = hostname().unwrap_or_else(|| "unknown-host".to_string());

        // The command names its workspace; answer it with that workspace's
        // token (design §4.9), since the cloud binds a runner to its token's
        // workspace. A command with no workspace uses the session's token.
        let api_token = command_token(&self.spawn_ctx.tokens, session, &cmd.workspace_slug)?;

        // 1. Cloud registration — same X-Api-Key surface as
        //    `pidash runner add`.
        let before = crate::cli::runner_ops::runner_ids_before_create(&self.spawn_ctx.paths);
        let resp = create_runner(
            &self.transport,
            CreateRunnerRequest {
                api_token: &api_token,
                dev_machine_id: &self.dev_machine_id,
                workspace_slug: (!cmd.workspace_slug.is_empty())
                    .then_some(cmd.workspace_slug.as_str()),
                project: &cmd.project,
                host_label: &host_label,
                // The server names engine runners (design §5 "Naming").
                name: (!is_engine && !cmd.name.is_empty()).then_some(cmd.name.as_str()),
                pod: (!cmd.pod.is_empty()).then_some(cmd.pod.as_str()),
                agent_kind: is_engine.then_some(crate::engine::api::ENGINE_AGENT_KIND),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("cloud registration failed: {e}"))?;

        // 2. Persist the `[[runner]]` block. The runner gets one exclusive
        //    working directory: the caller's `working_dir` when supplied;
        //    otherwise, for an engine runner, a folder allocated under
        //    `[engine] task_root` (design §10), and for any other agent an
        //    auto-created path under the data dir. `add_runner` below brings
        //    it up on hot-add — no restart required.
        let cloud_url = self.spawn_ctx.cloud_url();
        let persisted = async {
            // The server returned a runner this computer already has: it is
            // the answer as it is, so no folder is allocated for it.
            let cfg = crate::config::file::load_config(&self.spawn_ctx.paths)?;
            if let Some(existing) = cfg.runners.iter().find(|r| r.runner_id == resp.runner_id) {
                return Ok(crate::cli::runner_ops::AppliedRunner {
                    runner: existing.clone(),
                    is_first_runner: false,
                    already_present: true,
                });
            }
            let working_dir = create_working_dir(&self.spawn_ctx.paths, &cmd, &resp, agent_kind)?;
            let options = ApplyEnrollOptions {
                working_dir,
                agent_kind,
                model: (!is_engine && !cmd.model.is_empty()).then_some(cmd.model.as_str()),
                reasoning_effort: (!is_engine && !cmd.reasoning_effort.is_empty())
                    .then_some(cmd.reasoning_effort.as_str()),
            };
            apply_enroll_response(&self.spawn_ctx.paths, &resp, &cloud_url, options).await
        };
        let applied = match persisted.await.context("persisting [[runner]] block") {
            Ok(applied) => applied,
            // The runner was already on this computer, or the server
            // returned one it already had: never roll back a working one.
            Err(persist_err)
                if !crate::cli::runner_ops::may_roll_back_create(&persist_err, &resp, &before) =>
            {
                return Err(persist_err);
            }
            Err(persist_err) => {
                // Roll the cloud-side row back so a failed local write
                // doesn't leave an orphan runner (mirrors `pidash
                // runner add`'s rollback umbrella).
                if let Err(rollback_err) = crate::cloud::runners::delete_runner(
                    &cloud_url,
                    &api_token,
                    &resp.runner_id,
                    false,
                )
                .await
                {
                    tracing::warn!(
                        runner_id = %resp.runner_id,
                        "rollback of cloud runner after local failure also failed: {rollback_err:#}"
                    );
                }
                return Err(persist_err);
            }
        };

        // 3. Hot-add the runner in-process — no daemon restart.
        self.spawn_ctx
            .add_runner(applied.runner.clone())
            .await
            .context("hot-adding runner to running daemon")?;

        Ok((resp.runner_id, resp.runner_name))
    }
}

/// The working dir of a runner created by machine control: the command's,
/// else (engine) `<task_root>/<workspace-slug>/<project-identifier>/` with a
/// numeric suffix on a clash (design §10), else `None` — the generic
/// default under the data dir.
fn create_working_dir(
    paths: &crate::util::paths::Paths,
    cmd: &CreateRunnerCmd,
    resp: &crate::cloud::http::EnrollResponse,
    agent_kind: AgentKind,
) -> Result<Option<PathBuf>> {
    if !cmd.working_dir.is_empty() {
        return Ok(Some(PathBuf::from(&cmd.working_dir)));
    }
    if agent_kind != AgentKind::Engine {
        return Ok(None);
    }
    let workspace = if resp.workspace_slug.is_empty() {
        cmd.workspace_slug.as_str()
    } else {
        resp.workspace_slug.as_str()
    };
    let project = if resp.project_identifier.is_empty() {
        cmd.project.as_str()
    } else {
        resp.project_identifier.as_str()
    };
    crate::engine::workdir::allocate_engine_folder(paths, workspace, project).map(Some)
}

/// The engine runner this computer already has for `workspace`/`project`.
fn existing_engine_runner(
    paths: &crate::util::paths::Paths,
    workspace: &str,
    project: &str,
) -> Result<Option<crate::config::schema::RunnerConfig>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = crate::config::file::load_config(paths)?;
    Ok(cfg.runners.into_iter().find(|r| {
        r.agent.kind == AgentKind::Engine
            && r.workspace_slug.as_deref() == Some(workspace)
            && r.project_slug
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(project))
    }))
}

/// Kebab-case wire value → AgentKind, via the same clap ValueEnum
/// parser `--agent` uses, so web and CLI accept identical spellings.
fn parse_agent_kind(raw: &str) -> Result<AgentKind> {
    if raw.is_empty() {
        return Ok(AgentKind::default());
    }
    <AgentKind as clap::ValueEnum>::from_str(raw, false)
        .map_err(|_| anyhow::anyhow!("unknown agent kind {raw:?}"))
}

/// The token a machine-control command runs with: its workspace's, or
/// the session's when it names none.
fn command_token(
    tokens: &WorkspaceTokens,
    session: &MachineClient,
    workspace_slug: &str,
) -> Result<String> {
    if workspace_slug.is_empty() {
        return Ok(session.machine_token().to_string());
    }
    tokens.token_for(Some(workspace_slug)).ok_or_else(|| {
        anyhow::anyhow!(
            "workspace {workspace_slug:?} is not signed in on this machine — run `pidash auth login --workspace {workspace_slug}` on it"
        )
    })
}

/// The session's own token is dead (revoked, or the user left its
/// workspace): another workspace's token may still open a session.
fn is_token_fatal(e: &TransportError) -> bool {
    matches!(
        e,
        TransportError::MachineTokenRevoked
            | TransportError::MachineTokenExpired
            | TransportError::MembershipRevoked
    )
}

/// The machine itself is gone: no token can open a session for it.
fn is_machine_fatal(e: &TransportError) -> bool {
    matches!(e, TransportError::DevMachineRevoked)
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Refuse a new runner when this daemon already has
/// [`MAX_RUNNERS_PER_DAEMON`] configured. Local and cheap, so it runs
/// before any network call.
fn ensure_below_runner_cap(paths: &crate::util::paths::Paths) -> Result<()> {
    let existing = if paths.config_path().exists() {
        crate::config::file::load_config(paths)?.runners.len()
    } else {
        0
    };
    if existing >= crate::config::schema::MAX_RUNNERS_PER_DAEMON {
        anyhow::bail!(
            "this computer already has the maximum of {} runners; remove one first",
            crate::config::schema::MAX_RUNNERS_PER_DAEMON
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design §5 / PIDESKAPP-118: the cloud-driven add fails on the
    /// 50-runner limit before anything is registered on the server.
    #[test]
    fn runner_cap_is_checked_before_registration() {
        use crate::config::schema::*;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        paths.ensure().unwrap();
        ensure_below_runner_cap(&paths).expect("no config yet");
        let runner = |i: usize| RunnerConfig {
            name: format!("r{i}"),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("WEB".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: tmp.path().join(format!("w{i}")),
            },
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            openclaw: OpenClawSection::default(),
            grok: GrokSection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        };
        let mut cfg = Config {
            version: 2,
            daemon: Default::default(),
            runners: (0..MAX_RUNNERS_PER_DAEMON - 1).map(runner).collect(),
            cli: None,
            engine: Default::default(),
            auth: Default::default(),
        };
        crate::config::file::write_config(&paths, &cfg).unwrap();
        ensure_below_runner_cap(&paths).expect("one slot left");
        cfg.runners.push(runner(MAX_RUNNERS_PER_DAEMON));
        crate::config::file::write_config(&paths, &cfg).unwrap();
        let err = ensure_below_runner_cap(&paths).unwrap_err().to_string();
        assert!(err.contains("maximum of 50 runners"), "{err}");
    }

    #[test]
    fn parses_kebab_case_agent_kinds() {
        assert_eq!(
            parse_agent_kind("claude-code").unwrap(),
            AgentKind::ClaudeCode
        );
        assert_eq!(parse_agent_kind("codex").unwrap(), AgentKind::Codex);
        assert_eq!(
            parse_agent_kind("cursor-agent").unwrap(),
            AgentKind::CursorAgent
        );
        assert_eq!(parse_agent_kind("open-claw").unwrap(), AgentKind::OpenClaw);
        assert_eq!(parse_agent_kind("grok").unwrap(), AgentKind::Grok);
        assert_eq!(parse_agent_kind("muse-code").unwrap(), AgentKind::MuseCode);
        assert_eq!(parse_agent_kind("").unwrap(), AgentKind::default());
        assert!(parse_agent_kind("skynet").is_err());
    }

    #[test]
    fn create_runner_cmd_parses_wire_payload() {
        let body = serde_json::json!({
            "type": "create_runner",
            "request_id": "req-1",
            "workspace_slug": "acme",
            "project": "PROJ",
            "pod": "",
            "name": "my_runner",
            "working_dir": "/tmp/proj",
            "agent": "claude-code",
            "model": "claude-opus-4-8",
            "reasoning_effort": "",
            "mid": "extra-fields-ignored"
        });
        let parsed: MachineMsg = serde_json::from_value(body).unwrap();
        match parsed {
            MachineMsg::CreateRunner(cmd) => {
                assert_eq!(cmd.request_id, "req-1");
                assert_eq!(cmd.project, "PROJ");
                assert_eq!(cmd.agent, "claude-code");
                assert_eq!(cmd.model, "claude-opus-4-8");
                assert!(cmd.pod.is_empty());
            }
            other => panic!("expected CreateRunner, got {other:?}"),
        }
    }

    #[test]
    fn ping_and_welcome_parse_as_ack_only() {
        let ping: MachineMsg = serde_json::from_value(serde_json::json!({"type": "ping"})).unwrap();
        assert!(matches!(ping, MachineMsg::Ping {}));
        let welcome: MachineMsg = serde_json::from_value(
            serde_json::json!({"type": "welcome", "dev_machine_id": "1e6a7a56-0000-0000-0000-000000000001"}),
        )
        .unwrap();
        assert!(matches!(welcome, MachineMsg::Welcome { .. }));
    }

    // ---- One session per machine, any workspace's token (§4.9) ---------

    use crate::config::schema::{CliSection, WorkspaceCredential};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Minimal fake of the machine-session endpoints. `revoked` tokens get
    /// `401 machine_token_revoked` on open; any other token opens a session
    /// whose polls return no messages. Records `(method path, bearer)`.
    async fn fake_machine_cloud(
        revoked: &'static [&'static str],
        error_code: &'static str,
    ) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let seen_srv = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = seen_srv.clone();
                tokio::spawn(async move {
                    loop {
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let head_end = loop {
                            let Ok(n) = socket.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i;
                            }
                        };
                        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                        let content_length = head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        while buf.len() < head_end + 4 + content_length {
                            let Ok(n) = socket.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        let request_line = head.lines().next().unwrap_or_default().to_string();
                        let bearer = head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("authorization")
                                    .then(|| v.trim().trim_start_matches("Bearer ").to_string())
                            })
                            .unwrap_or_default();
                        let mut parts = request_line.split_whitespace();
                        let method = parts.next().unwrap_or_default().to_string();
                        let path = parts.next().unwrap_or_default().to_string();
                        seen.lock()
                            .unwrap()
                            .push((format!("{method} {path}"), bearer.clone()));
                        let (status, body) = if path.ends_with("/sessions/") && method == "POST" {
                            if revoked.contains(&bearer.as_str()) {
                                (
                                    "401 Unauthorized",
                                    serde_json::json!({"detail": error_code}).to_string(),
                                )
                            } else {
                                (
                                    "201 Created",
                                    format!(
                                        r#"{{"session_id":"{}","welcome":{{}}}}"#,
                                        Uuid::new_v4()
                                    ),
                                )
                            }
                        } else if path.ends_with("/poll") {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                            ("200 OK", r#"{"messages":[]}"#.to_string())
                        } else {
                            ("200 OK", "{}".to_string())
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if socket.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn opens(seen: &Mutex<Vec<(String, String)>>) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(req, _)| req.starts_with("POST") && req.ends_with("/sessions/"))
            .map(|(_, bearer)| bearer.clone())
            .collect()
    }

    #[tokio::test]
    async fn session_reopens_with_another_workspace_token_when_one_is_revoked() {
        assert_session_reopens("machine_token_revoked").await;
    }

    #[tokio::test]
    async fn session_reopens_with_another_workspace_token_when_one_is_expired() {
        assert_session_reopens("machine_token_expired").await;
    }

    async fn assert_session_reopens(error_code: &'static str) {
        let (url, seen) = fake_machine_cloud(&["mt_acme"], error_code).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        let tokens = WorkspaceTokens::from_cli(Some(&CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            workspaces: vec![WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            }],
            ..Default::default()
        }));
        let transport = SharedHttpTransport::new(url).unwrap();
        let ctx = RunnerSpawnCtx::for_tests(Some(transport.clone()), tokens.clone(), &paths, &[]);
        let daemon_state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        let control = MachineControl::new(transport, Uuid::new_v4(), ctx, daemon_state.clone());
        let handle = tokio::spawn(control.run());

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !opens(&seen).contains(&"mt_beta".to_string()) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session must reopen with the other workspace's token");

        assert_eq!(
            opens(&seen),
            vec!["mt_acme".to_string(), "mt_beta".to_string()],
            "default token tried first, then the other workspace's at once"
        );
        assert_eq!(tokens.token_for(Some("acme")), None, "acme signed out");
        assert_eq!(tokens.token_for(Some("beta")).as_deref(), Some("mt_beta"));
        // Polls run with the session's (beta) token.
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .filter(|(req, _)| req.ends_with("/poll"))
                .all(|(_, bearer)| bearer == "mt_beta")
        );

        daemon_state.shutdown();
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("machine control stops on shutdown")
            .unwrap();
    }

    #[tokio::test]
    async fn session_waits_for_a_sign_in_when_every_token_is_revoked() {
        let (url, seen) = fake_machine_cloud(&["mt_acme"], "machine_token_revoked").await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        let only_acme = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        let tokens = WorkspaceTokens::from_cli(Some(&only_acme));
        let transport = SharedHttpTransport::new(url).unwrap();
        let ctx = RunnerSpawnCtx::for_tests(Some(transport.clone()), tokens.clone(), &paths, &[]);
        let daemon_state = StateHandle::new(crate::config::schema::Config {
            engine: Default::default(),
            auth: Default::default(),
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: None,
        });
        let control = MachineControl::new(transport, Uuid::new_v4(), ctx, daemon_state.clone());
        let handle = tokio::spawn(control.run());

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while tokens.token_for(Some("acme")).is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("revoked token marked");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            opens(&seen).len(),
            1,
            "no retry loop while nothing is signed in"
        );
        assert!(
            !handle.is_finished(),
            "the session waits rather than ending"
        );

        // A later `pidash auth login --workspace beta` + CredentialsReload.
        let mut signed_in = only_acme.clone();
        signed_in.set_workspace_token("beta", "mt_beta", false);
        tokens.reload(Some(&signed_in));
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !opens(&seen).contains(&"mt_beta".to_string()) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session opens with the newly signed-in workspace");

        daemon_state.shutdown();
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("machine control stops on shutdown")
            .unwrap();
    }

    /// Design §10 for machine control: an engine runner created from the
    /// web gets `<task_root>/<workspace>/<project>/` (numeric suffix on a
    /// clash), not the generic data-dir default, and is named by the server.
    #[tokio::test]
    async fn engine_runners_created_by_machine_control_get_a_task_root_folder() {
        use crate::config::schema::{Config, DaemonConfig, EngineCredential};
        use crate::engine::test_server::{self, Handler};
        let handler: Handler = Arc::new(|req| {
            if req.path == "/api/v1/runner/runners/" {
                let body = req.json();
                let project = body["project"].as_str().unwrap().to_string();
                let id = Uuid::new_v4();
                let resp = serde_json::json!({
                    "runner_id": id,
                    "runner_name": format!("builtin-h-acme-{}-{}", project.to_lowercase(), &id.to_string()[..4]),
                    "refresh_token": "", "access_token": "", "access_token_expires_at": "",
                    "refresh_token_generation": 0, "workspace_slug": "acme",
                    "project_identifier": project, "long_poll_interval_secs": 25,
                    "protocol_version": 4
                });
                (201, resp.to_string())
            } else {
                (404, String::new())
            }
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        paths.ensure().unwrap();
        let cli = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            engine_credential: Some(EngineCredential {
                token: "ec_live".into(),
                expires_at: None,
            }),
            ..Default::default()
        };
        let machine_id = Uuid::new_v4();
        let daemon = DaemonConfig {
            cloud_url: server.url.clone(),
            dev_machine_id: Some(machine_id),
            ..DaemonConfig::default()
        };
        crate::config::file::write_config(
            &paths,
            &Config {
                version: 2,
                daemon: daemon.clone(),
                runners: vec![],
                cli: Some(cli.clone()),
                engine: Default::default(),
                auth: Default::default(),
            },
        )
        .unwrap();
        let tokens = WorkspaceTokens::from_cli(Some(&cli));
        // Runners come up offline: only the mock is contacted.
        let mut ctx =
            RunnerSpawnCtx::for_tests_with_daemon(None, tokens.clone(), &paths, daemon, &[]);
        ctx.engine = crate::engine::manager::EngineManager::new(
            Some(&server.url),
            paths.clone(),
            tokens,
            Some(tmp.path().join("engine/0.153.4/pidash-agent-engine")),
        );
        let transport = SharedHttpTransport::new(server.url.clone()).unwrap();
        let control = MachineControl::new(
            transport.clone(),
            machine_id,
            ctx,
            StateHandle::new(crate::config::file::load_config(&paths).unwrap()),
        );
        let session = MachineClient::new(transport, machine_id, "mt_acme".into());
        let cmd = |name: &str| CreateRunnerCmd {
            request_id: String::new(),
            workspace_slug: "acme".into(),
            project: "WEB".into(),
            pod: String::new(),
            name: name.into(),
            working_dir: String::new(),
            agent: "engine".into(),
            model: String::new(),
            reasoning_effort: String::new(),
        };

        // A folder already in use under the task root is not taken over:
        // the new runner gets the next free one (design §10).
        let root = paths.data_dir.join("engine/workdirs/acme");
        std::fs::create_dir_all(root.join("WEB")).unwrap();
        std::fs::write(root.join("WEB/notes.txt"), "someone else's").unwrap();

        let (first, _) = control
            .create_runner_inner(&session, cmd("ignored-name"))
            .await
            .unwrap();
        // One engine runner per machine and project (design §5): a second
        // command for the same project answers with the first runner and
        // registers nothing.
        let (second, _) = control
            .create_runner_inner(&session, cmd(""))
            .await
            .unwrap();
        assert_eq!(second, first);

        let cfg = crate::config::file::load_config(&paths).unwrap();
        let dir_of = |id: Uuid| {
            cfg.runners
                .iter()
                .find(|r| r.runner_id == id)
                .unwrap()
                .workspace
                .working_dir
                .clone()
        };
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(dir_of(first), root.join("WEB-2"));
        assert!(root.join("WEB-2").is_dir());
        let sent = server.requests_to("/api/v1/runner/runners/");
        assert_eq!(sent.len(), 1, "the reused runner is not registered again");
        assert_eq!(sent[0].json()["agent_kind"], "engine");
        assert!(
            sent[0].json().get("name").is_none(),
            "the server names engine runners"
        );
    }

    #[test]
    fn non_engine_runners_keep_the_generic_default_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        let cmd = CreateRunnerCmd {
            request_id: String::new(),
            workspace_slug: "acme".into(),
            project: "WEB".into(),
            pod: String::new(),
            name: String::new(),
            working_dir: String::new(),
            agent: "claude-code".into(),
            model: String::new(),
            reasoning_effort: String::new(),
        };
        let resp: crate::cloud::http::EnrollResponse = serde_json::from_value(serde_json::json!({
            "runner_id": Uuid::new_v4(), "runner_name": "r", "refresh_token": "",
            "access_token": "", "access_token_expires_at": "", "refresh_token_generation": 0,
            "workspace_slug": "acme", "project_identifier": "WEB",
            "long_poll_interval_secs": 25, "protocol_version": 4
        }))
        .unwrap();
        assert_eq!(
            create_working_dir(&paths, &cmd, &resp, AgentKind::ClaudeCode).unwrap(),
            None
        );
        let explicit = CreateRunnerCmd {
            working_dir: "/srv/web".into(),
            ..cmd
        };
        assert_eq!(
            create_working_dir(&paths, &explicit, &resp, AgentKind::Engine).unwrap(),
            Some(PathBuf::from("/srv/web"))
        );
    }

    #[test]
    fn commands_use_the_token_of_the_workspace_they_name() {
        let tokens = WorkspaceTokens::from_cli(Some(&CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            workspaces: vec![WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            }],
            ..Default::default()
        }));
        let session = MachineClient::new(
            SharedHttpTransport::new("http://127.0.0.1:1".into()).unwrap(),
            Uuid::new_v4(),
            "mt_acme".into(),
        );
        assert_eq!(command_token(&tokens, &session, "beta").unwrap(), "mt_beta");
        assert_eq!(command_token(&tokens, &session, "acme").unwrap(), "mt_acme");
        assert_eq!(command_token(&tokens, &session, "").unwrap(), "mt_acme");
        assert!(command_token(&tokens, &session, "gamma").is_err());
    }

    #[tokio::test]
    async fn an_engine_already_on_for_the_project_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::util::paths::Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        assert!(
            existing_engine_runner(&paths, "acme", "WEB")
                .unwrap()
                .is_none()
        );
        let resp = crate::cloud::http::EnrollResponse {
            runner_id: Uuid::new_v4(),
            runner_name: "builtin-acme-web".into(),
            refresh_token: "rt".into(),
            access_token: String::new(),
            access_token_expires_at: String::new(),
            refresh_token_generation: 1,
            workspace_slug: "acme".into(),
            pod_slug: String::new(),
            project_identifier: "WEB".into(),
            long_poll_interval_secs: 25,
            protocol_version: 4,
            machine_token: None,
            machine_token_minted: false,
            created: None,
        };
        apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            ApplyEnrollOptions {
                working_dir: Some(tmp.path().join("wd")),
                agent_kind: AgentKind::Engine,
                model: None,
                reasoning_effort: None,
            },
        )
        .await
        .unwrap();
        let found = existing_engine_runner(&paths, "acme", "web")
            .unwrap()
            .unwrap();
        assert_eq!(found.runner_id, resp.runner_id);
        assert!(
            existing_engine_runner(&paths, "beta", "WEB")
                .unwrap()
                .is_none()
        );
        assert!(
            existing_engine_runner(&paths, "acme", "API")
                .unwrap()
                .is_none()
        );
    }
}
