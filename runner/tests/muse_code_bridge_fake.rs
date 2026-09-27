//! End-to-end Muse Code bridge test. A shell subprocess plays the role of
//! `muse exec --json --prompt-file <PATH>` by replaying JSONL captured from a
//! real `muse` binary (`tests/fixtures/muse_code/`, muse 1.3.0-R3401.1):
//!
//! - `meta_completed_run.jsonl` — a real Meta-provider run (tool calls, one
//!   failed tool call, streamed final answer, `run.terminal.completed`), with
//!   prompt / tool output / answer text redacted;
//! - `echo_completed_run.jsonl` / `echo_resumed_run.jsonl` — two turns of
//!   `muse exec --json --provider echo`, the second resumed with
//!   `--session-id <first run's session stream id>`;
//! - `meta_transport_failed_run.jsonl` — a real `run.terminal.failed`.
//!
//! The canned frames must come from the real binary: the original fake emitted
//! the same invented schema the parser was written against, so the suite was
//! green while every production run failed. Refresh the fixtures from
//! `muse exec --json` when upgrading Muse.
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

/// Session stream id of the echo fixtures (both turns share it).
const ECHO_SESSION: &str = "01a0b6be-4608-7a63-92a0-ecd8ed7c4c7b";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/muse_code")
        .join(name)
}

fn fixture_session_id(name: &str) -> String {
    let body = std::fs::read_to_string(fixture(name)).expect("read fixture");
    let first: serde_json::Value =
        serde_json::from_str(body.lines().next().expect("non-empty fixture")).unwrap();
    first["stream"]["id"].as_str().unwrap().to_owned()
}

/// A fake `muse exec` that replays a captured fixture and exits 0.
fn replay_cmd(name: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(r#"cat "$1"; sleep 0.2"#)
        .arg("sh")
        .arg(fixture(name));
    cmd
}

fn fake_cmd(script: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    cmd
}

async fn new_bridge(resume: Option<&str>) -> Bridge {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Bridge::spawn_with_resume("muse", &cwd, None, resume)
        .await
        .expect("bridge setup")
}

/// Pump until a terminal event (or EOF), returning every event seen.
async fn drain(bridge: &mut Bridge, cursor: &mut BridgeCursor) -> Vec<BridgeEvent> {
    let mut all = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let Some(events) =
            tokio::time::timeout(Duration::from_millis(1000), bridge.next_events(cursor))
                .await
                .ok()
                .flatten()
        else {
            break;
        };
        let terminal = events.iter().any(|e| {
            matches!(
                e,
                BridgeEvent::Completed { .. } | BridgeEvent::Failed { .. }
            )
        });
        all.extend(events);
        if terminal {
            break;
        }
    }
    all
}

fn raw_methods(events: &[BridgeEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            BridgeEvent::Raw { method, .. } => Some(method.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn real_meta_run_completes_with_typed_transcript() {
    let mut bridge = new_bridge(None).await;
    let mut cursor = bridge
        .run_with_command(replay_cmd("meta_completed_run.jsonl"), None, Uuid::new_v4())
        .await
        .expect("bridge run setup");
    // The thread id is Muse's session stream UUID.
    assert_eq!(
        cursor.thread_id,
        fixture_session_id("meta_completed_run.jsonl")
    );
    assert_eq!(cursor.model.as_deref(), Some("muse-spark-1.3-contributor"));

    let events = drain(&mut bridge, &mut cursor).await;
    let methods = raw_methods(&events);
    assert!(!methods.contains(&"unknown"), "{methods:?}");
    assert!(methods.contains(&"assistant/message"), "{methods:?}");
    assert!(methods.contains(&"tool_call/started"), "{methods:?}");
    assert!(methods.contains(&"user/toolResult"), "{methods:?}");
    match events.last() {
        Some(BridgeEvent::Completed { done_payload, .. }) => {
            assert_eq!(done_payload["conclusion"], "completed");
            assert!(
                done_payload["result"]
                    .as_str()
                    .unwrap()
                    .starts_with("Delta 1."),
                "{done_payload}"
            );
        }
        other => panic!("expected Completed last, got {other:?}"),
    }
}

#[tokio::test]
async fn echo_turn_then_resumed_turn_share_the_session() {
    // Turn 1: fresh bridge; the session id comes from the stream.
    let mut bridge = new_bridge(None).await;
    let mut cursor = bridge
        .run_with_command(replay_cmd("echo_completed_run.jsonl"), None, Uuid::new_v4())
        .await
        .expect("turn 1 setup");
    assert_eq!(cursor.thread_id, ECHO_SESSION);
    let events = drain(&mut bridge, &mut cursor).await;
    assert!(
        matches!(events.last(), Some(BridgeEvent::Completed { done_payload, .. })
            if done_payload["result"] == "echo: hello there"),
        "{events:?}"
    );
    // `--session-id` for the next turn is the (UUID) session id.
    assert_eq!(
        bridge
            .warm(&PathBuf::from("/tmp"))
            .await
            .unwrap()
            .as_deref(),
        Some(ECHO_SESSION)
    );

    // Turn 2: a resumed bridge (as the daemon builds it for a follow-up turn)
    // replays the real resumed-turn capture.
    let mut bridge = new_bridge(Some(ECHO_SESSION)).await;
    let mut cursor = bridge
        .run_with_command(replay_cmd("echo_resumed_run.jsonl"), None, Uuid::new_v4())
        .await
        .expect("turn 2 setup");
    assert_eq!(cursor.thread_id, ECHO_SESSION);
    let events = drain(&mut bridge, &mut cursor).await;
    assert!(
        matches!(events.last(), Some(BridgeEvent::Completed { done_payload, .. })
            if done_payload["result"] == "echo: second turn"),
        "{events:?}"
    );
}

#[tokio::test]
async fn real_terminal_failure_is_surfaced_as_failed() {
    let mut bridge = new_bridge(None).await;
    let mut cursor = bridge
        .run_with_command(
            replay_cmd("meta_transport_failed_run.jsonl"),
            None,
            Uuid::new_v4(),
        )
        .await
        .expect("bridge run setup");
    let events = drain(&mut bridge, &mut cursor).await;
    match events.last() {
        Some(BridgeEvent::Failed { detail, .. }) => {
            let detail = detail.as_deref().unwrap_or("");
            assert!(detail.contains("transport error"), "{detail}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn terminal_before_run_start_is_surfaced_as_failed() {
    // A run that fails before `run.lifecycle.started` must still complete setup
    // and surface the real reason, not a generic setup crash.
    let script = r#"
        printf '%s\n' '{"record_type":"reconciliation","payload_type":"runtime.command.accepted","payload":{"kind":"command_accepted"},"stream":{"id":"01a0b6be-4608-7a63-92a0-ecd8ed7c4c7b","kind":"session"}}'
        printf '%s\n' '{"record_type":"event","payload_type":"run.terminal.failed","payload":{"kind":"run_terminal","terminal":"failed","text":"","reason":"unauthorized"},"stream":{"id":"01a0b6be-4608-7a63-92a0-ecd8ed7c4c7b","kind":"session"}}'
        sleep 0.2
    "#;
    let mut bridge = new_bridge(None).await;
    let mut cursor = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .expect("setup should end on a terminal record");
    assert_eq!(cursor.thread_id, ECHO_SESSION);
    let events = drain(&mut bridge, &mut cursor).await;
    assert!(
        matches!(events.last(), Some(BridgeEvent::Failed { detail: Some(d), .. })
            if d == "muse run failed: unauthorized"),
        "{events:?}"
    );
}

#[tokio::test]
async fn missing_session_stream_falls_back_to_run_id() {
    let script = r#"
        printf '%s\n' '{"record_type":"event","payload_type":"run.lifecycle.started","payload":{"kind":"run_started"}}'
        printf '%s\n' '{"record_type":"event","payload_type":"run.terminal.completed","payload":{"kind":"run_terminal","terminal":"completed","text":"ok"}}'
        sleep 0.2
    "#;
    let run_id = Uuid::new_v4();
    let mut bridge = new_bridge(None).await;
    let cursor = bridge
        .run_with_command(fake_cmd(script), None, run_id)
        .await
        .expect("setup");
    // A bare UUID — never a `muse-` prefixed id `--session-id` would reject.
    assert_eq!(cursor.thread_id, run_id.to_string());
}

#[tokio::test]
async fn unrecognized_first_frame_fails_setup_loudly() {
    // The schema the original bridge assumed. If Muse's envelope changes, setup
    // must fail on the first frame naming the schema — not stream the whole run
    // unmapped and surface as "agent stdout closed" at EOF.
    let script = r#"
        printf '%s\n' '{"type":"system","subtype":"init","session_id":"muse_fake_001"}'
        printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"all done"}'
        sleep 0.2
    "#;
    let mut bridge = new_bridge(None).await;
    let err = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .err()
        .expect("setup must fail on a non-record frame");
    let msg = format!("{err:#}");
    assert!(msg.contains("not a record envelope"), "{msg}");
}

#[tokio::test]
async fn stdout_closed_before_run_start_fails_setup() {
    let script = r#"
        printf '%s\n' '{"record_type":"reconciliation","payload_type":"runtime.command.accepted","payload":{"kind":"command_accepted"}}'
    "#;
    let mut bridge = new_bridge(None).await;
    let err = bridge
        .run_with_command(fake_cmd(script), None, Uuid::new_v4())
        .await
        .err()
        .expect("setup must fail on EOF");
    assert!(
        format!("{err:#}").contains("before emitting run.lifecycle.started"),
        "{err:#}"
    );
}

#[tokio::test]
async fn warm_returns_resume_session_id_without_spawning() {
    // warm must not spawn a process; it only echoes a known resume id so the
    // cloud can keep its session pointer stable until the first turn.
    let mut bridge = new_bridge(Some(ECHO_SESSION)).await;
    let warmed = bridge.warm(&PathBuf::from("/tmp")).await.expect("warm");
    assert_eq!(warmed.as_deref(), Some(ECHO_SESSION));
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
    // Replays the real echo run up to `run.lifecycle.started`, then blocks on a
    // long sleep, so the subprocess stays alive after run setup. Dropping the
    // bridge (without a graceful shutdown) must kill it rather than leave it
    // orphaned to completion.
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(r#"head -n 4 "$1"; sleep 30"#)
        .arg("sh")
        .arg(fixture("echo_completed_run.jsonl"));
    let mut bridge = new_bridge(None).await;
    let _cursor = bridge
        .run_with_command(cmd, None, Uuid::new_v4())
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
    let mut bridge = new_bridge(None).await;
    let mut cursor = bridge
        .run_with_command(replay_cmd("echo_completed_run.jsonl"), None, Uuid::new_v4())
        .await
        .expect("run setup");
    let events = drain(&mut bridge, &mut cursor).await;
    assert!(matches!(events.last(), Some(BridgeEvent::Completed { .. })));

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
