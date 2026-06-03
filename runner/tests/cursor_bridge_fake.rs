//! End-to-end Cursor Agent bridge test. A shell subprocess plays the role of
//! `cursor-agent --print --output-format stream-json` and emits canned
//! stream-JSON events; the Bridge drives it through `system/init → assistant
//! message → tool_call → result`.
//!
//! Unlike the Claude bridge, cursor-agent takes the prompt as a positional
//! argument and is spawned lazily by `run`, so these tests inject the fake
//! command through `Bridge::run_with_command`.

use pidash::agent::BridgeEvent;
use pidash::cursor_agent::bridge::{Bridge, BridgeCursor};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Emits a deterministic sequence of cursor-agent stream-JSON events: an init
/// frame with a session id, one assistant message, a tool_call started/completed
/// pair, and a terminal `result.success` frame.
fn fake_cursor_script() -> &'static str {
    r#"
        set -e
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"cur_fake_001","model":"cursor-sonnet","permissionMode":"force","cwd":"/tmp"}'
        printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]},"session_id":"cur_fake_001"}'
        printf '%s\n' '{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"path":"README.md"}},"session_id":"cur_fake_001"}'
        printf '%s\n' '{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"readToolCall":{"result":{"success":{}}}},"session_id":"cur_fake_001"}'
        printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"duration_ms":1234,"result":"all done","session_id":"cur_fake_001"}'
        sleep 0.2
    "#
}

fn fake_cmd(script: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    cmd
}

async fn drain_until_completed(
    bridge: &mut Bridge,
    cursor: &mut BridgeCursor,
) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let Some(events) =
            tokio::time::timeout(Duration::from_millis(500), bridge.next_events(cursor))
                .await
                .ok()
                .flatten()
        else {
            break;
        };
        for ev in events {
            match ev {
                BridgeEvent::Completed { done_payload, .. } => return done_payload,
                BridgeEvent::Failed { detail, .. } => panic!("unexpected Failed event: {detail:?}"),
                _ => {}
            }
        }
    }
    panic!("expected to observe a Completed event");
}

#[tokio::test]
async fn bridge_happy_path_drives_fake_cursor_to_completion() {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("cursor-agent", &cwd, None)
        .await
        .expect("bridge setup");

    // run_with_command spawns the fake, consumes system/init, and surfaces its
    // session id as the cursor's thread_id (matching the Codex/Claude contract).
    let mut cursor = bridge
        .run_with_command(fake_cmd(fake_cursor_script()), Uuid::new_v4())
        .await
        .expect("bridge run setup");
    assert_eq!(cursor.thread_id, "cur_fake_001");

    let mut saw_assistant = false;
    let mut saw_tool_call = false;
    let mut saw_completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let Some(events) =
            tokio::time::timeout(Duration::from_millis(500), bridge.next_events(&mut cursor))
                .await
                .ok()
                .flatten()
        else {
            break;
        };
        for ev in events {
            match ev {
                BridgeEvent::Raw { method, .. } if method == "assistant/message" => {
                    saw_assistant = true;
                }
                BridgeEvent::Raw { method, .. } if method.starts_with("tool_call/") => {
                    saw_tool_call = true;
                }
                BridgeEvent::Completed { done_payload, .. } => {
                    saw_completed = true;
                    assert_eq!(
                        done_payload.get("conclusion").and_then(|v| v.as_str()),
                        Some("success"),
                    );
                    assert_eq!(
                        done_payload.get("duration_ms").and_then(|v| v.as_u64()),
                        Some(1234),
                    );
                    break;
                }
                BridgeEvent::Failed { detail, .. } => panic!("unexpected Failed event: {detail:?}"),
                _ => {}
            }
        }
        if saw_completed {
            break;
        }
    }

    assert!(saw_assistant, "expected to observe an assistant/message");
    assert!(saw_tool_call, "expected to observe a tool_call event");
    assert!(saw_completed, "expected to observe a Completed event");
}

#[tokio::test]
async fn one_shot_run_drives_fake_cursor_to_completion() {
    // cursor-agent print mode is inherently one-shot; run_one_shot must reach a
    // terminal result just like run. We exercise the same path via the test
    // seam, which is what run_one_shot funnels through in production.
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("cursor-agent", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(fake_cursor_script()), Uuid::new_v4())
        .await
        .expect("one-shot run setup");
    assert_eq!(cursor.thread_id, "cur_fake_001");
    let done = drain_until_completed(&mut bridge, &mut cursor).await;
    assert_eq!(
        done.get("result").and_then(|v| v.as_str()),
        Some("all done")
    );
}

#[tokio::test]
async fn bridge_translates_result_error_to_failed() {
    let script = r#"
        set -e
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"cur_err_001"}'
        printf '%s\n' '{"type":"result","subtype":"error","is_error":true,"result":"model refused"}'
        sleep 0.2
    "#;
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("cursor-agent", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(script), Uuid::new_v4())
        .await
        .expect("bridge run setup");

    let events = tokio::time::timeout(Duration::from_secs(2), bridge.next_events(&mut cursor))
        .await
        .expect("pump should not time out")
        .expect("expected a Failed event, got None");

    let mut saw_failed = false;
    for ev in events {
        if let BridgeEvent::Failed { detail, .. } = ev {
            saw_failed = true;
            assert!(
                detail.as_deref().unwrap_or("").contains("model refused"),
                "expected detail to include error message, got {detail:?}"
            );
        }
    }
    assert!(saw_failed, "expected a Failed event from result.error");
}

#[tokio::test]
async fn warm_returns_resume_session_id_without_spawning() {
    // warm must not spawn a process; it only echoes a known resume id so the
    // cloud can keep its session pointer stable until the first turn.
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn_with_resume("cursor-agent", &cwd, None, Some("prev_chat_42"))
        .await
        .expect("bridge setup");
    let warmed = bridge.warm(&cwd).await.expect("warm");
    assert_eq!(warmed.as_deref(), Some("prev_chat_42"));
    // No process spawned yet, so the observability handle reports no PID.
    assert!(bridge.process_handle().pid.is_none());
}
