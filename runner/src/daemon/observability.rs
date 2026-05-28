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
///
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

/// Plain-data extract of a shell command the agent kicked off. Mirrors
/// `crate::daemon::state::ExecCommandSnapshot`'s shape sans the
/// timestamp (which the caller stamps with `Utc::now()`). Decoupling
/// the parser shape from `StateHandle` lets us unit-test the dispatch
/// logic without any tokio runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommandHint {
    pub command: String,
    pub cwd: Option<String>,
    pub tool_call_id: Option<String>,
}

/// Plain-data extract for a command-tool completion. Kept separate from
/// [`ExecCommandHint`] because some protocols echo only the tool id on
/// completion, not the command text.
///
/// `tool_call_id` is non-optional: completion events without an id cannot
/// be matched against any stored command, so we drop them at parse time
/// rather than carrying a `None` that would force every consumer to
/// re-decide what to do with it. `success` is best-effort — `Some(true)`
/// for a clean completion, `Some(false)` for a non-success terminal state
/// (codex `failed` status or Claude `is_error: true`), `None` when the
/// protocol frame doesn't surface an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommandCompletionHint {
    pub tool_call_id: String,
    pub success: Option<bool>,
}

/// Single dispatch table for "what shell command did the agent just
/// kick off?" given a `BridgeEvent::Raw` `(method, params)`. Returns
/// `Some` only for the methods we know how to extract a command from
/// today: codex `item/started` (commandExecution items) and Claude
/// `assistant/message` (Bash tool_use blocks). Unit-testable in
/// isolation; the supervisor calls this from `handle_bridge_event`
/// without owning the routing logic itself, so a typo in either method
/// name is caught by tests rather than by silently absent command-tool
/// context in production failures.
pub fn extract_exec_command_hint(
    method: &str,
    params: &serde_json::Value,
) -> Option<ExecCommandHint> {
    match method {
        "item/started" => {
            let item = params.get("item")?;
            if item.get("type").and_then(|v| v.as_str()) != Some("commandExecution") {
                return None;
            }
            if item.get("status").and_then(|v| v.as_str()) != Some("inProgress") {
                return None;
            }
            let command = item.get("command").and_then(|v| v.as_str())?.trim();
            if command.is_empty() {
                return None;
            }
            let cwd = item
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(ExecCommandHint {
                command: command.to_string(),
                cwd,
                tool_call_id: item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        }
        "assistant/message" => parse_claude_bash_tool_use(params),
        _ => None,
    }
}

/// Extract command-tool completions from bridge Raw events. This lets the
/// failure-detail builder say whether the last observed command had already
/// completed before an agent/API failure occurred, and whether it ended
/// cleanly. Claude may echo multiple tool_result blocks in one message, so
/// callers must consider every hint.
///
/// Hints without an explicit tool id are dropped here, not surfaced as
/// `None`: the only useful operation on a completion hint is matching it
/// against a stored command's id, and a `None` id can never match
/// anything meaningful. See also [`crate::daemon::state::StateHandle::note_exec_command_completed`].
pub fn extract_exec_command_completion_hints(
    method: &str,
    params: &serde_json::Value,
) -> Vec<ExecCommandCompletionHint> {
    match method {
        "item/completed" => {
            let Some(item) = params.get("item") else {
                return Vec::new();
            };
            if item.get("type").and_then(|v| v.as_str()) != Some("commandExecution") {
                return Vec::new();
            }
            let Some(tool_call_id) = item.get("id").and_then(|v| v.as_str()) else {
                return Vec::new();
            };
            // Codex item/completed carries a status string. Treat any
            // value other than "completed" (e.g. "failed", "errored") as
            // an unsuccessful terminal — the command did finish, but the
            // failure-detail wording should not call it a clean
            // completion. Missing status leaves success as `None`.
            let success = item
                .get("status")
                .and_then(|v| v.as_str())
                .map(|status| status == "completed");
            vec![ExecCommandCompletionHint {
                tool_call_id: tool_call_id.to_string(),
                success,
            }]
        }
        "user/toolResult" => parse_claude_tool_results(params),
        _ => Vec::new(),
    }
}

/// Best-effort extractor for Bash `tool_use` blocks in a Claude
/// `assistant/message` Raw frame. Returns the last valid Bash command if
/// the message content array contains any `{"type":"tool_use","name":"Bash"}`
/// block with a string `input.command`; `None` otherwise. Used to populate
/// `last_exec_command` for failure-detail enrichment, in parallel with the
/// codex `item/started` / `commandExecution` capture path.
///
/// Other tool_use blocks (Read, Edit, Grep, …) are intentionally ignored
/// — Bash is the call most likely to hang on network / sandbox issues
/// and the only one whose `input.command` resembles a shell command we'd
/// want to surface verbatim. Future tools can be added as new arms if
/// they prove to be common stall sources.
///
/// Last-wins limitation: when a single assistant message carries more
/// than one Bash block, only the trailing one is stored. Earlier Bash
/// calls in the same message are not tracked for completion — if one of
/// them is the call that hangs, the failure-detail will name the wrong
/// command. In practice Claude usually serialises Bash calls across
/// successive messages, so this is a known acceptable loss of fidelity
/// rather than a silent bug.
fn parse_claude_bash_tool_use(message: &serde_json::Value) -> Option<ExecCommandHint> {
    let content = message.get("content")?.as_array()?;
    let mut last_bash: Option<ExecCommandHint> = None;
    for block in content {
        let is_tool_use = block.get("type").and_then(|v| v.as_str()) == Some("tool_use");
        let is_bash = block.get("name").and_then(|v| v.as_str()) == Some("Bash");
        if is_tool_use && is_bash {
            let Some(cmd) = block
                .get("input")
                .and_then(|input| input.get("command"))
                .and_then(|command| command.as_str())
                .map(str::trim)
            else {
                continue;
            };
            if cmd.is_empty() {
                continue;
            }
            last_bash = Some(ExecCommandHint {
                command: cmd.to_string(),
                cwd: None,
                tool_call_id: block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            });
        }
    }
    last_bash
}

/// Pull command-tool completion hints out of a Claude `user/toolResult`
/// raw frame. Blocks without a `tool_use_id` are dropped — they cannot
/// match a stored command. The `is_error` field, when present, drives
/// the `success` flag (`is_error: true` ⇒ `Some(false)`).
fn parse_claude_tool_results(message: &serde_json::Value) -> Vec<ExecCommandCompletionHint> {
    let Some(content) = message.get("content").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                return None;
            }
            let tool_call_id = block
                .get("tool_use_id")
                .and_then(|v| v.as_str())?
                .to_string();
            let success = block
                .get("is_error")
                .and_then(|v| v.as_bool())
                .map(|is_error| !is_error);
            Some(ExecCommandCompletionHint {
                tool_call_id,
                success,
            })
        })
        .collect()
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
    fn parse_claude_bash_tool_use_extracts_command_and_id() {
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
        let hint = parse_claude_bash_tool_use(&msg).unwrap();
        assert_eq!(hint.command, "git fetch origin");
        assert_eq!(hint.tool_call_id.as_deref(), Some("tu_1"));
    }

    #[test]
    fn parse_claude_bash_tool_use_uses_last_bash_block() {
        let msg = json!({
            "content": [
                {
                    "type": "tool_use",
                    "id": "tu_1",
                    "name": "Bash",
                    "input": {"command": "git status"},
                },
                {
                    "type": "tool_use",
                    "id": "tu_read",
                    "name": "Read",
                    "input": {"file_path": "/tmp/foo"},
                },
                {
                    "type": "tool_use",
                    "id": "tu_2",
                    "name": "Bash",
                    "input": {"command": "npm test"},
                },
            ],
        });
        let hint = parse_claude_bash_tool_use(&msg).unwrap();
        assert_eq!(hint.command, "npm test");
        assert_eq!(hint.tool_call_id.as_deref(), Some("tu_2"));
    }

    #[test]
    fn parse_claude_bash_tool_use_ignores_non_bash_tools() {
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
        assert!(parse_claude_bash_tool_use(&msg).is_none());
    }

    #[test]
    fn parse_claude_bash_tool_use_returns_none_when_no_tool_use() {
        let msg = json!({
            "content": [{"type": "text", "text": "just thinking"}],
        });
        assert!(parse_claude_bash_tool_use(&msg).is_none());
    }

    #[test]
    fn parse_claude_bash_tool_use_returns_none_on_empty_command() {
        let msg = json!({
            "content": [
                {"type": "tool_use", "name": "Bash", "input": {"command": "  "}},
            ],
        });
        assert!(parse_claude_bash_tool_use(&msg).is_none());
    }

    #[test]
    fn parse_claude_tool_results_extracts_all_tool_use_ids() {
        let msg = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "done"},
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "ok"},
                {"type": "tool_result", "tool_use_id": "tu_2", "content": "also ok"},
            ],
        });
        let hints = parse_claude_tool_results(&msg);
        let ids = hints
            .iter()
            .map(|hint| hint.tool_call_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["tu_1", "tu_2"]);
        // is_error absent ⇒ success is None
        assert!(hints.iter().all(|hint| hint.success.is_none()));
    }

    #[test]
    fn parse_claude_tool_results_propagates_is_error_into_success() {
        let msg = json!({
            "content": [
                {"type": "tool_result", "tool_use_id": "tu_ok", "is_error": false, "content": "ok"},
                {"type": "tool_result", "tool_use_id": "tu_err", "is_error": true, "content": "boom"},
            ],
        });
        let hints = parse_claude_tool_results(&msg);
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].tool_call_id, "tu_ok");
        assert_eq!(hints[0].success, Some(true));
        assert_eq!(hints[1].tool_call_id, "tu_err");
        assert_eq!(hints[1].success, Some(false));
    }

    #[test]
    fn parse_claude_tool_results_drops_blocks_without_tool_use_id() {
        // A tool_result without a tool_use_id can't match any stored
        // command. Surfacing it as a completion hint would risk a
        // spurious match against a stored command whose own tool_call_id
        // was absent — exactly the regression this PR is designed to
        // prevent. The parser must drop these blocks at the boundary.
        let msg = json!({
            "content": [
                {"type": "tool_result", "content": "no id here"},
                {"type": "tool_result", "tool_use_id": "tu_real", "content": "fine"},
            ],
        });
        let hints = parse_claude_tool_results(&msg);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].tool_call_id, "tu_real");
    }

    // ---- extract_exec_command_hint dispatch tests ------------------------
    //
    // These guard the supervisor's wiring against method-name typos. If
    // `handle_bridge_event` ever stops passing `"item/started"` /
    // `"assistant/message"` here, the production code silently degrades to
    // "no command-tool context" forever — these tests catch that at
    // compile-fixed string level.

    #[test]
    fn extract_exec_command_hint_handles_codex_item_started() {
        let params = json!({
            "item": {
                "type": "commandExecution",
                "status": "inProgress",
                "command": "git fetch origin",
                "cwd": "/tmp/x",
            },
        });
        let hint = extract_exec_command_hint("item/started", &params).unwrap();
        assert_eq!(hint.command, "git fetch origin");
        assert_eq!(hint.cwd.as_deref(), Some("/tmp/x"));
        assert!(hint.tool_call_id.is_none());
    }

    #[test]
    fn extract_exec_command_hint_skips_codex_completed_items() {
        // Only `inProgress` items count — completed items shouldn't
        // overwrite the live `last_exec_command` with a stale one.
        let params = json!({
            "item": {
                "type": "commandExecution",
                "status": "completed",
                "command": "git status",
            },
        });
        assert!(extract_exec_command_hint("item/started", &params).is_none());
    }

    #[test]
    fn extract_exec_command_hint_skips_non_command_items() {
        let params = json!({
            "item": {"type": "fileEdit", "status": "inProgress"},
        });
        assert!(extract_exec_command_hint("item/started", &params).is_none());
    }

    #[test]
    fn extract_exec_command_hint_handles_claude_assistant_message() {
        let params = json!({
            "content": [
                {
                    "type": "tool_use",
                    "id": "tu_2",
                    "name": "Bash",
                    "input": {"command": "npm install"},
                },
            ],
        });
        let hint = extract_exec_command_hint("assistant/message", &params).unwrap();
        assert_eq!(hint.command, "npm install");
        // Claude tool_use blocks don't carry cwd; the working dir is
        // implicit from the run's workspace_root.
        assert!(hint.cwd.is_none());
        assert_eq!(hint.tool_call_id.as_deref(), Some("tu_2"));
    }

    #[test]
    fn extract_exec_command_completion_hints_handles_codex_item_completed() {
        let params = json!({
            "item": {"id": "it_1", "type": "commandExecution", "status": "completed"},
        });
        let hints = extract_exec_command_completion_hints("item/completed", &params);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].tool_call_id, "it_1");
        assert_eq!(hints[0].success, Some(true));
    }

    #[test]
    fn extract_exec_command_completion_hints_marks_failed_codex_item_unsuccessful() {
        let params = json!({
            "item": {"id": "it_2", "type": "commandExecution", "status": "failed"},
        });
        let hints = extract_exec_command_completion_hints("item/completed", &params);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].tool_call_id, "it_2");
        assert_eq!(
            hints[0].success,
            Some(false),
            "a failed item/completed must not be reported as a clean completion"
        );
    }

    #[test]
    fn extract_exec_command_completion_hints_drops_codex_item_completed_without_id() {
        // Without an item id there is nothing to match against the stored
        // command; emitting a hint here would spuriously mark unrelated
        // commands as completed.
        let params = json!({
            "item": {"type": "commandExecution", "status": "completed"},
        });
        let hints = extract_exec_command_completion_hints("item/completed", &params);
        assert!(hints.is_empty());
    }

    #[test]
    fn extract_exec_command_completion_hints_handles_claude_tool_results() {
        let params = json!({
            "content": [
                {"type": "tool_result", "tool_use_id": "tu_3", "content": "done"},
                {"type": "tool_result", "tool_use_id": "tu_4", "content": "also done"},
            ],
        });
        let hints = extract_exec_command_completion_hints("user/toolResult", &params);
        let ids = hints
            .iter()
            .map(|hint| hint.tool_call_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["tu_3", "tu_4"]);
    }

    #[test]
    fn extract_exec_command_hint_returns_none_on_unknown_method() {
        let params = json!({"anything": true});
        assert!(extract_exec_command_hint("turn/started", &params).is_none());
        assert!(extract_exec_command_hint("codex/event/token_count", &params).is_none());
        assert!(extract_exec_command_hint("totally/made/up", &params).is_none());
    }
}
