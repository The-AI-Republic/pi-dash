use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use chrono::{Duration as ChronoDuration, Utc};
use pidash::cloud::http::{
    CredentialsHandle, RunnerCloudClient, RunnerCredentials, SharedHttpTransport,
};
use pidash::cloud::protocol::{ClientMsg, Envelope, RunEventRecord};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    idempotency_key: Option<String>,
    body: String,
}

async fn start_fake_cloud(
    runner_id: Uuid,
    run_id: Uuid,
) -> (
    SocketAddr,
    Arc<Mutex<Vec<RecordedRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let recorded_srv = recorded.clone();
    let handle = tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let recorded = recorded_srv.clone();
            tokio::spawn(async move {
                handle_conn(socket, recorded, runner_id, run_id).await;
            });
        }
    });
    (addr, recorded, handle)
}

async fn handle_conn(
    mut socket: TcpStream,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    runner_id: Uuid,
    run_id: Uuid,
) {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let mut headers_end = None;
    loop {
        let n = match socket.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(idx) = find_header_end(&buf) {
            headers_end = Some(idx);
            let content_length = content_length(&buf[..idx]).unwrap_or(0);
            if buf.len() >= idx + 4 + content_length {
                break;
            }
        }
    }
    let Some(idx) = headers_end else { return };
    let head = String::from_utf8_lossy(&buf[..idx]).to_string();
    let body = String::from_utf8_lossy(&buf[idx + 4..]).to_string();

    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut authorization = None;
    let mut idempotency_key = None;
    for line in lines {
        if let Some(rest) = line
            .strip_prefix("Authorization: ")
            .or_else(|| line.strip_prefix("authorization: "))
        {
            authorization = Some(rest.to_string());
        } else if let Some(rest) = line
            .strip_prefix("Idempotency-Key: ")
            .or_else(|| line.strip_prefix("idempotency-key: "))
        {
            idempotency_key = Some(rest.to_string());
        }
    }
    recorded.lock().unwrap().push(RecordedRequest {
        method: method.clone(),
        path: path.clone(),
        authorization,
        idempotency_key,
        body,
    });

    let refresh_path = format!("/api/v1/runner/runners/{runner_id}/refresh/");
    let events_path = format!("/api/v1/runner/runs/{run_id}/events/");
    let response = if method == "POST" && path == refresh_path {
        let exp = (Utc::now() + ChronoDuration::hours(1)).to_rfc3339();
        format!(
            r#"{{"refresh_token":"refresh-2","access_token":"access-1","access_token_expires_at":"{exp}","refresh_token_generation":2}}"#
        )
    } else if method == "POST" && path == events_path {
        r#"{"ok":true,"accepted":1}"#.to_string()
    } else {
        r#"{"error":"not found"}"#.to_string()
    };
    let status = if response.contains("not found") {
        "404 Not Found"
    } else {
        "200 OK"
    };
    let payload = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.len(),
        response
    );
    let _ = socket.write_all(payload.as_bytes()).await;
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(head: &[u8]) -> Option<usize> {
    let head = std::str::from_utf8(head).ok()?;
    for line in head.lines() {
        if let Some(rest) = line
            .strip_prefix("Content-Length: ")
            .or_else(|| line.strip_prefix("content-length: "))
        {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[tokio::test]
async fn http_transport_posts_run_events_batch_to_events_endpoint() {
    let runner_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let (addr, recorded, _server) = start_fake_cloud(runner_id, run_id).await;
    let temp = tempfile::tempdir().unwrap();
    let creds = CredentialsHandle::new(
        temp.path().join("credentials.toml"),
        RunnerCredentials {
            runner_id,
            name: "runner".into(),
            refresh_token: "refresh-1".into(),
            refresh_token_generation: 1,
        },
    );
    let transport = SharedHttpTransport::new(format!("http://{addr}")).unwrap();
    let client = RunnerCloudClient::new(runner_id, creds, transport);

    let env = Envelope::for_runner(
        runner_id,
        ClientMsg::RunEvents {
            run_id,
            events: vec![RunEventRecord {
                seq: 1,
                kind: "assistant/message".into(),
                payload: serde_json::json!({
                    "schema": "runner_event_summary_v1",
                    "summary": "raw method=assistant/message",
                }),
            }],
        },
    );
    client.dispatch_client_msg(env).await.unwrap();

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[0].path,
        format!("/api/v1/runner/runners/{runner_id}/refresh/")
    );
    assert_eq!(
        recorded[1].path,
        format!("/api/v1/runner/runs/{run_id}/events/")
    );
    assert_eq!(recorded[1].method, "POST");
    assert_eq!(
        recorded[1].authorization.as_deref(),
        Some("Bearer access-1")
    );
    assert!(recorded[1].idempotency_key.is_some());
    let body: serde_json::Value = serde_json::from_str(&recorded[1].body).unwrap();
    assert_eq!(body["type"], "run_events");
    assert_eq!(body["run_id"], run_id.to_string());
    assert_eq!(body["events"][0]["seq"], 1);
    assert_eq!(
        body["events"][0]["payload"]["summary"],
        "raw method=assistant/message"
    );
}
