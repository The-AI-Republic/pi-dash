//! End-to-end Claude Code bridge test. A shell subprocess plays the role of
//! `claude --print --output-format stream-json` and emits canned stream-JSON
//! events; the Bridge drives it through `system/init → assistant message →
//! result`.

use pidash::agent::BridgeEvent;
use pidash::agent::RunPayload;
use pidash::claude_code::bridge::Bridge;
use pidash::claude_code::process::ClaudeProcess;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// The script drains stdin (so the bridge's `send_line` + stdin close
/// don't block on a full pipe), then emits a deterministic sequence of
/// stream-JSON events on stdout: an init frame with a session id, one
/// assistant message, and a terminal `result.success` frame.
fn fake_claude_script() -> &'static str {
    r#"
        set -e
        # Drain stdin in the background so the bridge can write + close it.
        (cat >/dev/null) &
        # system/init
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess_fake_001","model":"claude-sonnet-4-6","tools":["Read"]}'
        # one assistant message
        printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]},"session_id":"sess_fake_001"}'
        # terminal: result.success
        printf '%s\n' '{"type":"result","subtype":"success","session_id":"sess_fake_001","result":"all done","total_cost_usd":0.0001,"usage":{"input_tokens":10,"output_tokens":5}}'
        # Let the reader drain before we exit.
        sleep 0.2
    "#
}

#[tokio::test]
async fn bridge_happy_path_drives_fake_claude_to_completion() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(fake_claude_script());
    let proc = ClaudeProcess::spawn_command(cmd)
        .await
        .expect("spawn fake claude");
    let mut bridge = Bridge::from_process(proc);

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let payload = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "hi".into(),
        model: None,
        resume_thread_id: None,
    };

    // run() should consume the init frame and surface its session id as the
    // cursor's thread_id, matching the Codex bridge's contract.
    let mut cursor = bridge.run(&payload, &cwd).await.expect("bridge run setup");
    assert_eq!(cursor.thread_id, "sess_fake_001");

    let mut saw_assistant = false;
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
                BridgeEvent::Completed { done_payload, .. } => {
                    saw_completed = true;
                    assert_eq!(
                        done_payload.get("conclusion").and_then(|v| v.as_str()),
                        Some("success"),
                    );
                    break;
                }
                BridgeEvent::Failed { detail, .. } => {
                    panic!("unexpected Failed event: {detail:?}");
                }
                _ => {}
            }
        }
        if saw_completed {
            break;
        }
    }

    assert!(saw_assistant, "expected to observe an assistant/message");
    assert!(saw_completed, "expected to observe a Completed event");
}

#[tokio::test]
async fn bridge_translates_result_error_to_failed() {
    let script = r#"
        set -e
        (cat >/dev/null) &
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess_err_001"}'
        printf '%s\n' '{"type":"result","subtype":"error_max_turns","session_id":"sess_err_001","is_error":true,"result":"exceeded turn budget"}'
        sleep 0.2
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let proc = ClaudeProcess::spawn_command(cmd)
        .await
        .expect("spawn fake claude");
    let mut bridge = Bridge::from_process(proc);

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let payload = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "will fail".into(),
        model: None,
        resume_thread_id: None,
    };
    let mut cursor = bridge.run(&payload, &cwd).await.expect("bridge run setup");

    let events = tokio::time::timeout(Duration::from_secs(2), bridge.next_events(&mut cursor))
        .await
        .expect("pump should not time out")
        .expect("expected a Failed event, got None");

    let mut saw_failed = false;
    for ev in events {
        if let BridgeEvent::Failed { detail, .. } = ev {
            saw_failed = true;
            assert!(
                detail.as_deref().unwrap_or("").contains("exceeded"),
                "expected detail to include error message, got {detail:?}"
            );
        }
    }
    assert!(saw_failed, "expected a Failed event from result.error");
}
