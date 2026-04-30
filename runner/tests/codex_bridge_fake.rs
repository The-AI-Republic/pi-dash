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
        read _
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
        resume_thread_id: None,
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
        resume_thread_id: None,
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
        resume_thread_id: None,
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
