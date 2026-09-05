//! End-to-end Muse Code bridge test. A shell subprocess plays the role of
//! `muse exec --json --prompt-file <PATH>` and emits canned JSONL events; the
//! Bridge drives it through `system/init → assistant message → tool_call →
//! result`.
//!
//! Like the Cursor bridge, `muse exec` is spawned lazily by `run` (the prompt
//! isn't known at construction), so these tests inject the fake command through
//! `Bridge::run_with_command`. Muse takes its prompt from a file rather than
//! argv, so the injected command carries no real prompt file (`None`).

use pidash::agent::BridgeEvent;
use pidash::muse_code::bridge::{Bridge, BridgeCursor};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Emits a deterministic sequence of muse exec JSONL events: an init frame with
/// a session id, one assistant message, a tool_call started/completed pair, and
/// a terminal `result.success` frame.
fn fake_muse_script() -> &'static str {
    r#"
        set -e
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"muse_fake_001","model":"muse-spark","permissionMode":"yolo","cwd":"/tmp"}'
        printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]},"session_id":"muse_fake_001"}'
        printf '%s\n' '{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"read":{"path":"README.md"}},"session_id":"muse_fake_001"}'
        printf '%s\n' '{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"read":{"result":{"success":{}}}},"session_id":"muse_fake_001"}'
        printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"duration_ms":1234,"result":"all done","session_id":"muse_fake_001"}'
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
async fn bridge_happy_path_drives_fake_muse_to_completion() {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");

    // run_with_command spawns the fake, consumes the first frame, and surfaces
    // its session id as the cursor's thread_id (matching the Codex/Cursor
    // contract). No real prompt file is injected in the test.
    let mut cursor = bridge
        .run_with_command(fake_cmd(fake_muse_script()), None, Uuid::new_v4())
        .await
        .expect("bridge run setup");
    assert_eq!(cursor.thread_id, "muse_fake_001");

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
async fn one_shot_run_drives_fake_muse_to_completion() {
    // muse exec is inherently one-shot; run_one_shot must reach a terminal
    // result just like run. We exercise the same path via the test seam, which
    // is what run_one_shot funnels through in production.
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(fake_muse_script()), None, Uuid::new_v4())
        .await
        .expect("one-shot run setup");
    assert_eq!(cursor.thread_id, "muse_fake_001");
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
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"muse_err_001"}'
        printf '%s\n' '{"type":"result","subtype":"error","is_error":true,"result":"model refused"}'
        sleep 0.2
    "#;
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
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
async fn result_before_init_is_surfaced_as_failed() {
    // muse exec can fail (auth/quota) before ever emitting a `system/init`
    // frame, going straight to a terminal `result`. The bridge must still
    // complete run setup and surface that result as a Failed event carrying the
    // real detail, not bail with a generic "stdout closed" crash that drops it.
    let script = r#"
        set -e
        printf '%s\n' '{"type":"result","subtype":"error","is_error":true,"result":"unauthorized: set META_API_KEY"}'
        sleep 0.2
    "#;
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    // Run setup must succeed even though no init frame arrives; the thread id is
    // synthesized from the run id.
    let mut cursor = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .expect("run setup should not bail when result precedes init");

    let events = tokio::time::timeout(Duration::from_secs(2), bridge.next_events(&mut cursor))
        .await
        .expect("pump should not time out")
        .expect("expected a Failed event, got None");

    let mut saw_failed = false;
    for ev in events {
        if let BridgeEvent::Failed { detail, .. } = ev {
            saw_failed = true;
            assert!(
                detail.as_deref().unwrap_or("").contains("unauthorized"),
                "expected detail to include the pre-init error, got {detail:?}"
            );
        }
    }
    assert!(
        saw_failed,
        "expected a Failed event from a result-before-init"
    );
}

#[tokio::test]
async fn no_init_frame_still_streams_content_and_completes() {
    // Muse's schema is not guaranteed to lead with `system/init`. When the first
    // frame is ordinary content, the bridge must adopt the frame's session id as
    // the thread id, surface the content, and still reach completion.
    let script = r#"
        set -e
        printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"working"}]},"session_id":"muse_noinit_001"}'
        printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"done"}'
        sleep 0.2
    "#;
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .expect("run setup should not require an init frame");
    // The thread id is adopted from the first content frame's session id.
    assert_eq!(cursor.thread_id, "muse_noinit_001");

    let mut saw_assistant = false;
    let mut saw_completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline && !saw_completed {
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
                BridgeEvent::Completed { .. } => saw_completed = true,
                BridgeEvent::Failed { detail, .. } => panic!("unexpected Failed: {detail:?}"),
                _ => {}
            }
        }
    }
    assert!(saw_assistant, "expected the pre-buffered assistant frame");
    assert!(
        saw_completed,
        "expected to reach completion without an init frame"
    );
}

#[tokio::test]
async fn warm_returns_resume_session_id_without_spawning() {
    // warm must not spawn a process; it only echoes a known resume id so the
    // cloud can keep its session pointer stable until the first turn.
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn_with_resume("muse", &cwd, None, Some("prev_sess_42"))
        .await
        .expect("bridge setup");
    let warmed = bridge.warm(&cwd).await.expect("warm");
    assert_eq!(warmed.as_deref(), Some("prev_sess_42"));
    // No process spawned yet, so the observability handle reports no PID.
    assert!(bridge.process_handle().pid.is_none());
}

/// Liveness check via `kill -0` (portable across Linux/macOS). Returns false
/// once the process is gone (reaped) — `wait_task` reaps right after killing, so
/// the brief zombie window closes immediately.
fn proc_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn wait_until(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cond()
}

#[tokio::test]
async fn dropping_bridge_kills_a_running_muse() {
    // A fake that emits init then blocks on a long sleep, so the subprocess
    // stays alive after run setup. Dropping the bridge (without a graceful
    // shutdown) must kill it rather than leave it orphaned to completion.
    let script = r#"
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"muse_long_001"}'
        sleep 30
    "#;
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    let _cursor = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .expect("run setup");
    let pid = bridge
        .process_handle()
        .pid
        .expect("a spawned process should report a pid");
    assert!(
        proc_alive(pid),
        "child should be alive right after run setup"
    );

    // Drop the bridge — its kill channel closes; the wait task should kill +
    // reap the still-running child.
    drop(bridge);

    let gone = wait_until(|| !proc_alive(pid), Duration::from_secs(5)).await;
    assert!(
        gone,
        "dropping the bridge should kill the running muse (pid {pid})"
    );
}

#[tokio::test]
async fn shutdown_returns_promptly_after_the_process_exits() {
    // Drive a fake to completion (process exits), then `shutdown` must not block
    // on a `changed()` that will never fire — it should observe the prior exit
    // and return well under the (deliberately large) grace.
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bridge = Bridge::spawn("muse", &cwd, None)
        .await
        .expect("bridge setup");
    let mut cursor = bridge
        .run_with_command(fake_cmd(fake_muse_script()), None, Uuid::new_v4())
        .await
        .expect("run setup");
    let _ = drain_until_completed(&mut bridge, &mut cursor).await;

    let started = tokio::time::Instant::now();
    bridge
        .shutdown(Duration::from_secs(30))
        .await
        .expect("shutdown");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "shutdown should return promptly after the process already exited"
    );
}
