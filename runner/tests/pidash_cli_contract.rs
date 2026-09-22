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
    run_id: Option<String>,
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
    let mut run_id = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("X-Api-Key: ") {
            api_key = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("x-api-key: ") {
            api_key = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("X-Pi-Dash-Run-Id: ") {
            run_id = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("x-pi-dash-run-id: ") {
            run_id = Some(rest.to_string());
        }
    }

    let req = RecordedRequest {
        method,
        path,
        api_key,
        run_id,
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
        run_id: None,
    };
    ApiClient::new(env).unwrap()
}

fn client_for_run(fake: &Fake, run_id: &str) -> ApiClient {
    let env = CliEnv {
        api_url: format!("http://{}", fake.addr),
        workspace_slug: "acme".into(),
        token: "test-token".into(),
        run_id: Some(run_id.to_string()),
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

// ---------------------------------------------------------------------------
// Run identity: every write from inside an agent run carries X-Pi-Dash-Run-Id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn writes_carry_the_run_id_header_reads_do_not() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok("{}"))).await;
    let run_id = "123e4567-e89b-12d3-a456-426614174000";
    let client = client_for_run(&fake, run_id);

    client.get("users/me/").await.expect("get");
    client
        .patch("workspaces/acme/projects/p/work-items/i/", &serde_json::json!({"state": "s"}))
        .await
        .expect("patch");
    client
        .post("workspaces/acme/projects/p/work-items/i/comments/", &serde_json::json!({"comment_html": "x"}))
        .await
        .expect("post");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(recorded[0].run_id, None, "reads are attributed to nobody");
    assert_eq!(recorded[1].method, "PATCH");
    assert_eq!(recorded[1].run_id.as_deref(), Some(run_id));
    assert_eq!(recorded[2].method, "POST");
    assert_eq!(recorded[2].run_id.as_deref(), Some(run_id));
}

#[tokio::test]
async fn writes_without_a_run_id_send_no_header() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok("{}"))).await;
    let client = client(&fake);
    client
        .patch("workspaces/acme/projects/p/work-items/i/", &serde_json::json!({"state": "s"}))
        .await
        .expect("patch");
    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].run_id, None, "operator use must look like a human move");
}

#[tokio::test]
async fn run_yield_posts_outcome_to_the_run_yield_route() {
    let fake = start_fake(Box::new(|req| {
        assert_eq!(req.method, "POST");
        CannedResponse::ok(r#"{"ok":true,"outcome":"done"}"#)
    }))
    .await;
    let run_id = "123e4567-e89b-12d3-a456-426614174000";
    let client = client_for_run(&fake, run_id);
    pidash::cli::run_cmd::cmd_yield(
        &client,
        pidash::cli::run_cmd::YieldArgs {
            outcome: pidash::cli::run_cmd::Outcome::Done,
            note: Some("  approved  ".into()),
            run_id: None,
        },
    )
    .await
    .expect("yield");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].path,
        format!("/api/v1/workspaces/acme/agent-runs/{run_id}/yield/")
    );
    assert_eq!(recorded[0].run_id.as_deref(), Some(run_id));
    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
    assert_eq!(body["outcome"], "done");
    assert_eq!(body["note"], "approved");
}

#[tokio::test]
async fn run_yield_without_a_run_id_fails_before_any_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok("{}"))).await;
    let client = client(&fake);
    let err = pidash::cli::run_cmd::cmd_yield(
        &client,
        pidash::cli::run_cmd::YieldArgs {
            outcome: pidash::cli::run_cmd::Outcome::Progressed,
            note: None,
            run_id: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.exit_code, EXIT_INVALID);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// `pidash page …` — the read path into project pages (PDASHOSS01-185).
// ---------------------------------------------------------------------------

const PAGE_ENVELOPE: &str = r#"{"count":1,"next_cursor":"20:1:0","prev_cursor":"20:-1:1","results":[{"id":"00000000-0000-0000-0000-0000000000f1","name":"Conventions"}]}"#;

const PAGE_DETAIL: &str = r##"{"id":"00000000-0000-0000-0000-0000000000f1","name":"Conventions","description_html":"<h1>Rules</h1>","description_stripped":"Rules","description_markdown":"# Rules"}"##;

#[tokio::test]
async fn page_list_hits_the_project_pages_route() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_ENVELOPE))).await;

    pidash::cli::page::cmd_list(
        &client(&fake),
        pidash::cli::page::ListArgs {
            project: "ENG".into(),
            cursor: None,
            per_page: None,
            include_archived: false,
        },
    )
    .await
    .expect("page list");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(recorded[0].path, "/api/v1/workspaces/acme/projects/ENG/pages/");
    assert_eq!(recorded[0].api_key.as_deref(), Some("test-token"));
}

#[tokio::test]
async fn page_list_forwards_pagination_and_include_archived() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_ENVELOPE))).await;

    pidash::cli::page::cmd_list(
        &client(&fake),
        pidash::cli::page::ListArgs {
            project: "00000000-0000-0000-0000-0000000000aa".into(),
            cursor: Some("20:1:0".into()),
            per_page: Some(50),
            include_archived: true,
        },
    )
    .await
    .expect("page list");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(
        recorded[0].path,
        "/api/v1/workspaces/acme/projects/00000000-0000-0000-0000-0000000000aa/pages/\
         ?cursor=20%3A1%3A0&per_page=50&include_archived=true"
    );
}

#[tokio::test]
async fn page_list_maps_a_non_member_403_to_the_auth_exit_code() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 403,
        status_text: "Forbidden",
        body: r#"{"error":"You don't have the required permissions."}"#.into(),
    }))
    .await;

    let err = pidash::cli::page::cmd_list(
        &client(&fake),
        pidash::cli::page::ListArgs {
            project: "ENG".into(),
            cursor: None,
            per_page: None,
            include_archived: false,
        },
    )
    .await
    .expect_err("403 must not be silently swallowed");

    assert_eq!(err.exit_code, EXIT_AUTH);
}

#[tokio::test]
async fn page_get_hits_the_page_detail_route() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_get(
        &client(&fake),
        pidash::cli::page::GetArgs {
            page_id: "00000000-0000-0000-0000-0000000000f1".into(),
            project: "ENG".into(),
            body_only: false,
        },
    )
    .await
    .expect("page get");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(
        recorded[0].path,
        "/api/v1/workspaces/acme/projects/ENG/pages/00000000-0000-0000-0000-0000000000f1/"
    );
}

#[tokio::test]
async fn page_get_body_only_uses_the_same_route_and_succeeds() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_get(
        &client(&fake),
        pidash::cli::page::GetArgs {
            page_id: "00000000-0000-0000-0000-0000000000f1".into(),
            project: "ENG".into(),
            body_only: true,
        },
    )
    .await
    .expect("page get --body-only");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].path,
        "/api/v1/workspaces/acme/projects/ENG/pages/00000000-0000-0000-0000-0000000000f1/"
    );
}

#[tokio::test]
async fn page_get_body_only_errors_when_the_server_omits_markdown() {
    let fake = start_fake(Box::new(|_req| {
        CannedResponse::ok(r#"{"id":"00000000-0000-0000-0000-0000000000f1","description_html":"<p>x</p>"}"#)
    }))
    .await;

    let err = pidash::cli::page::cmd_get(
        &client(&fake),
        pidash::cli::page::GetArgs {
            page_id: "00000000-0000-0000-0000-0000000000f1".into(),
            project: "ENG".into(),
            body_only: true,
        },
    )
    .await
    .expect_err("a server without page markdown must be reported");

    assert_eq!(err.exit_code, EXIT_SERVER);
}

#[tokio::test]
async fn page_get_rejects_a_non_uuid_page_id_before_any_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let err = pidash::cli::page::cmd_get(
        &client(&fake),
        pidash::cli::page::GetArgs {
            page_id: "release-checklist".into(),
            project: "ENG".into(),
            body_only: false,
        },
    )
    .await
    .expect_err("a slug is not a page id");

    assert_eq!(err.exit_code, EXIT_INVALID);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// `pidash page create|update|archive|unarchive` — the write path
// (PDASHOSS01-200).
// ---------------------------------------------------------------------------

const PAGE_ID: &str = "00000000-0000-0000-0000-0000000000f1";
const PARENT_PAGE_ID: &str = "00000000-0000-0000-0000-0000000000f2";

fn created(body: &str) -> CannedResponse {
    CannedResponse {
        status: 201,
        status_text: "Created",
        body: body.into(),
    }
}

fn body_json(req: &RecordedRequest) -> serde_json::Value {
    serde_json::from_str(&req.body).expect("request body is JSON")
}

fn no_page_body() -> pidash::cli::page::PageBodyArgs {
    pidash::cli::page::PageBodyArgs {
        body: None,
        body_file: None,
    }
}

fn update_args() -> pidash::cli::page::UpdateArgs {
    pidash::cli::page::UpdateArgs {
        page_id: PAGE_ID.into(),
        project: "ENG".into(),
        title: None,
        body: no_page_body(),
        parent: None,
        clear_parent: false,
        access: None,
    }
}

#[tokio::test]
async fn page_create_posts_to_the_project_pages_route() {
    let fake = start_fake(Box::new(|_req| created(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_create(
        &client_for_run(&fake, "run-7"),
        pidash::cli::page::CreateArgs {
            project: "ENG".into(),
            title: "Conventions".into(),
            body: pidash::cli::page::PageBodyArgs {
                body: Some("# Rules".into()),
                body_file: None,
            },
            parent: Some(PARENT_PAGE_ID.into()),
            access: Some(pidash::cli::page::PageAccess::Private),
        },
    )
    .await
    .expect("page create");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path, "/api/v1/workspaces/acme/projects/ENG/pages/");
    assert_eq!(recorded[0].run_id.as_deref(), Some("run-7"));
    assert_eq!(
        body_json(&recorded[0]),
        serde_json::json!({
            "name": "Conventions",
            "description_markdown": "# Rules",
            "parent": PARENT_PAGE_ID,
            "access": 1,
        })
    );
}

#[tokio::test]
async fn page_create_reads_the_body_from_a_file() {
    let fake = start_fake(Box::new(|_req| created(PAGE_DETAIL))).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.md");
    std::fs::write(&path, "# From a file\n\n- one\n").unwrap();

    pidash::cli::page::cmd_create(
        &client(&fake),
        pidash::cli::page::CreateArgs {
            project: "ENG".into(),
            title: "Conventions".into(),
            body: pidash::cli::page::PageBodyArgs {
                body: None,
                body_file: Some(path),
            },
            parent: None,
            access: None,
        },
    )
    .await
    .expect("page create --body-file");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(
        body_json(&recorded[0]),
        serde_json::json!({"name": "Conventions", "description_markdown": "# From a file\n\n- one\n"})
    );
}

#[tokio::test]
async fn page_update_sends_only_the_provided_keys() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_update(
        &client(&fake),
        pidash::cli::page::UpdateArgs {
            body: pidash::cli::page::PageBodyArgs {
                body: Some("# New body".into()),
                body_file: None,
            },
            ..update_args()
        },
    )
    .await
    .expect("page update");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "PATCH");
    assert_eq!(
        recorded[0].path,
        format!("/api/v1/workspaces/acme/projects/ENG/pages/{PAGE_ID}/")
    );
    assert_eq!(
        body_json(&recorded[0]),
        serde_json::json!({"description_markdown": "# New body"})
    );
}

#[tokio::test]
async fn page_update_clear_parent_sends_null() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_update(
        &client(&fake),
        pidash::cli::page::UpdateArgs {
            title: Some("Renamed".into()),
            clear_parent: true,
            ..update_args()
        },
    )
    .await
    .expect("page update --clear-parent");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(
        body_json(&recorded[0]),
        serde_json::json!({"name": "Renamed", "parent": null})
    );
}

#[tokio::test]
async fn page_update_without_fields_fails_before_any_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let err = pidash::cli::page::cmd_update(&client(&fake), update_args())
        .await
        .expect_err("an update with nothing to change is invalid");

    assert_eq!(err.exit_code, EXIT_INVALID);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn page_update_rejects_a_non_uuid_page_id_before_any_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let err = pidash::cli::page::cmd_update(
        &client(&fake),
        pidash::cli::page::UpdateArgs {
            page_id: "conventions".into(),
            title: Some("Renamed".into()),
            ..update_args()
        },
    )
    .await
    .expect_err("a slug is not a page id");

    assert_eq!(err.exit_code, EXIT_INVALID);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn page_update_maps_a_locked_page_409_to_invalid() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 409,
        status_text: "Conflict",
        body: r#"{"error":"Page is locked"}"#.into(),
    }))
    .await;

    let err = pidash::cli::page::cmd_update(
        &client(&fake),
        pidash::cli::page::UpdateArgs {
            title: Some("Renamed".into()),
            ..update_args()
        },
    )
    .await
    .expect_err("409 must surface");

    assert_eq!(err.exit_code, EXIT_INVALID);
}

#[tokio::test]
async fn page_update_maps_a_live_service_503_to_server() {
    let fake = start_fake(Box::new(|_req| CannedResponse {
        status: 503,
        status_text: "Service Unavailable",
        body: r#"{"error":"live document service unavailable"}"#.into(),
    }))
    .await;

    let err = pidash::cli::page::cmd_update(
        &client(&fake),
        pidash::cli::page::UpdateArgs {
            body: pidash::cli::page::PageBodyArgs {
                body: Some("# x".into()),
                body_file: None,
            },
            ..update_args()
        },
    )
    .await
    .expect_err("503 must surface");

    assert_eq!(err.exit_code, EXIT_SERVER);
}

#[tokio::test]
async fn page_archive_posts_to_the_archive_route() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_archive(
        &client(&fake),
        pidash::cli::page::ArchiveArgs {
            page_id: PAGE_ID.into(),
            project: "ENG".into(),
        },
    )
    .await
    .expect("page archive");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(
        recorded[0].path,
        format!("/api/v1/workspaces/acme/projects/ENG/pages/{PAGE_ID}/archive/")
    );
}

#[tokio::test]
async fn page_unarchive_deletes_the_archive_route() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    pidash::cli::page::cmd_unarchive(
        &client_for_run(&fake, "run-8"),
        pidash::cli::page::ArchiveArgs {
            page_id: PAGE_ID.into(),
            project: "ENG".into(),
        },
    )
    .await
    .expect("page unarchive");

    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "DELETE");
    assert_eq!(
        recorded[0].path,
        format!("/api/v1/workspaces/acme/projects/ENG/pages/{PAGE_ID}/archive/")
    );
    assert_eq!(recorded[0].run_id.as_deref(), Some("run-8"));
}

#[tokio::test]
async fn page_archive_rejects_a_non_uuid_page_id_before_any_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let err = pidash::cli::page::cmd_archive(
        &client(&fake),
        pidash::cli::page::ArchiveArgs {
            page_id: "conventions".into(),
            project: "ENG".into(),
        },
    )
    .await
    .expect_err("a slug is not a page id");

    assert_eq!(err.exit_code, EXIT_INVALID);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

// The remaining page tests drive the real `pidash` binary, because stdin
// (`--body-file -`), clap conflicts, and the process exit code are only
// observable end to end. An empty config dir makes `CliEnv::resolve` fall
// through to the env vars pointing at the fake.

struct BinOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn run_pidash(fake: &Fake, args: &[&str], stdin: &str) -> BinOutput {
    let config_dir = tempfile::tempdir().unwrap();
    let api_url = format!("http://{}", fake.addr);
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let stdin = stdin.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(env!("CARGO_BIN_EXE_pidash"))
            .env("PIDASH_CONFIG_DIR", config_dir.path())
            .env("PIDASH_DATA_DIR", config_dir.path().join("data"))
            .env("PIDASH_API_URL", api_url)
            .env("PIDASH_WORKSPACE_SLUG", "acme")
            .env("PIDASH_TOKEN", "test-token")
            .env_remove("PIDASH_RUN_ID")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pidash");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("wait pidash");
        BinOutput {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn page_create_body_file_dash_reads_stdin() {
    let fake = start_fake(Box::new(|_req| created(PAGE_DETAIL))).await;

    let out = run_pidash(
        &fake,
        &["page", "create", "--project", "ENG", "--title", "X", "--body-file", "-"],
        "# Piped\n\nfrom stdin\n",
    )
    .await;

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let printed: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(printed["id"], PAGE_ID);
    let recorded = fake.recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(
        body_json(&recorded[0]),
        serde_json::json!({"name": "X", "description_markdown": "# Piped\n\nfrom stdin\n"})
    );
}

#[tokio::test]
async fn page_body_and_body_file_conflict_at_the_cli() {
    let fake = start_fake(Box::new(|_req| created(PAGE_DETAIL))).await;

    let out = run_pidash(
        &fake,
        &[
            "page", "create", "--project", "ENG", "--title", "X", "--body", "x", "--body-file", "-",
        ],
        "",
    )
    .await;

    assert_ne!(out.code, Some(0));
    assert!(out.stderr.contains("cannot be used with"), "stderr: {}", out.stderr);
    assert!(fake.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn page_update_without_fields_exits_2_without_a_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let out = run_pidash(&fake, &["page", "update", PAGE_ID, "--project", "ENG"], "").await;

    assert_eq!(out.code, Some(EXIT_INVALID), "stderr: {}", out.stderr);
    assert!(out.stderr.contains("nothing to update"));
    assert!(fake.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn page_update_non_uuid_page_id_exits_2_without_a_request() {
    let fake = start_fake(Box::new(|_req| CannedResponse::ok(PAGE_DETAIL))).await;

    let out = run_pidash(
        &fake,
        &["page", "update", "conventions", "--project", "ENG", "--title", "Y"],
        "",
    )
    .await;

    assert_eq!(out.code, Some(EXIT_INVALID), "stderr: {}", out.stderr);
    assert!(fake.recorded.lock().unwrap().is_empty());
}
