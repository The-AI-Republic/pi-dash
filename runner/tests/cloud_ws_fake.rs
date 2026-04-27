//! Fake cloud test: start an in-process tokio-tungstenite server, run the
//! runner's `Connection::open` against it, and verify the Hello/Welcome
//! handshake + a few message round-trips.

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use pidash::cloud::protocol::{ClientMsg, Envelope, RunnerStatus, ServerMsg, WIRE_VERSION};
use pidash::cloud::ws::{Connection, run_connection};
use pidash::config::schema::Credentials;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

async fn start_fake_cloud() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        let ws_stream = tokio_tungstenite::accept_hdr_async(
            stream,
            |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
             resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                for (k, v) in req.headers().iter() {
                    headers.push((k.as_str().to_string(), v.to_str().unwrap_or("").to_string()));
                }
                Ok(resp)
            },
        )
        .await
        .unwrap();

        let (mut tx, mut rx) = ws_stream.split();
        // Immediately send a Welcome.
        let welcome = Envelope::new(ServerMsg::Welcome {
            server_time: Utc::now(),
            heartbeat_interval_secs: 25,
            protocol_version: WIRE_VERSION,
        });
        tx.send(Message::Text(serde_json::to_string(&welcome).unwrap()))
            .await
            .unwrap();

        // Expect a Hello from the runner.
        let frame = rx.next().await.unwrap().unwrap();
        let text = frame.into_text().unwrap();
        let hello: Envelope<ClientMsg> = serde_json::from_str(&text).unwrap();
        match hello.body {
            ClientMsg::Hello { .. } => {}
            other => panic!("expected Hello, got {other:?}"),
        }

        // Send an Assign.
        let assign = Envelope::new(ServerMsg::Assign {
            run_id: uuid::Uuid::new_v4(),
            work_item_id: None,
            prompt: "test".into(),
            repo_url: None,
            repo_ref: None,
            git_work_branch: None,
            expected_codex_model: None,
            approval_policy_overrides: None,
            deadline: None,
            resume_thread_id: None,
        });
        tx.send(Message::Text(serde_json::to_string(&assign).unwrap()))
            .await
            .unwrap();
        // Keep the socket open briefly so the test has time to read.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });
    (addr, handle)
}

#[tokio::test]
async fn runner_connects_hello_welcome_and_receives_assign() {
    let (addr, server_handle) = start_fake_cloud().await;
    let cloud_url = format!("http://{}", addr);
    let creds = Credentials {
        token: None,
        runner_id: uuid::Uuid::new_v4(),
        runner_secret: "apd_rs_testsecret".into(),
        api_token: None,
        issued_at: Utc::now(),
    };

    let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
    let (in_tx, mut in_rx) = mpsc::channel::<Envelope<ServerMsg>>(8);

    let conn = Connection::open(&cloud_url, &creds).await.expect("open ws");
    // Send a Hello ourselves since ConnectionLoop normally does it.
    let hello = Envelope::new(ClientMsg::Hello {
        runner_id: creds.runner_id,
        version: "0.1.0".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        status: RunnerStatus::Idle,
        in_flight_run: None,
        protocol_version: WIRE_VERSION,
    });
    out_tx.send(hello).await.unwrap();

    // Drive the connection on a background task.
    let loop_handle = tokio::spawn(async move {
        let _ = run_connection(conn, out_rx, in_tx).await;
    });

    // First frame from the fake cloud should be Welcome.
    let welcome = tokio::time::timeout(std::time::Duration::from_secs(2), in_rx.recv())
        .await
        .expect("timeout waiting for welcome")
        .expect("channel closed");
    assert!(matches!(welcome.body, ServerMsg::Welcome { .. }));

    // Then an Assign.
    let assign = tokio::time::timeout(std::time::Duration::from_secs(2), in_rx.recv())
        .await
        .expect("timeout waiting for assign")
        .expect("channel closed");
    match assign.body {
        ServerMsg::Assign { prompt, .. } => assert_eq!(prompt, "test"),
        other => panic!("expected Assign, got {other:?}"),
    }

    drop(out_tx);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), loop_handle).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), server_handle).await;
}
