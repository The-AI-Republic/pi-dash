//! Per-dev-machine control session.
//!
//! Opens `POST /api/v1/runner/dev-machines/<mid>/sessions/` with the
//! shared `mt_` MachineToken and long-polls it for machine-scoped
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
use std::time::Duration;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::cli::runner_ops::{ApplyEnrollOptions, RunnerWorkdirPlan, apply_enroll_response};
use crate::cloud::http::{
    CreateRunnerRequest, MachineClient, PollMessage, TransportError, create_runner,
};
use crate::cloud::protocol::{CreateRunnerCmd, MachineMsg};
use crate::config::schema::AgentKind;
use crate::daemon::state::StateHandle;
use crate::daemon::supervisor::RunnerSpawnCtx;

/// Server default; the open response's welcome frame can override it.
const DEFAULT_LONG_POLL_SECS: u64 = 25;
const BACKOFF_CAP_SECS: u64 = 30;
/// Bound on the in-memory duplicate-delivery guard.
const SEEN_CAP: usize = 256;

pub(crate) struct MachineControl {
    client: MachineClient,
    spawn_ctx: RunnerSpawnCtx,
    daemon_state: StateHandle,
}

impl MachineControl {
    pub(crate) fn new(
        client: MachineClient,
        spawn_ctx: RunnerSpawnCtx,
        daemon_state: StateHandle,
    ) -> Self {
        Self {
            client,
            spawn_ctx,
            daemon_state,
        }
    }

    /// Session lifecycle: open with backoff → poll until evicted or a
    /// fatal auth error → reopen. Mirrors `HttpLoop::run`'s inline
    /// 1→30s doubling with reset-on-success.
    pub async fn run(self) {
        let shutdown = self.daemon_state.shutdown_notified();
        let shutdown_fut = shutdown.notified();
        tokio::pin!(shutdown_fut);

        // At-least-once delivery guard: a message handled but not yet
        // acked (crash / eviction between poll cycles) redelivers on the
        // next session. Executing `create_runner` twice would mint a
        // duplicate runner, so remember handled mids for this process's
        // lifetime. (A daemon restart clears it — acceptable, because
        // restart re-polls the PEL only for messages never acked, and
        // the cloud-side result key means the operator sees the first
        // outcome either way.)
        let mut seen_mids: HashSet<String> = HashSet::new();
        let mut backoff_secs: u64 = 1;

        loop {
            let opened = tokio::select! {
                biased;
                _ = &mut shutdown_fut => return,
                r = self.client.open_session() => r,
            };
            let session_id = match opened {
                Ok(resp) => {
                    backoff_secs = 1;
                    tracing::info!(session_id = %resp.session_id, "machine control session open");
                    resp.session_id
                }
                Err(e) => {
                    if is_fatal(&e) {
                        tracing::error!("machine control session unrecoverable: {e}");
                        return;
                    }
                    tracing::warn!(
                        "machine session open failed (retrying in {backoff_secs}s): {e}"
                    );
                    tokio::select! {
                        biased;
                        _ = &mut shutdown_fut => return,
                        _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                    }
                    backoff_secs = (backoff_secs * 2).min(BACKOFF_CAP_SECS);
                    continue;
                }
            };

            let mut acks: Vec<String> = Vec::new();
            'poll: loop {
                let polled = tokio::select! {
                    biased;
                    _ = &mut shutdown_fut => {
                        self.client.close_session(&session_id).await;
                        return;
                    }
                    r = self.client.poll(&session_id, std::mem::take(&mut acks), DEFAULT_LONG_POLL_SECS) => r,
                };
                match polled {
                    Ok(resp) => {
                        backoff_secs = 1;
                        for msg in resp.messages {
                            acks.push(msg.stream_id.clone());
                            if !msg.mid.is_empty() && !seen_mids.insert(msg.mid.clone()) {
                                tracing::debug!(mid = %msg.mid, "skipping duplicate machine message");
                                continue;
                            }
                            if seen_mids.len() > SEEN_CAP {
                                seen_mids.clear();
                            }
                            self.handle_message(msg).await;
                        }
                    }
                    Err(TransportError::SessionEvicted { reason }) => {
                        tracing::info!("machine session evicted ({reason}); reopening");
                        break 'poll;
                    }
                    Err(e) => {
                        if is_fatal(&e) {
                            tracing::error!("machine control session unrecoverable: {e}");
                            return;
                        }
                        tracing::warn!("machine poll failed (retrying in {backoff_secs}s): {e}");
                        tokio::select! {
                            biased;
                            _ = &mut shutdown_fut => return,
                            _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                        }
                        backoff_secs = (backoff_secs * 2).min(BACKOFF_CAP_SECS);
                    }
                }
            }
        }
    }

    async fn handle_message(&self, msg: PollMessage) {
        match serde_json::from_value::<MachineMsg>(msg.body) {
            Ok(MachineMsg::CreateRunner(cmd)) => self.handle_create_runner(cmd).await,
            Ok(MachineMsg::Welcome { .. }) | Ok(MachineMsg::Ping {}) => {}
            Ok(MachineMsg::ConfigPush {}) => {
                tracing::debug!("config_push received; not implemented yet — acked");
            }
            Err(e) => {
                tracing::warn!(kind = %msg.kind, "unparseable machine message (acked): {e}");
            }
        }
    }

    async fn handle_create_runner(&self, cmd: CreateRunnerCmd) {
        let request_id = cmd.request_id.clone();
        tracing::info!(
            %request_id,
            project = %cmd.project,
            name = %cmd.name,
            "cloud-driven create_runner received"
        );
        let outcome = self.create_runner_inner(cmd).await;
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
        if let Err(e) = self.client.post_command_result(&request_id, &body).await {
            tracing::warn!(%request_id, "posting command result failed: {e}");
        }
    }

    async fn create_runner_inner(&self, cmd: CreateRunnerCmd) -> Result<(Uuid, String)> {
        if cmd.project.is_empty() {
            anyhow::bail!("create_runner command missing project");
        }
        let agent_kind = parse_agent_kind(&cmd.agent)?;
        let host_label = hostname().unwrap_or_else(|| "unknown-host".to_string());

        // 1. Cloud registration — same X-Api-Key surface as
        //    `pidash runner add`.
        let resp = create_runner(
            self.client.transport(),
            CreateRunnerRequest {
                api_token: self.client.machine_token(),
                dev_machine_id: &self.client.dev_machine_id(),
                workspace_slug: (!cmd.workspace_slug.is_empty())
                    .then_some(cmd.workspace_slug.as_str()),
                project: &cmd.project,
                host_label: &host_label,
                name: (!cmd.name.is_empty()).then_some(cmd.name.as_str()),
                pod: (!cmd.pod.is_empty()).then_some(cmd.pod.as_str()),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("cloud registration failed: {e}"))?;

        // 2. Persist the `[[runner]]` block. `Legacy` workdir plan —
        //    worktree pools are built once at daemon startup, so a
        //    hot-added runner can't join one until the next restart;
        //    running the agent directly in working_dir keeps config and
        //    runtime consistent. Operators can migrate via the CLI.
        let options = ApplyEnrollOptions {
            working_dir: (!cmd.working_dir.is_empty()).then(|| PathBuf::from(&cmd.working_dir)),
            agent_kind,
            model: (!cmd.model.is_empty()).then_some(cmd.model.as_str()),
            reasoning_effort: (!cmd.reasoning_effort.is_empty())
                .then_some(cmd.reasoning_effort.as_str()),
            workdir_plan: RunnerWorkdirPlan::Legacy,
        };
        let cloud_url = self.spawn_ctx.cloud_url();
        let applied = match apply_enroll_response(&self.spawn_ctx.paths, &resp, &cloud_url, options)
            .await
            .context("persisting [[runner]] block")
        {
            Ok(applied) => applied,
            Err(persist_err) => {
                // Roll the cloud-side row back so a failed local write
                // doesn't leave an orphan runner (mirrors `pidash
                // runner add`'s rollback umbrella).
                if let Err(rollback_err) = crate::cloud::runners::delete_runner(
                    &cloud_url,
                    self.client.machine_token(),
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

/// Kebab-case wire value → AgentKind, via the same clap ValueEnum
/// parser `--agent` uses, so web and CLI accept identical spellings.
fn parse_agent_kind(raw: &str) -> Result<AgentKind> {
    if raw.is_empty() {
        return Ok(AgentKind::default());
    }
    <AgentKind as clap::ValueEnum>::from_str(raw, false)
        .map_err(|_| anyhow::anyhow!("unknown agent kind {raw:?}"))
}

/// Errors that no amount of retrying fixes: the credential itself is
/// dead. Transient network / 5xx / timeout keep the retry loop alive.
fn is_fatal(e: &TransportError) -> bool {
    matches!(
        e,
        TransportError::MachineTokenRevoked
            | TransportError::DevMachineRevoked
            | TransportError::MembershipRevoked
    )
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
