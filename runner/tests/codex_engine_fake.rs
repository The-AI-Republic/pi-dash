//! Shared-engine test. A shell subprocess plays `codex app-server`; a
//! [`SharedCodexEngine`] owns it in an actor task. Two lanes — modelling a chat
//! turn and an issue run — acquire their own threads on the one process and
//! drain their own event streams concurrently, asserting isolation, per-thread
//! ordering, and that neither lane stalls the other.

use pidash::codex::app_server::AppServer;
use pidash::codex::bridge::{Bridge, BridgeEvent, RunPayload};
use pidash::codex::engine::{EngineSession, SharedCodexEngine};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Drain one session's events until it completes, returning the deltas it saw
/// (in order) and whether it completed cleanly. Fails loudly on timeout so a
/// stalled lane surfaces as a test failure rather than a hang.
async fn drain_until_complete(mut session: EngineSession) -> (Vec<String>, bool) {
    let mut deltas = Vec::new();
    let mut completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let Some(ev) = tokio::time::timeout(Duration::from_secs(1), session.events.recv())
            .await
            .ok()
            .flatten()
        else {
            // Either a 1s lull (keep waiting until the deadline) or the channel
            // closed (None → the inner recv returned None, break).
            continue;
        };
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
    (deltas, completed)
}

#[tokio::test]
async fn shared_engine_streams_chat_and_run_concurrently() {
    // The fake hands out `th_chat` then `th_run`, then interleaves two
    // thread-stamped streams at once. Each stream carries several deltas so a
    // lane that stalled the other (e.g. a mutex held across `next_session_event`)
    // would show up as missing frames on the starved lane.
    // The two lanes run sequentially at setup, so stdin arrives as
    // thread/start(chat) id2, turn/start(chat), thread/start(run) id4,
    // turn/start(run) — codex ids are allocated sequentially per process.
    let script = r#"
        set -e
        read _                                  # initialize
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read _                                  # initialized (no response)
        read _                                  # thread/start for chat (id 2)
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_chat"}}'
        read _                                  # turn/start chat (id 3, no response)
        read _                                  # thread/start for run (id 4)
        printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"threadId":"th_run"}}'
        read _                                  # turn/start run (id 5, no response)
        # Two conversations streaming at the same time, interleaved.
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_chat","text":"c1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_run","text":"r1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_chat","text":"c2"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_run","text":"r2"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_run","conclusion":"success","done":{"status":"ok","who":"run"}}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_chat","conclusion":"success","done":{"status":"ok","who":"chat"}}}'
        sleep 0.5
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd).await.expect("spawn fake codex");
    let engine = SharedCodexEngine::from_bridge(Bridge::from_server(server, None));
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Two independent lanes, each with its own handle — as the chat lane and
    // the issue-run lane would hold in the daemon.
    let chat = engine.handle();
    let run = engine.handle();

    let chat_session = chat
        .run_session(
            "chat",
            &RunPayload {
                run_id: Uuid::new_v4(),
                prompt: "hi".into(),
                model: None,
            },
            &cwd,
        )
        .await
        .expect("start chat turn");
    let run_session = run
        .run_session(
            "run",
            &RunPayload {
                run_id: Uuid::new_v4(),
                prompt: "do it".into(),
                model: None,
            },
            &cwd,
        )
        .await
        .expect("start issue run");

    assert_eq!(chat_session.thread_id, "th_chat");
    assert_eq!(run_session.thread_id, "th_run");
    assert_eq!(engine.handle().live_thread_count().await, 2);

    // Drain both concurrently. Neither task is allowed to block the other; if
    // the engine serialized frame reads behind one lane, the other would time
    // out with missing deltas.
    let (chat_res, run_res) = tokio::join!(
        drain_until_complete(chat_session),
        drain_until_complete(run_session),
    );

    let (chat_deltas, chat_done) = chat_res;
    let (run_deltas, run_done) = run_res;

    // Isolation + ordering: each lane saw only its own deltas, in order.
    assert_eq!(chat_deltas, vec!["c1", "c2"], "chat lane saw wrong frames");
    assert_eq!(run_deltas, vec!["r1", "r2"], "run lane saw wrong frames");
    assert!(chat_done, "chat turn never completed");
    assert!(run_done, "issue run never completed");
}

#[tokio::test]
async fn releasing_a_session_keeps_the_engine_and_other_sessions_alive() {
    let script = r#"
        set -e
        read _                                  # initialize
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read _                                  # initialized
        read _                                  # thread/start A (id 2)
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_a"}}'
        read _                                  # turn/start A (id 3, no response)
        read _                                  # thread/start B (id 4)
        printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"threadId":"th_b"}}'
        read _                                  # turn/start B (id 5, no response)
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_a","conclusion":"success","done":{"status":"ok"}}}'
        sleep 0.5
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd).await.expect("spawn fake codex");
    let engine = SharedCodexEngine::from_bridge(Bridge::from_server(server, None));
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let h = engine.handle();

    let a = h
        .run_session(
            "A",
            &RunPayload {
                run_id: Uuid::new_v4(),
                prompt: "a".into(),
                model: None,
            },
            &cwd,
        )
        .await
        .expect("run A");
    let _b = h
        .run_session(
            "B",
            &RunPayload {
                run_id: Uuid::new_v4(),
                prompt: "b".into(),
                model: None,
            },
            &cwd,
        )
        .await
        .expect("run B");
    assert_eq!(h.live_thread_count().await, 2, "two live threads");

    // Closing A drops its thread but leaves the process (and B) running.
    assert_eq!(h.release_session("A").await.as_deref(), Some("th_a"));
    assert_eq!(h.live_thread_count().await, 1, "A released, B still live");
    assert_eq!(h.release_session("A").await, None, "double-release is a no-op");

    // A's event stream is closed now that its route is gone.
    let mut a = a;
    assert!(
        a.events.recv().await.is_none(),
        "released session's stream should close"
    );

    // The engine still answers — the process is alive.
    assert_eq!(h.live_thread_count().await, 1);
}

#[tokio::test]
async fn engine_handle_exposes_process_observability() {
    // The daemon's lanes read pid / exit-watch / recent-stderr / default-model
    // from the engine the same way they read them from a per-lane bridge. The
    // handle must surface all four without an actor round-trip on the hot path.
    // Emit the stderr line up front: this test never starts a session, so the
    // fake is never sent `initialize` and must not block on stdin first.
    let script = r#"
        set -e
        printf 'boot: fake codex up\n' 1>&2
        sleep 0.5
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd).await.expect("spawn fake codex");
    let engine =
        SharedCodexEngine::from_bridge(Bridge::from_server(server, Some("gpt-x".into())));
    let h = engine.handle();

    // pid captured at spawn; process still alive so exit watch is unset.
    let ph = h.process_handle();
    assert!(ph.pid.is_some(), "engine handle should surface the pid");
    assert!(
        ph.exit_rx.borrow().is_none(),
        "process is alive, no exit snapshot yet"
    );

    // Default model threaded from the bridge.
    assert_eq!(h.model_default(), Some("gpt-x"));

    // Recent-stderr snapshots the shared ring (the fake wrote one boot line).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let snap = h.recent_stderr().await;
        if snap.lines.iter().any(|l| l.contains("boot: fake codex up")) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "stderr line never surfaced: {snap:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
