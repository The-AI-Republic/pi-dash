//! End-to-end Codex bridge test. A shell subprocess plays the role of
//! `codex app-server` and emits canned JSON-RPC responses; the Bridge drives
//! it through `initialize → thread/start → turn/start → turn/completed`.

use pidash::codex::app_server::AppServer;
use pidash::codex::bridge::{Bridge, BridgeEvent, RunPayload};
use pidash::codex::jsonrpc::{self, Incoming};
use pidash::codex::schema::{ClientInfo, InitializeParams, TurnInputItem, TurnStartParams};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

async fn wait_completed(bridge: &mut Bridge, cursor: &mut pidash::codex::bridge::BridgeCursor) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let Some(frame) = tokio::time::timeout(Duration::from_secs(1), bridge.next_frame())
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        for ev in cursor.translate(frame) {
            if let BridgeEvent::Completed { .. } = ev {
                return;
            }
        }
    }
    panic!("expected turn/completed to produce Completed event");
}

fn fake_codex_script() -> &'static str {
    // The script reads four lines from stdin (initialize, initialized,
    // thread/start, turn/start) and emits a deterministic sequence of
    // responses + one terminal notification.
    r#"
        set -e
        # initialize
        read _
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        # initialized notification (no response expected)
        read _
        # thread/start
        read thread
        case "$thread" in
          *'"sandbox":"danger-full-access"'*'"approvalPolicy":"never"'*) ;;
          *) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"expected bypassed codex thread/start policy"}}'; exit 0;;
        esac
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_fake_001"}}'
        # turn/start
        read _
        # one benign item/agentMessage/delta (gets forwarded to local history only)
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"text":"hello"}}'
        # terminal: turn/completed with a done payload
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","summary":"fake run"}}}'
        # keep stdin open so the Bridge can still write if it wants.
        sleep 0.3
    "#
}

#[tokio::test]
async fn warm_bridge_reuses_initialized_thread_across_turns() {
    let script = r#"
        set -e
        read init
        case "$init" in *'"method":"initialize"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"expected initialize"}}'; exit 0;; esac
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read initialized
        case "$initialized" in *'"method":"initialized"'*) ;; *) exit 1;; esac
        read thread
        case "$thread" in *'"method":"thread/start"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"expected thread/start"}}'; exit 0;; esac
        case "$thread" in *'"sandbox":"danger-full-access"'*'"approvalPolicy":"never"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"expected bypassed codex thread/start policy"}}'; exit 0;; esac
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_chat"}}'
        read turn1
        case "$turn1" in *'"method":"turn/start"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"code":-32600,"message":"expected first turn/start"}}'; exit 0;; esac
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","turn":1}}}'
        read turn2
        case "$turn2" in *'"method":"turn/start"'*) ;; *) printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"code":-32600,"message":"expected second turn/start"}}'; exit 0;; esac
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success","done":{"status":"ok","turn":2}}}'
        sleep 0.1
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd)
        .await
        .expect("spawn fake codex");
    let mut bridge = Bridge::from_server(server, None);

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let thread_id = bridge.warm(&cwd).await.expect("warm bridge");
    assert_eq!(thread_id, "th_chat");

    let first = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "first".into(),
        model: None,
    };
    let mut first_cursor = bridge.run(&first, &cwd).await.expect("first turn setup");
    assert_eq!(first_cursor.thread_id, "th_chat");
    wait_completed(&mut bridge, &mut first_cursor).await;

    let second = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "second".into(),
        model: None,
    };
    let mut second_cursor = bridge.run(&second, &cwd).await.expect("second turn setup");
    assert_eq!(second_cursor.thread_id, "th_chat");
    wait_completed(&mut bridge, &mut second_cursor).await;
}

#[tokio::test]
async fn multiplexed_sessions_route_frames_to_their_own_threads() {
    // One engine process hosts two conversations as separate threads. The fake
    // app-server hands out `th_a` and `th_b`, then emits interleaved frames
    // each stamped with its `threadId`. The Bridge must demultiplex them so
    // each thread's cursor sees only its own events, in order (delta before
    // completed), and neither turn stalls the other.
    let script = r#"
        set -e
        read _                                  # initialize
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read _                                  # initialized (no response)
        read _                                  # thread/start for session A
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_a"}}'
        read _                                  # thread/start for session B
        printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"threadId":"th_b"}}'
        read _                                  # turn/start A (no response)
        read _                                  # turn/start B (no response)
        # Interleaved, thread-stamped notifications: A and B streaming at once.
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_a","text":"a1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th_b","text":"b1"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_a","conclusion":"success","done":{"status":"ok","who":"a"}}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th_b","conclusion":"success","done":{"status":"ok","who":"b"}}}'
        sleep 0.3
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd)
        .await
        .expect("spawn fake codex");
    let mut bridge = Bridge::from_server(server, None);
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let th_a = bridge.warm_session("A", &cwd).await.expect("warm A");
    let th_b = bridge.warm_session("B", &cwd).await.expect("warm B");
    assert_eq!(th_a, "th_a");
    assert_eq!(th_b, "th_b");
    assert_eq!(bridge.live_thread_count(), 0, "no turns started yet");

    bridge
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
    bridge
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
    assert_eq!(bridge.live_thread_count(), 2, "two live threads");

    // Collect the demuxed events per thread until both turns complete.
    let mut a_completed = false;
    let mut b_completed = false;
    let mut a_saw_own_delta = false;
    let mut b_saw_own_delta = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline && !(a_completed && b_completed) {
        let Some((thread_id, events)) =
            tokio::time::timeout(Duration::from_secs(1), bridge.next_session_event())
                .await
                .ok()
                .flatten()
        else {
            continue;
        };
        for ev in events {
            match ev {
                BridgeEvent::Raw { method, params, .. } => {
                    // Isolation: every frame routed to a thread carries that
                    // thread's id — never the other conversation's.
                    let stamped = params.get("threadId").and_then(|v| v.as_str());
                    assert_eq!(
                        stamped,
                        Some(thread_id.as_str()),
                        "frame {method} routed to the wrong thread"
                    );
                    if thread_id == "th_a" && params.get("text").and_then(|v| v.as_str()) == Some("a1")
                    {
                        a_saw_own_delta = true;
                    }
                    if thread_id == "th_b" && params.get("text").and_then(|v| v.as_str()) == Some("b1")
                    {
                        b_saw_own_delta = true;
                    }
                }
                BridgeEvent::Completed { done_payload, .. } => {
                    // Ordering: the delta must arrive before completion.
                    if thread_id == "th_a" {
                        assert!(a_saw_own_delta, "A completed before its own delta");
                        assert_eq!(done_payload["who"], "a");
                        a_completed = true;
                    } else if thread_id == "th_b" {
                        assert!(b_saw_own_delta, "B completed before its own delta");
                        assert_eq!(done_payload["who"], "b");
                        b_completed = true;
                    }
                }
                _ => {}
            }
        }
    }
    assert!(a_completed, "thread A never completed");
    assert!(b_completed, "thread B never completed");

    // Closing one session drops its thread but keeps the process (and B) alive.
    assert_eq!(bridge.release_session("A").as_deref(), Some("th_a"));
    assert_eq!(bridge.live_thread_count(), 1, "A released, B still live");
    assert_eq!(bridge.release_session("A"), None, "double-release is a no-op");
}

#[tokio::test]
async fn bridge_happy_path_drives_fake_codex_to_completion() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(fake_codex_script());
    let server = AppServer::spawn_command(cmd)
        .await
        .expect("spawn fake codex");
    let mut bridge = Bridge::from_server(server, None);

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let payload = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "hi".into(),
        model: None,
    };
    let mut cursor = bridge.run(&payload, &cwd).await.expect("bridge run setup");
    assert_eq!(cursor.thread_id, "th_fake_001");

    let mut saw_completed = false;
    let mut saw_raw = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let Some(frame) =
            tokio::time::timeout(Duration::from_secs(1), bridge.server.inbound.recv())
                .await
                .ok()
                .flatten()
        else {
            continue;
        };
        for ev in cursor.translate(frame) {
            match ev {
                BridgeEvent::Raw { .. } => saw_raw = true,
                BridgeEvent::Completed { done_payload, .. } => {
                    saw_completed = true;
                    assert_eq!(done_payload["status"], "ok");
                }
                _ => {}
            }
        }
        if saw_completed {
            break;
        }
    }
    assert!(saw_raw, "expected at least one raw notification");
    assert!(
        saw_completed,
        "expected turn/completed to produce Completed event"
    );
}

#[tokio::test]
async fn bridge_reports_codex_crash_on_early_exit() {
    // Script exits immediately; init request should fail with a useful error.
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("exit 0");
    let server = AppServer::spawn_command(cmd).await.expect("spawn");
    let mut bridge = Bridge::from_server(server, None);

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let payload = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "x".into(),
        model: None,
    };
    let result = bridge.run(&payload, &cwd).await;
    assert!(
        result.is_err(),
        "expected an error when codex exits before responding"
    );
}

#[tokio::test]
async fn bridge_forwards_approval_request_event() {
    let script = r#"
        set -e
        read _
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
        read _
        read _
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th_fake"}}'
        read _
        printf '%s\n' '{"jsonrpc":"2.0","method":"item/commandExecution/requestApproval","params":{"approval_id":"a-1","command":"rm -rf /tmp/x"}}'
        sleep 0.1
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let server = AppServer::spawn_command(cmd).await.expect("spawn");
    let mut bridge = Bridge::from_server(server, None);
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let payload = RunPayload {
        run_id: Uuid::new_v4(),
        prompt: "do it".into(),
        model: None,
    };
    let mut cursor = bridge.run(&payload, &cwd).await.unwrap();
    let mut saw = false;
    for _ in 0..8 {
        let Some(frame) =
            tokio::time::timeout(Duration::from_secs(1), bridge.server.inbound.recv())
                .await
                .ok()
                .flatten()
        else {
            continue;
        };
        for ev in cursor.translate(frame) {
            if let BridgeEvent::ApprovalRequest { approval_id, .. } = ev {
                assert_eq!(approval_id, "a-1");
                saw = true;
            }
        }
        if saw {
            break;
        }
    }
    assert!(saw, "expected approval request event");
}

/// Reads a message sent on stdin by echoing an `Incoming::Notification` helper.
#[tokio::test]
async fn app_server_reads_line_delimited_json() {
    let script = r#"
        read _
        printf '%s\n' '{"jsonrpc":"2.0","method":"custom/ping","params":{"ok":true}}'
        sleep 0.05
    "#;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    let mut server = AppServer::spawn_command(cmd).await.unwrap();
    server.send_raw("{\"x\":1}").await.ok();
    let frame = tokio::time::timeout(Duration::from_secs(1), server.inbound.recv())
        .await
        .expect("timeout")
        .expect("closed");
    match frame {
        Incoming::Notification { method, .. } => assert_eq!(method, "custom/ping"),
        _ => panic!("expected notification, got {frame:?}"),
    }
}

#[test]
fn incoming_parses_frames_without_jsonrpc_field() {
    let response: Incoming = serde_json::from_str(r#"{"id":1,"result":{"ok":true}}"#)
        .expect("response without jsonrpc should parse");
    match response {
        Incoming::Response { id, result, .. } => {
            assert_eq!(id, 1);
            assert_eq!(result.expect("result")["ok"], true);
        }
        other => panic!("expected response, got {other:?}"),
    }

    let notification: Incoming =
        serde_json::from_str(r#"{"method":"turn/completed","params":{"conclusion":"success"}}"#)
            .expect("notification without jsonrpc should parse");
    match notification {
        Incoming::Notification { method, params, .. } => {
            assert_eq!(method, "turn/completed");
            assert_eq!(params["conclusion"], "success");
        }
        other => panic!("expected notification, got {other:?}"),
    }
}

#[test]
fn codex_request_params_serialize_in_v2_shape() {
    let init = jsonrpc::request(
        1,
        "initialize",
        &InitializeParams {
            client_info: ClientInfo {
                name: "pidash".into(),
                version: "0".into(),
            },
        },
    )
    .expect("serialize initialize");
    let init: serde_json::Value = serde_json::from_str(&init).unwrap();
    assert_eq!(init["params"]["clientInfo"]["name"], "pidash");

    let turn = jsonrpc::request(
        2,
        "turn/start",
        &TurnStartParams {
            thread_id: "thread-123".into(),
            input: vec![TurnInputItem {
                item_type: "text".into(),
                text: "hello".into(),
            }],
            model: Some("gpt-5-codex".into()),
            effort: None,
        },
    )
    .expect("serialize turn/start");
    let turn: serde_json::Value = serde_json::from_str(&turn).unwrap();
    assert_eq!(turn["params"]["threadId"], "thread-123");
    assert_eq!(turn["params"]["input"][0]["type"], "text");
    assert_eq!(turn["params"]["input"][0]["text"], "hello");
}
