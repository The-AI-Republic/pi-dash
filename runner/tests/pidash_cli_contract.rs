//! Contract test for the `pidash` CRUD subcommands.
//!
//! Each test stands up a tiny HTTP fake on loopback, points an `ApiClient`
//! at it, and exercises the subcommand logic through the same functions the
//! CLI dispatches to. Goals:
//!
//! - verify the X-Api-Key header and URL shape the CLI produces
//! - verify the three-step identifier → UUID resolution in
//!   `cli::resolve::resolve_issue` + `resolve_state_name`
//! - verify HTTP status → CLI exit-code mapping (`api_client::EXIT_*`)

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pidash::api_client::{
    ApiClient, CliEnv, EXIT_AUTH, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_SERVER, EXIT_THROTTLED,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    api_key: Option<String>,
    #[allow(dead_code)]
    body: String,
}

#[derive(Debug, Clone)]
struct CannedResponse {
    status: u16,
    status_text: &'static str,
    body: String,
}

impl CannedResponse {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            status_text: "OK",
            body: body.into(),
        }
    }
}

type Handler = Box<dyn Fn(&RecordedRequest) -> CannedResponse + Send + Sync>;

struct Fake {
    addr: SocketAddr,
    _handle: tokio::task::JoinHandle<()>,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
}

async fn start_fake(handler: Handler) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded_srv = recorded.clone();
    let handler = Arc::new(handler);
    let _handle = tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let recorded_c = recorded_srv.clone();
            let handler_c = handler.clone();
            tokio::spawn(async move {
                handle_conn(socket, recorded_c, handler_c).await;
            });
        }
    });
    Fake {
        addr,
        _handle,
        recorded,
    }
}

async fn handle_conn(
    mut socket: TcpStream,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    handler: Arc<Handler>,
) {
    // Read until we've seen the full request (headers + body by Content-Length).
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
            let need = idx + 4 + content_length;
            if buf.len() >= need {
                break;
            }
        }
    }
    let Some(idx) = headers_end else { return };
    let head = String::from_utf8_lossy(&buf[..idx]).to_string();
    let body = String::from_utf8_lossy(&buf[idx + 4..]).to_string();

    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("").to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut api_key = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("X-Api-Key: ") {
            api_key = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("x-api-key: ") {
            api_key = Some(rest.to_string());
        }
    }

    let req = RecordedRequest {
        method,
        path,
        api_key,
        body,
    };
    recorded.lock().unwrap().push(req.clone());

    let resp = handler(&req);
    let payload = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        resp.status,
        resp.status_text,
        resp.body.len(),
        resp.body
    );
    let _ = socket.write_all(payload.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(3) {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
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

fn client(fake: &Fake) -> ApiClient {
    let env = CliEnv {
        api_url: format!("http://{}", fake.addr),
        workspace_slug: "acme".into(),
        token: "test-token".into(),
    };
    ApiClient::new(env).unwrap()
}

#[tokio::test]
async fn workspace_me_hits_users_me_with_api_key() {
    let fake = start_fake(Box::new(|_req| {
        CannedResponse::ok(r#"{"id":"u1","email":"bot@example.com"}"#)
    }))
    .await;

    let client = client(&fake);
    let resp = tokio::time::timeout(Duration::from_secs(5), client.get("users/me/"))
        .await
        .unwrap()
        .expect("expected success");
    assert_eq!(resp["email"], "bot@example.com");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(recorded[0].path, "/api/v1/users/me/");
    assert_eq!(recorded[0].api_key.as_deref(), Some("test-token"));
}

#[tokio::test]
async fn resolve_issue_extracts_id_and_project() {
    let fake = start_fake(Box::new(|req| {
        assert_eq!(req.path, "/api/v1/workspaces/acme/work-items/ENG-1/");
        CannedResponse::ok(
            r#"{"id":"00000000-0000-0000-0000-000000000001","project":"00000000-0000-0000-0000-0000000000aa","name":"demo"}"#,
        )
    }))
    .await;
    let client = client(&fake);
    let issue = pidash::cli::resolve::resolve_issue(&client, "ENG-1")
        .await
        .expect("resolve_issue");
    assert_eq!(issue.id, "00000000-0000-0000-0000-000000000001");
    assert_eq!(issue.project_id, "00000000-0000-0000-0000-0000000000aa");
}

#[tokio::test]
async fn resolve_state_name_is_case_insensitive() {
    let fake = start_fake(Box::new(|req| {
        assert!(req.path.ends_with("/states/"));
        CannedResponse::ok(
            r#"[
                {"id":"00000000-0000-0000-0000-0000000000b1","name":"Todo","group":"unstarted"},
                {"id":"00000000-0000-0000-0000-0000000000b2","name":"In Progress","group":"started"},
                {"id":"00000000-0000-0000-0000-0000000000b3","name":"Done","group":"completed"}
            ]"#,
        )
    }))
    .await;
    let client = client(&fake);
    let uuid = pidash::cli::resolve::resolve_state_name(
        &client,
        "00000000-0000-0000-0000-0000000000aa",
        "in progress",
    )
    .await
    .expect("state name resolved");
    assert_eq!(uuid, "00000000-0000-0000-0000-0000000000b2");
}

#[tokio::test]
async fn resolve_state_name_errors_when_missing() {
    let fake = start_fake(Box::new(|_req| {
        CannedResponse::ok(
            r#"[{"id":"00000000-0000-0000-0000-0000000000b1","name":"Todo","group":"unstarted"}]"#,
        )
    }))
    .await;
    let client = client(&fake);
    let err = pidash::cli::resolve::resolve_state_name(
        &client,
        "00000000-0000-0000-0000-0000000000aa",
        "Blocked",
    )
    .await
    .expect_err("should 404");
    assert_eq!(err.exit_code, EXIT_NOT_FOUND);
}

#[tokio::test]
async fn http_404_maps_to_exit_not_found() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 404,
        status_text: "Not Found",
        body: r#"{"error":"nope"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client
        .get("workspaces/acme/work-items/ZZZ-99/")
        .await
        .expect_err("404");
    assert_eq!(err.exit_code, EXIT_NOT_FOUND);
}

#[tokio::test]
async fn http_401_maps_to_exit_auth() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 401,
        status_text: "Unauthorized",
        body: r#"{"error":"bad token"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("401");
    assert_eq!(err.exit_code, EXIT_AUTH);
}

#[tokio::test]
async fn http_403_maps_to_exit_auth() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 403,
        status_text: "Forbidden",
        body: r#"{"error":"forbidden"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("403");
    assert_eq!(err.exit_code, EXIT_AUTH);
}

#[tokio::test]
async fn http_400_maps_to_exit_invalid() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 400,
        status_text: "Bad Request",
        body: r#"{"error":"nope"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("400");
    assert_eq!(err.exit_code, EXIT_INVALID);
}

#[tokio::test]
async fn http_409_maps_to_exit_invalid() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 409,
        status_text: "Conflict",
        body: r#"{"error":"conflict"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("409");
    assert_eq!(err.exit_code, EXIT_INVALID);
}

#[tokio::test]
async fn http_422_maps_to_exit_invalid() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 422,
        status_text: "Unprocessable Entity",
        body: r#"{"error":"bad body"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("422");
    assert_eq!(err.exit_code, EXIT_INVALID);
}

#[tokio::test]
async fn http_429_maps_to_exit_throttled() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 429,
        status_text: "Too Many Requests",
        body: r#"{"error":"slow down"}"#.into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("429");
    assert_eq!(err.exit_code, EXIT_THROTTLED);
}

#[tokio::test]
async fn http_500_maps_to_exit_server() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 500,
        status_text: "Server Error",
        body: "boom".into(),
    }))
    .await;
    let client = client(&fake);
    let err = client.get("users/me/").await.expect_err("500");
    assert_eq!(err.exit_code, EXIT_SERVER);
}
