//! Fake cloud test: start an in-process tokio-tungstenite server, run the
//! runner's `Connection::open` against it, and verify the Hello/Welcome
//! handshake + a few message round-trips.

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use pidash::cloud::protocol::{ClientMsg, Envelope, RunnerStatus, ServerMsg, WIRE_VERSION};
use pidash::cloud::ws::{Connection, ConnectionLoop, run_connection};
use pidash::config::schema::Credentials;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc};
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

/// Fake cloud that accepts N connections, sends Welcome + immediately
/// closes each. Used to drive `ConnectionLoop` through repeated
/// reconnects so we can assert the `connected` notify fires on each one.
async fn start_flapping_cloud(connections: usize) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        for _ in 0..connections {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (mut tx, _rx) = ws_stream.split();
            let welcome = Envelope::new(ServerMsg::Welcome {
                server_time: Utc::now(),
                heartbeat_interval_secs: 25,
                protocol_version: WIRE_VERSION,
            });
            let _ = tx
                .send(Message::Text(serde_json::to_string(&welcome).unwrap()))
                .await;
            // Brief pause so the runner sees the Welcome before we close.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = tx.close().await;
        }
    });
    (addr, handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connection_loop_fires_connected_notify_on_each_handshake() {
    // Two consecutive WS connections — the second is a reconnect after
    // the first is force-closed by the fake cloud. The `connected`
    // notify must fire twice: once for cold-start, once for reconnect.
    // This is the hook the supervisor's hello-emitter watches to
    // re-emit Hellos so the cloud rebuilds `authorised_runners` for
    // the new consumer instance after a reconnect.
    let (addr, server_handle) = start_flapping_cloud(2).await;
    let cloud_url = format!("http://{}", addr);

    let creds = Credentials {
        token: None,
        runner_id: uuid::Uuid::new_v4(),
        runner_secret: "apd_rs_testsecret".into(),
        api_token: None,
        issued_at: Utc::now(),
    };

    let (out_tx, out_rx) = mpsc::channel::<Envelope<ClientMsg>>(8);
    // Drop the sender immediately. We don't need to push frames in this
    // test, and holding it open would deadlock ConnectionLoop's
    // `forward.await` after each WS close (the inner forward task parks
    // on `outbound.recv()` forever if the channel never closes).
    drop(out_tx);
    let (in_tx, mut in_rx) = mpsc::channel::<Envelope<ServerMsg>>(8);
    let connected = Arc::new(Notify::new());
    let shutdown = Arc::new(Notify::new());

    let (status_tx, status_rx) = tokio::sync::watch::channel(RunnerStatus::Idle);
    let (in_flight_tx, in_flight_rx) = tokio::sync::watch::channel::<Option<uuid::Uuid>>(None);
    // Hold the senders so the watch channels stay alive for the life of
    // the test — dropping them would close the receivers ConnectionLoop
    // is borrowing.
    let _keep_alive = (status_tx, in_flight_tx);

    // Count notifications on a background watcher mirroring the
    // supervisor's hello-emitter pattern.
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_for_watcher = counter.clone();
    let connected_for_watcher = connected.clone();
    let watcher = tokio::spawn(async move {
        loop {
            connected_for_watcher.notified().await;
            counter_for_watcher.fetch_add(1, Ordering::SeqCst);
        }
    });

    let loop_ = ConnectionLoop {
        cloud_url,
        creds,
        outbound: out_rx,
        inbound: in_tx,
        status_snapshot: status_rx,
        in_flight: in_flight_rx,
        shutdown: shutdown.clone(),
        connected,
    };
    let loop_handle = tokio::spawn(loop_.run());

    // Drain inbound — we don't care what's on it, but the consumer
    // dropping would close the channel and stop ConnectionLoop early.
    let drain = tokio::spawn(async move { while in_rx.recv().await.is_some() {} });

    // Wait for two notifications. Backoff's first reconnect delay is
    // bounded by ~2s + jitter; 15s gives plenty of slack.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while counter.load(Ordering::SeqCst) < 2 {
        if std::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let final_count = counter.load(Ordering::SeqCst);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), loop_handle).await;
    watcher.abort();
    drain.abort();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;

    assert_eq!(
        final_count, 2,
        "expected `connected` notify to fire on cold start AND on reconnect; \
         got {final_count} fires"
    );
}
