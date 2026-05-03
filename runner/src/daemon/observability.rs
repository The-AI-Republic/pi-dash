//! Observability scalars for the per-active-run snapshot the runner sends
//! to the cloud on every poll. Designed to be agent-agnostic and
//! presentation-only; the cloud watchdog and the web UI consume these as
//! descriptive scalars (`.ai_design/runner_agent_bridge/design.md` §4.1).

use serde::{Deserialize, Serialize};

use crate::agent::BridgeEvent;

/// Streaming token usage parsed opportunistically from a Codex
/// `codex/event/token_count` Raw frame. Claude does not emit equivalent
/// streaming counts during a run, so this is left `None` for Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

/// Maximum length for `last_event_kind` — matches the cloud's
/// `RunnerLiveState.last_event_kind` column.
pub const KIND_MAX: usize = 64;

/// Maximum length for `last_event_summary` — matches the cloud's
/// `RunnerLiveState.last_event_summary` column.
pub const SUMMARY_MAX: usize = 200;

/// Classify a `BridgeEvent` into a short kind label, for the live-state
/// `last_event_kind` field. Length-capped to `KIND_MAX`.
pub fn kind_of(event: &BridgeEvent) -> String {
    let raw = match event {
        BridgeEvent::RunStarted { .. } => "run/started".to_string(),
        BridgeEvent::Raw { method, .. } => method.clone(),
        BridgeEvent::ApprovalRequest { .. } => "approval/request".to_string(),
        BridgeEvent::AwaitingReauth { .. } => "auth/awaiting_reauth".to_string(),
        BridgeEvent::Completed { .. } => "run/completed".to_string(),
        BridgeEvent::Failed { .. } => "run/failed".to_string(),
    };
    truncate(&raw, KIND_MAX)
}

/// Build a structure-only one-line summary of a `BridgeEvent` for the
/// live-state `last_event_summary` field. This must NEVER include prompt
/// text, model output, or file content — only method names, ids,
/// durations, exit codes. Length-capped to `SUMMARY_MAX`.
///
/// Returns `String` rather than `Option<String>` because every variant
/// produces a non-empty summary today; callers that want to defer
/// stamping (e.g. on a quiet idle tick) should not call this in the
/// first place.
pub fn summary_of(event: &BridgeEvent) -> String {
    let raw = match event {
        BridgeEvent::RunStarted { thread_id, .. } => {
            format!("run started thread={thread_id}")
        }
        BridgeEvent::Raw { method, .. } => {
            // Only the method name and an opaque tag for the params shape.
            // Never inline `params` content — those carry user prompts and
            // model output for many codex methods.
            format!("raw method={method}")
        }
        BridgeEvent::ApprovalRequest {
            approval_id, kind, ..
        } => {
            format!("approval request id={approval_id} kind={kind:?}")
        }
        BridgeEvent::AwaitingReauth { .. } => "awaiting reauth".to_string(),
        BridgeEvent::Completed { .. } => "run completed".to_string(),
        BridgeEvent::Failed { reason, .. } => {
            format!("run failed reason={reason:?}")
        }
    };
    truncate(&raw, SUMMARY_MAX)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Truncate on a char boundary.
    let mut end = max;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    s[..end].to_string()
}

/// Best-effort parser for a Codex `codex/event/token_count` Raw frame's
/// params. Returns `None` if the params don't match the expected shape.
/// Failures are non-fatal — the supervisor logs at debug and continues.
///
/// Accepts either of the two shapes seen on the wire:
///   - `{ "usage": { "input_tokens": ..., "output_tokens": ... } }`
///   - `{ "input_tokens": ..., "output_tokens": ... }` (flat)
/// Both shapes must additionally surface `total_tokens` to be accepted —
/// a frame with only `input_tokens`/`output_tokens` and no `total_tokens`
/// is treated as not-a-token-count to keep the parser narrow. This
/// avoids false positives on unrelated frames that happen to carry
/// numeric fields named `input_tokens` / `output_tokens`.
pub fn parse_codex_token_count(params: &serde_json::Value) -> Option<TokenUsage> {
    let usage = params.get("usage").unwrap_or(params);
    let input = usage.get("input_tokens").and_then(|v| v.as_u64())?;
    let output = usage.get("output_tokens").and_then(|v| v.as_u64())?;
    let total = usage.get("total_tokens").and_then(|v| v.as_u64())?;
    Some(TokenUsage {
        input,
        output,
        total,
    })
}

/// Best-effort extractor for a Bash `tool_use` block in a Claude
/// `assistant/message` Raw frame. Returns `Some(command)` if the message
/// content array contains a `{"type":"tool_use","name":"Bash"}` block
/// with a string `input.command`; `None` otherwise. Used to populate
/// `last_exec_command` for failure-detail enrichment, in parallel with
/// the codex `item/started` / `commandExecution` capture path.
///
/// Other tool_use blocks (Read, Edit, Grep, …) are intentionally ignored
/// — Bash is the call most likely to hang on network / sandbox issues
/// and the only one whose `input.command` resembles a shell command we'd
/// want to surface verbatim. Future tools can be added as new arms if
/// they prove to be common stall sources.
pub fn parse_claude_bash_command(message: &serde_json::Value) -> Option<String> {
    let content = message.get("content")?.as_array()?;
    for block in content {
        let is_tool_use = block.get("type").and_then(|v| v.as_str()) == Some("tool_use");
        let is_bash = block.get("name").and_then(|v| v.as_str()) == Some("Bash");
        if is_tool_use && is_bash {
            let cmd = block.get("input")?.get("command")?.as_str()?.trim();
            if cmd.is_empty() {
                return None;
            }
            return Some(cmd.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::protocol::{ApprovalKind, FailureReason};
    use serde_json::json;
    use uuid::Uuid;

    fn run_id() -> Uuid {
        Uuid::nil()
    }

    #[test]
    fn kind_of_uses_raw_method() {
        let ev = BridgeEvent::Raw {
            run_id: run_id(),
            method: "codex/event/token_count".into(),
            params: json!({}),
        };
        assert_eq!(kind_of(&ev), "codex/event/token_count");
    }

    #[test]
    fn kind_of_caps_length() {
        let long = "x".repeat(200);
        let ev = BridgeEvent::Raw {
            run_id: run_id(),
            method: long,
            params: json!({}),
        };
        assert_eq!(kind_of(&ev).len(), KIND_MAX);
    }

    #[test]
    fn summary_of_does_not_leak_params() {
        // Even when params carry user-content, summary_of must never
        // surface them. We assert the summary contains the method name
        // but does NOT contain the embedded `prompt` or `output` content.
        let ev = BridgeEvent::Raw {
            run_id: run_id(),
            method: "tool/exec".into(),
            params: json!({"prompt": "rm -rf /etc", "output": "secret data"}),
        };
        let s = summary_of(&ev);
        assert!(s.contains("method=tool/exec"));
        assert!(!s.contains("rm -rf"), "summary leaked params content: {s}");
        assert!(
            !s.contains("secret data"),
            "summary leaked params content: {s}"
        );
    }

    #[test]
    fn summary_of_caps_length() {
        let long = "x".repeat(500);
        let ev = BridgeEvent::Raw {
            run_id: run_id(),
            method: long,
            params: json!({}),
        };
        let s = summary_of(&ev);
        assert!(s.len() <= SUMMARY_MAX);
    }

    #[test]
    fn summary_of_failed_includes_reason_classifier() {
        let ev = BridgeEvent::Failed {
            run_id: run_id(),
            reason: FailureReason::Timeout,
            detail: Some("user-detail".into()),
        };
        let s = summary_of(&ev);
        assert!(s.contains("Timeout"));
        // Detail string carries operator-supplied text; we deliberately
        // do not echo it through, only the classifier.
        assert!(!s.contains("user-detail"));
    }

    #[test]
    fn summary_of_approval_includes_kind() {
        let ev = BridgeEvent::ApprovalRequest {
            run_id: run_id(),
            approval_id: "abc".into(),
            kind: ApprovalKind::CommandExecution,
            payload: json!({"cmd": "ls"}),
            reason: Some("test".into()),
        };
        let s = summary_of(&ev);
        assert!(s.contains("approval request"));
        assert!(s.contains("kind="));
        assert!(!s.contains("\"cmd\""));
    }

    #[test]
    fn parse_codex_token_count_accepts_usage_block() {
        let params = json!({
            "usage": {"input_tokens": 100, "output_tokens": 250, "total_tokens": 350}
        });
        let u = parse_codex_token_count(&params).unwrap();
        assert_eq!(u.input, 100);
        assert_eq!(u.output, 250);
        assert_eq!(u.total, 350);
    }

    #[test]
    fn parse_codex_token_count_accepts_flat_block_with_total() {
        let params = json!({
            "input_tokens": 10,
            "output_tokens": 20,
            "total_tokens": 30
        });
        let u = parse_codex_token_count(&params).unwrap();
        assert_eq!(u.input, 10);
        assert_eq!(u.output, 20);
        assert_eq!(u.total, 30);
    }

    #[test]
    fn parse_codex_token_count_rejects_flat_without_total() {
        // Defensive: a frame with only input/output and no total is
        // ambiguous — could be an unrelated event with a similar
        // numeric field naming. Stay narrow to avoid false positives.
        let params = json!({"input_tokens": 10, "output_tokens": 20});
        assert!(parse_codex_token_count(&params).is_none());
    }

    #[test]
    fn parse_codex_token_count_returns_none_on_garbage() {
        let params = json!({"unrelated": true});
        assert!(parse_codex_token_count(&params).is_none());
    }

    #[test]
    fn parse_claude_bash_command_extracts_command() {
        let msg = json!({
            "id": "msg_1",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check."},
                {
                    "type": "tool_use",
                    "id": "tu_1",
                    "name": "Bash",
                    "input": {"command": "git fetch origin", "description": "sync"},
                },
            ],
        });
        assert_eq!(
            parse_claude_bash_command(&msg),
            Some("git fetch origin".to_string())
        );
    }

    #[test]
    fn parse_claude_bash_command_ignores_non_bash_tools() {
        // A Read tool_use should NOT populate last_exec_command — Read
        // hangs are far rarer than Bash and we want the extractor narrow
        // to avoid noise in the failure-detail string.
        let msg = json!({
            "content": [
                {
                    "type": "tool_use",
                    "name": "Read",
                    "input": {"file_path": "/tmp/foo"},
                },
            ],
        });
        assert!(parse_claude_bash_command(&msg).is_none());
    }

    #[test]
    fn parse_claude_bash_command_returns_none_when_no_tool_use() {
        let msg = json!({
            "content": [{"type": "text", "text": "just thinking"}],
        });
        assert!(parse_claude_bash_command(&msg).is_none());
    }

    #[test]
    fn parse_claude_bash_command_returns_none_on_empty_command() {
        let msg = json!({
            "content": [
                {"type": "tool_use", "name": "Bash", "input": {"command": "  "}},
            ],
        });
        assert!(parse_claude_bash_command(&msg).is_none());
    }
}
