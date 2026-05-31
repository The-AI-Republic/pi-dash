use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use pidash::cli::runner::AddArgs;
use pidash::config::schema::{AgentKind, MAX_RUNNERS_PER_DAEMON};
use pidash::util::paths::Paths;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    api_key: Option<String>,
    body: String,
}

struct Fake {
    addr: SocketAddr,
    _handle: tokio::task::JoinHandle<()>,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
}

async fn start_fake() -> Fake {
    start_fake_with_workspaces(r#"{"workspaces":[{"slug":"acme","name":"Acme"}]}"#.to_string())
        .await
}

async fn start_fake_with_workspaces(workspaces_body: String) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded_srv = recorded.clone();
    let workspaces_body = Arc::new(workspaces_body);
    let _handle = tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let recorded = recorded_srv.clone();
            let workspaces_body = workspaces_body.clone();
            tokio::spawn(async move {
                handle_conn(socket, recorded, workspaces_body).await;
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
    workspaces_body: Arc<String>,
) {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 2048];
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
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut api_key = None;
    for line in lines {
        if let Some(rest) = line
            .strip_prefix("X-Api-Key: ")
            .or_else(|| line.strip_prefix("x-api-key: "))
        {
            api_key = Some(rest.to_string());
        }
    }

    recorded.lock().unwrap().push(RecordedRequest {
        method: method.clone(),
        path: path.clone(),
        api_key,
        body,
    });

    let body = match (method.as_str(), path.as_str()) {
        ("POST", "/api/v1/auth/device/start/") => {
            r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"http://127.0.0.1/device","expires_in":600,"interval":1}"#
        }
        ("POST", "/api/v1/auth/device/token/") => {
            r#"{"access_token":"cli-token","user_email":"dev@example.com"}"#
        }
        ("GET", "/api/v1/auth/workspaces/") => workspaces_body.as_str(),
        _ => r#"{"error":"unexpected_request"}"#,
    };
    let status = if body.contains("unexpected_request") {
        "404 Not Found"
    } else {
        "200 OK"
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = socket.write_all(resp.as_bytes()).await;
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

fn paths(root: &std::path::Path) -> Paths {
    Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        runtime_dir: root.join("runtime"),
    }
}

fn write_config_with_runner_count(paths: &Paths, cloud_url: &str, runner_count: usize) {
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();

    let mut toml = format!(
        r#"
version = 2

[daemon]
cloud_url = "{cloud_url}"
"#
    );
    for i in 0..runner_count {
        toml.push_str(&format!(
            r#"
[[runner]]
name = "runner_{i:03}"
runner_id = "{}"
workspace_slug = "acme"
project_slug = "TEST"

[runner.workspace]
working_dir = "/tmp/pidash-runner-{i:03}"
"#,
            uuid::Uuid::new_v4()
        ));
    }
    std::fs::write(paths.config_path(), toml).unwrap();
}

#[tokio::test]
async fn runner_add_checks_local_cap_before_auth_bootstrap() {
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_config_with_runner_count(&paths, "http://127.0.0.1:9", MAX_RUNNERS_PER_DAEMON);

    let err = pidash::cli::runner::add(
        AddArgs {
            url: None,
            name: None,
            project: "TEST".to_string(),
            workspace: None,
            pod: None,
            working_dir: None,
            agent: AgentKind::Codex,
        },
        &paths,
    )
    .await
    .expect_err("runner add should fail at the local cap before auth");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("daemon already at the 50-runner cap"),
        "unexpected cap error: {msg}",
    );

    let config = std::fs::read_to_string(paths.config_path()).unwrap();
    assert!(
        !config.contains("token = \"cli-token\""),
        "cap failure should not run auth bootstrap"
    );
}

#[tokio::test]
async fn runner_add_rejects_invalid_name_before_auth_bootstrap() {
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_config_with_runner_count(&paths, "http://127.0.0.1:9", 0);

    let err = pidash::cli::runner::add(
        AddArgs {
            url: None,
            name: Some("test runner".to_string()),
            project: "TEST".to_string(),
            workspace: None,
            pod: None,
            working_dir: None,
            agent: AgentKind::Codex,
        },
        &paths,
    )
    .await
    .expect_err("invalid runner name should fail before auth");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("invalid --name \"test runner\""),
        "unexpected name error: {msg}",
    );
    assert!(
        msg.contains("runner name must start"),
        "unexpected validation error: {msg}",
    );

    let config = std::fs::read_to_string(paths.config_path()).unwrap();
    assert!(
        !config.contains("token = "),
        "name validation failure should not run auth bootstrap"
    );
}

#[tokio::test]
async fn runner_add_bootstraps_auth_when_cli_token_is_missing() {
    let fake = start_fake().await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_config_with_runner_count(&paths, &format!("http://{}", fake.addr), 0);

    let err = pidash::cli::runner::add(
        AddArgs {
            url: None,
            name: None,
            project: "TEST".to_string(),
            workspace: None,
            pod: None,
            working_dir: None,
            agent: AgentKind::Codex,
        },
        &paths,
    )
    .await
    .expect_err("fake cloud should reject runner creation after auth bootstrap");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("cloud rejected runner creation"),
        "unexpected error after auth bootstrap: {msg}",
    );

    let config = std::fs::read_to_string(paths.config_path()).unwrap();
    assert!(config.contains("token = \"cli-token\""));
    assert!(config.contains("workspace_slug = \"acme\""));

    let recorded = fake.recorded.lock().unwrap();
    assert!(
        recorded
            .iter()
            .any(|r| r.method == "POST" && r.path == "/api/v1/auth/device/start/"),
        "device auth was not started: {recorded:?}",
    );
    assert!(
        recorded.iter().any(|r| {
            r.method == "GET"
                && r.path == "/api/v1/auth/workspaces/"
                && r.api_key.as_deref() == Some("cli-token")
        }),
        "workspace binding was not resolved with the new token: {recorded:?}",
    );
    assert!(
        recorded.iter().any(|r| {
            r.method == "POST"
                && r.path == "/api/v1/runner/runners/"
                && r.api_key.as_deref() == Some("cli-token")
        }),
        "runner creation was not attempted with the bootstrapped token: {recorded:?}",
    );
    assert!(
        !recorded
            .iter()
            .any(|r| r.path == "/api/v1/runner/projects/"),
        "auth bootstrap must suppress login's inline runner prompt: {recorded:?}",
    );
}

#[tokio::test]
async fn runner_add_auth_bootstrap_uses_explicit_workspace_without_prompt() {
    let fake = start_fake_with_workspaces(
        r#"{"workspaces":[{"slug":"acme","name":"Acme"},{"slug":"beta","name":"Beta"}]}"#
            .to_string(),
    )
    .await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_config_with_runner_count(&paths, &format!("http://{}", fake.addr), 0);

    let err = pidash::cli::runner::add(
        AddArgs {
            url: None,
            name: None,
            project: "TEST".to_string(),
            workspace: Some("beta".to_string()),
            pod: None,
            working_dir: None,
            agent: AgentKind::Codex,
        },
        &paths,
    )
    .await
    .expect_err("fake cloud should reject runner creation after auth bootstrap");

    let msg = format!("{err:#}");
    assert!(
        msg.contains("cloud rejected runner creation"),
        "unexpected error after auth bootstrap: {msg}",
    );

    let config = std::fs::read_to_string(paths.config_path()).unwrap();
    assert!(config.contains("token = \"cli-token\""));
    assert!(config.contains("workspace_slug = \"beta\""));

    let recorded = fake.recorded.lock().unwrap();
    let create_req = recorded
        .iter()
        .find(|r| r.method == "POST" && r.path == "/api/v1/runner/runners/")
        .expect("runner creation was not attempted");
    let body: serde_json::Value =
        serde_json::from_str(&create_req.body).expect("runner create body should be JSON");
    assert_eq!(body["workspace_slug"], "beta");
}
