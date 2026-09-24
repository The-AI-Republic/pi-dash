//! Seam test for slice 2b: the supervisor's chat lane and issue-run lane drive
//! ONE shared codex engine through the `AgentBridge::SharedCodex` wrapper — the
//! exact surface (`warm` / `run` / `next_events` / `shutdown`) both lanes use.
//! A shell subprocess plays `codex app-server`; the wrapper presents it as an
//! `AgentBridge` per session. Asserts the two lanes stream concurrently through
//! one process, each seeing only its own events in order, neither stalling, and
//! that ending one session (`shutdown` → release) keeps the process and the
//! other session live.

use pidash::agent::{AgentBridge, AgentCursor, BridgeEvent, RunPayload};
use pidash::codex::app_server::AppServer;
use pidash::codex::bridge::Bridge;
use pidash::codex::engine::SharedCodexEngine;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Drain one lane's turn through the `AgentBridge` surface until it completes,
/// collecting the assistant-delta text it saw (in order). Fails loudly on a
/// stall so a starved lane surfaces as a test failure rather than a hang.
async fn drain_turn(bridge: &mut AgentBridge, cursor: &mut AgentCursor) -> (Vec<String>, bool) {
    let mut deltas = Vec::new();
    let mut completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let Ok(batch) =
            tokio::time::timeout(Duration::from_secs(1), bridge.next_events(cursor)).await
        else {
            continue; // 1s lull — keep waiting until the deadline.
        };
        let Some(events) = batch else {
            break; // stream closed
        };
        for ev in events {
            match ev {
                BridgeEvent::Raw { params, .. } => {
                    if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
                        deltas.push(text.to_string());
                    }
                }
                BridgeEvent::Completed { .. } => {
                    completed = true;
                    break;
                }
                _ => {}
            }
        }
        if completed {
            break;
        }
    }
    (deltas, completed)
}

fn payload(prompt: &str) -> RunPayload {
    RunPayload {
        run_id: Uuid::new_v4(),
        prompt: prompt.into(),
        model: None,
    }
}

#[tokio::test]
async fn chat_and_run_share_one_engine_through_agent_bridge() {
    // stdin order (serialized through the one engine actor): initialize (id1),
    // thread/start chat (id2), turn/start chat (id3), thread/start run (id4),
    // turn/start run (id5). The chat lane warms then runs; warm creates the
    // thread (id2) and run's warm_session reuses it, so the turn is id3 — the
    // same sequence the raw-handle engine test drives.
    let script = r#"
        set -e
        read _                                  # initialize
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read _                                  # initialized (no response)
        read _                                  # thread/start chat (id 2)
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_chat"}}'
        read _                                  # turn/start chat (id 3, no response)
        read _                                  # thread/start run (id 4)
        printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"threadId":"th_run"}}'
        read _                                  # turn/start run (id 5, no response)
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_chat","text":"c1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_run","text":"r1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_chat","text":"c2"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_run","text":"r2"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_run","conclusion":"success","done":{"status":"ok"}}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_chat","conclusion":"success","done":{"status":"ok"}}}'
        sleep 0.5
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd).await.expect("spawn fake codex");
    let engine = SharedCodexEngine::from_bridge(Bridge::from_server(server, None));
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // The two lanes as the daemon builds them: each holds an EngineHandle clone,
    // keyed by its own session id.
    let mut chat_bridge = AgentBridge::shared_codex(engine.handle(), Uuid::new_v4().to_string());
    let mut run_bridge = AgentBridge::shared_codex(engine.handle(), Uuid::new_v4().to_string());

    // The shared engine's process outlives any one session, so the chat lane
    // treats a mid-turn `agent stdout closed` as a failed turn (respawn +
    // resume on the next message), not a lost runtime.
    assert!(
        chat_bridge.survives_process_exit(),
        "SharedCodex bridge must report it survives a process exit"
    );

    // Chat lane warms first (as ChatWorker::handle_warm does), then runs a turn.
    let warmed = chat_bridge.warm(&cwd).await.expect("warm chat");
    assert_eq!(warmed.as_deref(), Some("th_chat"));
    let mut chat_cursor = chat_bridge.run(&payload("hi"), &cwd).await.expect("chat turn");

    // Run lane goes straight to a one-shot turn (as AssignWorker does).
    let mut run_cursor = run_bridge
        .run_one_shot(&payload("do it"), &cwd)
        .await
        .expect("issue run");

    assert_eq!(chat_cursor.thread_id(), "th_chat");
    assert_eq!(run_cursor.thread_id(), "th_run");
    // Both threads are live on the one shared process.
    assert_eq!(engine.handle().live_thread_count().await, 2);
    // The wrapper reports codex regardless of lane.
    assert_eq!(chat_cursor.agent_kind(), "codex");
    assert_eq!(run_cursor.agent_kind(), "codex");

    // Drain both lanes at once. If the wrapper serialized the engine behind one
    // lane, the other would time out with missing deltas.
    let (chat_res, run_res) = tokio::join!(
        drain_turn(&mut chat_bridge, &mut chat_cursor),
        drain_turn(&mut run_bridge, &mut run_cursor),
    );
    let (chat_deltas, chat_done) = chat_res;
    let (run_deltas, run_done) = run_res;

    assert_eq!(chat_deltas, vec!["c1", "c2"], "chat lane saw wrong frames");
    assert_eq!(run_deltas, vec!["r1", "r2"], "run lane saw wrong frames");
    assert!(chat_done, "chat turn never completed");
    assert!(run_done, "issue run never completed");

    // Ending the chat (ChatWorker post-loop `bridge.shutdown`) releases its
    // thread but leaves the process — and the run lane's thread — alive.
    chat_bridge.shutdown(Duration::from_secs(1)).await.ok();
    assert_eq!(
        engine.handle().live_thread_count().await,
        1,
        "chat released; run thread still live on the same process"
    );
}
