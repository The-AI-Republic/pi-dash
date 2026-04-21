//! Agent abstraction layer. Both the Codex and Claude Code bridges expose
//! the same behavioural surface (`spawn`, `run`, `next_events`, `send_approval`,
//! `interrupt`, `shutdown`) through [`AgentBridge`], so the supervisor does
//! not have to know which underlying CLI is driving a run.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use crate::cloud::protocol::{ApprovalDecision, ApprovalKind, FailureReason};
use crate::config::schema::{AgentKind, Config};

/// Events the bridge surfaces to the daemon's state machine. Agent-agnostic:
/// Codex and Claude both translate their native protocols into this shape.
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    RunStarted {
        run_id: Uuid,
        thread_id: String,
    },
    Raw {
        run_id: Uuid,
        method: String,
        params: serde_json::Value,
    },
    ApprovalRequest {
        run_id: Uuid,
        approval_id: String,
        kind: ApprovalKind,
        payload: serde_json::Value,
        reason: Option<String>,
    },
    AwaitingReauth {
        run_id: Uuid,
        detail: Option<String>,
    },
    Completed {
        run_id: Uuid,
        done_payload: serde_json::Value,
    },
    Failed {
        run_id: Uuid,
        reason: FailureReason,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct RunPayload {
    pub run_id: Uuid,
    pub prompt: String,
    pub model: Option<String>,
    pub resume_thread_id: Option<String>,
}

/// Enum dispatch over the concrete bridges. Each variant owns the agent's
/// subprocess; the supervisor treats them uniformly.
pub enum AgentBridge {
    Codex(crate::codex::bridge::Bridge),
    ClaudeCode(crate::claude_code::bridge::Bridge),
}

/// Per-run cursor, paired with an `AgentBridge`. Holds agent-specific frame
/// translation state (sequence numbers, session ids).
pub enum AgentCursor {
    Codex(crate::codex::bridge::BridgeCursor),
    ClaudeCode(crate::claude_code::bridge::BridgeCursor),
}

impl AgentCursor {
    pub fn run_id(&self) -> Uuid {
        match self {
            AgentCursor::Codex(c) => c.run_id,
            AgentCursor::ClaudeCode(c) => c.run_id,
        }
    }

    pub fn thread_id(&self) -> &str {
        match self {
            AgentCursor::Codex(c) => &c.thread_id,
            AgentCursor::ClaudeCode(c) => &c.thread_id,
        }
    }
}

impl AgentBridge {
    /// Spawn the agent subprocess selected by the runner's config.
    pub async fn spawn_from_config(
        config: &Config,
        cwd: &Path,
        model_override: Option<String>,
    ) -> Result<Self> {
        match config.agent.kind {
            AgentKind::Codex => {
                let b = crate::codex::bridge::Bridge::spawn(
                    &config.codex.binary,
                    cwd,
                    selected_model(model_override, config.codex.model_default.clone()),
                )
                .await?;
                Ok(AgentBridge::Codex(b))
            }
            AgentKind::ClaudeCode => {
                let b = crate::claude_code::bridge::Bridge::spawn(
                    &config.claude_code.binary,
                    cwd,
                    selected_model(model_override, config.claude_code.model_default.clone()),
                )
                .await?;
                Ok(AgentBridge::ClaudeCode(b))
            }
        }
    }

    pub async fn run(&mut self, payload: &RunPayload, cwd: &Path) -> Result<AgentCursor> {
        match self {
            AgentBridge::Codex(b) => Ok(AgentCursor::Codex(b.run(payload, cwd).await?)),
            AgentBridge::ClaudeCode(b) => Ok(AgentCursor::ClaudeCode(b.run(payload, cwd).await?)),
        }
    }

    /// Wait for the next batch of events from the agent. Returns `None` when
    /// the subprocess has closed its output stream (no more events will ever
    /// arrive); callers should surface that as a run failure.
    pub async fn next_events(&mut self, cursor: &mut AgentCursor) -> Option<Vec<BridgeEvent>> {
        match (self, cursor) {
            (AgentBridge::Codex(b), AgentCursor::Codex(c)) => {
                let frame = b.next_frame().await?;
                Some(c.translate(frame))
            }
            (AgentBridge::ClaudeCode(b), AgentCursor::ClaudeCode(c)) => b.next_events(c).await,
            // These pairings are constructed together by `run`, so a mismatch
            // is a programmer error — fail loudly.
            _ => panic!("agent bridge and cursor variants mismatched"),
        }
    }

    pub async fn send_approval(
        &mut self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.send_approval(approval_id, decision).await,
            AgentBridge::ClaudeCode(b) => b.send_approval(approval_id, decision).await,
        }
    }

    pub async fn interrupt(&mut self) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.interrupt().await,
            AgentBridge::ClaudeCode(b) => b.interrupt().await,
        }
    }

    pub async fn shutdown(self, grace: Duration) -> Result<()> {
        match self {
            AgentBridge::Codex(b) => b.server.shutdown(grace).await,
            AgentBridge::ClaudeCode(b) => b.shutdown(grace).await,
        }
    }
}

fn selected_model(
    model_override: Option<String>,
    configured_default: Option<String>,
) -> Option<String> {
    model_override.or(configured_default)
}

#[cfg(test)]
mod tests {
    use super::selected_model;

    #[test]
    fn selected_model_prefers_run_override() {
        assert_eq!(
            selected_model(
                Some("claude-override".into()),
                Some("claude-default".into())
            ),
            Some("claude-override".into())
        );
    }

    #[test]
    fn selected_model_falls_back_to_config_default() {
        assert_eq!(
            selected_model(None, Some("claude-default".into())),
            Some("claude-default".into())
        );
    }

    #[test]
    fn selected_model_returns_none_when_unset() {
        assert_eq!(selected_model(None, None), None);
    }
}
