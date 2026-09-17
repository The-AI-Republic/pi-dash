//! Crash / respawn / resume test for the shared codex engine.
//!
//! A first fake `codex app-server` (script A) crashes *mid-turn* — it streams a
//! delta and then exits without a `turn/completed`. The engine is given a
//! respawn factory that brings up a second fake (script B) which honours
//! `thread/resume` and finishes a fresh turn on the SAME thread id.
//!
//! Asserts the acceptance criterion: killing the engine mid-session respawns it
//! and resumes each live thread from its stored id, with the user seeing a
//! failed turn (the in-flight turn's stream closes without completing) rather
//! than a lost session (the next turn continues on the same thread).

use pidash::codex::app_server::AppServer;
use pidash::codex::bridge::{Bridge, BridgeEvent, RunPayload};
use pidash::codex::engine::{EngineSession, SharedCodexEngine};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// The first process: warms a thread, streams one delta, then dies mid-turn
/// (exits before `turn/completed`). Per-process JSON-RPC ids restart at 1, so
/// initialize=1, thread/start=2, turn/start=3.
const SCRIPT_CRASH_MID_TURN: &str = r#"
    set -e
    read _                                  # initialize
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
    read _                                  # initialized (no response)
    read _                                  # thread/start (id 2)
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th1"}}'
    read _                                  # turn/start (id 3, no response)
    printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th1","text":"a1"}}'
    # crash: exit mid-turn, before turn/completed
"#;

/// The respawn: a fresh process that accepts `thread/resume` for th1 and
/// finishes a new turn on it. Ids restart at 1: initialize=1, thread/resume=2,
/// turn/start=3.
const SCRIPT_RESUME: &str = r#"
    set -e
    read _                                  # initialize
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
    read _                                  # initialized (no response)
    read _                                  # thread/resume (id 2)
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{}}'
    read _                                  # turn/start (id 3, no response)
    printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th1","text":"b1"}}'
    printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th1","conclusion":"success","done":{"status":"ok"}}}'
    sleep 0.5
"#;

fn payload(prompt: &str) -> RunPayload {
    RunPayload {
        run_id: Uuid::new_v4(),
        prompt: prompt.into(),
        model: None,
    }
}

async fn spawn_fake(script: &str) -> AppServer {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    AppServer::spawn_command(cmd).await.expect("spawn fake codex")
}

/// Drain a session's events until its channel closes (crash or turn end),
/// returning the text deltas it saw and whether the turn completed cleanly.
async fn drain_until_closed(mut session: EngineSession) -> (Vec<String>, bool) {
    let mut deltas = Vec::new();
    let mut completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "drain timed out: deltas={deltas:?} completed={completed}"
        );
        match tokio::time::timeout(Duration::from_secs(1), session.events.recv()).await {
            Ok(Some(BridgeEvent::Raw { params, .. })) => {
                if let Some(t) = params.get("text").and_then(|v| v.as_str()) {
                    deltas.push(t.to_string());
                }
            }
            Ok(Some(BridgeEvent::Completed { .. })) => completed = true,
            Ok(Some(_)) => {}
            Ok(None) => break, // channel closed: crash or session released
            Err(_) => continue, // 1s lull; keep waiting until the deadline
        }
    }
    (deltas, completed)
}

#[tokio::test]
async fn engine_respawns_and_resumes_live_thread_after_crash() {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Initial process crashes mid-turn; the factory brings up a resume-capable
    // replacement each time it is called.
    let server_a = spawn_fake(SCRIPT_CRASH_MID_TURN).await;
    let engine = SharedCodexEngine::from_bridge_with_factory(
        Bridge::from_server(server_a, None),
        || async {
            let server = spawn_fake(SCRIPT_RESUME).await;
            Ok(Bridge::from_server(server, None))
        },
    );
    let h = engine.handle();

    // Turn 1 runs on the first process and is interrupted by the crash.
    let s1 = h
        .run_session("chat", &payload("hi"), &cwd)
        .await
        .expect("start turn 1");
    assert_eq!(s1.thread_id, "th1");
    let pid_a = h.process_handle().pid;

    let (d1, c1) = drain_until_closed(s1).await;
    // User sees a *failed turn*: a partial delta then the stream closes without
    // completing. The session is not lost — only this turn failed.
    assert_eq!(d1, vec!["a1"], "turn 1 should stream its partial delta");
    assert!(!c1, "turn 1 must NOT complete — the engine crashed mid-turn");

    // Turn 2 finds the engine dead, respawns it, resumes th1 from its stored id,
    // and completes on the SAME thread — the conversation continued.
    let s2 = h
        .run_session("chat", &payload("again"), &cwd)
        .await
        .expect("start turn 2 after respawn");
    assert_eq!(
        s2.thread_id, "th1",
        "turn 2 must resume the SAME thread id, not start a new one"
    );

    let pid_b = h.process_handle().pid;
    assert!(pid_a.is_some() && pid_b.is_some());
    assert_ne!(pid_a, pid_b, "respawn should replace the engine process");

    let (d2, c2) = drain_until_closed(s2).await;
    assert_eq!(d2, vec!["b1"], "turn 2 should stream on the resumed thread");
    assert!(c2, "turn 2 should complete on the respawned engine");
}
