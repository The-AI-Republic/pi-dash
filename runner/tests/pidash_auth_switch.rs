// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash auth login` against a fake cloud: switching accounts on a host
//! must not present the previous user's `dev_machine_id` (the cloud refuses
//! another user's record), and must park / restore runners per user.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use pidash::cli::auth::login;
use pidash::config::file;
use pidash::util::paths::Paths;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const OLD_MACHINE: &str = "11111111-1111-4111-8111-111111111111";
const SURVIVOR: &str = "22222222-2222-4222-8222-222222222222";

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    api_key: Option<String>,
    body: String,
}

/// Who the fake cloud says the stored (pre-login) machine token belongs to.
#[derive(Clone, Copy)]
enum StoredTokenOwner {
    User(&'static str),
    /// `/users/me/` fails for it (cloud too old, token revoked, ...).
    Unknown,
}

struct Fake {
    addr: SocketAddr,
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

impl Fake {
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self, method: &str, path: &str) -> Vec<Recorded> {
        self.recorded
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == method && r.path == path)
            .cloned()
            .collect()
    }
}

/// The device flow signs in `new_user`; `/machine-token/` refuses
/// [`OLD_MACHINE`] (owned by somebody else) and resolves anything else to
/// [`SURVIVOR`].
async fn start_fake(new_user: &'static str, stored: StoredTokenOwner) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded: Arc<Mutex<Vec<Recorded>>> = Arc::default();
    let rec = recorded.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let rec = rec.clone();
            tokio::spawn(handle(socket, rec, new_user, stored));
        }
    });
    Fake { addr, recorded }
}

async fn handle(
    mut socket: TcpStream,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    new_user: &'static str,
    stored: StoredTokenOwner,
) {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 2048];
    let head_end = loop {
        let n = match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= i + 4 + len {
                break i;
            }
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let body = String::from_utf8_lossy(&buf[head_end + 4..]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let api_key = lines.find_map(|l| {
        l.strip_prefix("X-Api-Key: ")
            .or_else(|| l.strip_prefix("x-api-key: "))
            .map(str::to_string)
    });
    recorded.lock().unwrap().push(Recorded {
        method: method.clone(),
        path: path.clone(),
        api_key: api_key.clone(),
        body: body.clone(),
    });

    let user_json = |id: &str| format!(r#"{{"id":"{id}","email":"{id}@example.com"}}"#);
    let (status, reply) = match (method.as_str(), path.as_str()) {
        ("POST", "/api/v1/auth/device/start/") => (
            "200 OK",
            r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"http://127.0.0.1/device","expires_in":600,"interval":1}"#.to_string(),
        ),
        ("POST", "/api/v1/auth/device/token/") => (
            "200 OK",
            format!(r#"{{"access_token":"api-{new_user}","user_email":"{new_user}@example.com"}}"#),
        ),
        ("GET", "/api/v1/auth/workspaces/") => (
            "200 OK",
            r#"{"workspaces":[{"slug":"acme","name":"Acme"}]}"#.to_string(),
        ),
        ("GET", "/api/v1/users/me/") => match api_key.as_deref() {
            Some(k) if k == format!("api-{new_user}") => ("200 OK", user_json(new_user)),
            Some("mt_stored") => match stored {
                StoredTokenOwner::User(id) => ("200 OK", user_json(id)),
                StoredTokenOwner::Unknown => ("500 Internal Server Error", "{}".to_string()),
            },
            _ => ("401 Unauthorized", "{}".to_string()),
        },
        ("POST", "/api/v1/auth/machine-token/") => {
            if body.contains(OLD_MACHINE) && !matches!(stored, StoredTokenOwner::User(u) if u == new_user) {
                (
                    "404 Not Found",
                    r#"{"error":"dev_machine_not_found"}"#.to_string(),
                )
            } else {
                let machine = if body.contains(OLD_MACHINE) { OLD_MACHINE } else { SURVIVOR };
                (
                    "201 Created",
                    format!(
                        r#"{{"machine_token":"mt_{new_user}","workspace_slug":"acme","dev_machine_id":"{machine}","user":{}}}"#,
                        user_json(new_user)
                    ),
                )
            }
        }
        ("POST", "/api/v1/auth/revoke/") => ("200 OK", "{}".to_string()),
        _ => ("404 Not Found", r#"{"error":"unexpected_request"}"#.to_string()),
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let _ = socket.write_all(resp.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn paths(root: &std::path::Path) -> Paths {
    let paths = Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        runtime_dir: root.join("runtime"),
        machine_context_dir: root.join("config"),
    };
    paths.ensure().unwrap();
    paths
}

fn runner_toml(root: &std::path::Path, name: &str) -> String {
    format!(
        r#"
[[runner]]
name = "{name}"
runner_id = "{}"
workspace_slug = "acme"
project_slug = "PROJ"

[runner.workspace]
working_dir = "{}"
"#,
        uuid::Uuid::new_v4(),
        root.join(format!("wd-{name}")).display()
    )
}

/// A host signed in with `mt_stored` on [`OLD_MACHINE`], running `alpha`.
fn write_signed_in(paths: &Paths, root: &std::path::Path, cloud: &str, user: Option<&str>) {
    let user_lines = user
        .map(|u| format!("user_id = \"{u}\"\nuser_email = \"{u}@example.com\"\n"))
        .unwrap_or_default();
    let toml = format!(
        r#"version = 2

[daemon]
cloud_url = "{cloud}"
dev_machine_id = "{OLD_MACHINE}"

[cli]
token = "mt_stored"
workspace_slug = "acme"
{user_lines}{}"#,
        runner_toml(root, "alpha")
    );
    std::fs::write(paths.config_path(), toml).unwrap();
}

fn login_args() -> login::Args {
    login::Args {
        url: None,
        no_browser: true,
        force: false,
        workspace: None,
        device_code_stdin: false,
        yes: true,
    }
}

fn runner_names(paths: &Paths) -> Vec<String> {
    file::load_config(paths)
        .unwrap()
        .runners
        .into_iter()
        .map(|r| r.name)
        .collect()
}

fn parked_files(paths: &Paths) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(paths.parked_dir())
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn account_switch_omits_old_machine_id_parks_and_restores() {
    let fake = start_fake("u2", StoredTokenOwner::User("u1")).await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_signed_in(&paths, tmp.path(), &fake.url(), Some("u1"));
    // u2 used this host before: their runner is parked, waiting.
    std::fs::create_dir_all(paths.parked_dir()).unwrap();
    std::fs::write(
        paths.parked_dir().join("u2.toml"),
        format!("version = 1\n{}", runner_toml(tmp.path(), "beta")),
    )
    .unwrap();

    login::run_auth_only(login_args(), &paths)
        .await
        .expect("account switch signs in");

    let exchanges = fake.requests("POST", "/api/v1/auth/machine-token/");
    assert_eq!(
        exchanges.len(),
        1,
        "no refused first attempt: {exchanges:?}"
    );
    let body: serde_json::Value = serde_json::from_str(&exchanges[0].body).unwrap();
    assert_ne!(
        body["dev_machine_id"], OLD_MACHINE,
        "u1's dev machine must not be presented for u2"
    );
    assert_eq!(
        body["host_label"].as_str().map(|s| !s.is_empty()),
        Some(true)
    );

    let cfg = file::load_config(&paths).unwrap();
    let cli = cfg.cli.clone().unwrap();
    assert_eq!(cli.user_id.as_deref(), Some("u2"));
    assert_eq!(cli.token.as_deref(), Some("mt_u2"));
    assert_eq!(
        cfg.daemon
            .dev_machine_id
            .map(|id| id.to_string())
            .as_deref(),
        Some(SURVIVOR)
    );
    assert_eq!(runner_names(&paths), vec!["beta"]);
    assert_eq!(parked_files(&paths), vec!["u1.toml"]);
    let parked = std::fs::read_to_string(paths.parked_dir().join("u1.toml")).unwrap();
    assert!(parked.contains("alpha"), "{parked}");

    // The previous account's token was revoked with its own credential.
    let revokes = fake.requests("POST", "/api/v1/auth/revoke/");
    assert!(
        revokes
            .iter()
            .any(|r| r.api_key.as_deref() == Some("mt_stored")),
        "{revokes:?}"
    );
}

#[tokio::test]
async fn same_user_relogin_keeps_machine_id_and_runners() {
    let fake = start_fake("u1", StoredTokenOwner::User("u1")).await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_signed_in(&paths, tmp.path(), &fake.url(), Some("u1"));

    login::run_auth_only(login_args(), &paths).await.unwrap();

    let exchanges = fake.requests("POST", "/api/v1/auth/machine-token/");
    let body: serde_json::Value = serde_json::from_str(&exchanges[0].body).unwrap();
    assert_eq!(body["dev_machine_id"], OLD_MACHINE);
    assert_eq!(runner_names(&paths), vec!["alpha"]);
    assert!(parked_files(&paths).is_empty());
}

#[tokio::test]
async fn legacy_config_user_is_backfilled_before_deciding() {
    // No user_id stored, but the cloud says the stored token is u1's: the
    // backfill makes this an ordinary same-user re-login.
    let fake = start_fake("u1", StoredTokenOwner::User("u1")).await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_signed_in(&paths, tmp.path(), &fake.url(), None);

    login::run_auth_only(login_args(), &paths).await.unwrap();

    assert!(
        fake.requests("GET", "/api/v1/users/me/")
            .iter()
            .any(|r| r.api_key.as_deref() == Some("mt_stored")),
        "stored user was not looked up"
    );
    let exchanges = fake.requests("POST", "/api/v1/auth/machine-token/");
    let body: serde_json::Value = serde_json::from_str(&exchanges[0].body).unwrap();
    assert_eq!(body["dev_machine_id"], OLD_MACHINE);
    assert_eq!(runner_names(&paths), vec!["alpha"]);
    assert!(parked_files(&paths).is_empty());
}

#[tokio::test]
async fn unknown_previous_user_is_treated_as_a_switch() {
    let fake = start_fake("u2", StoredTokenOwner::Unknown).await;
    let tmp = tempdir().unwrap();
    let paths = paths(tmp.path());
    write_signed_in(&paths, tmp.path(), &fake.url(), None);

    login::run_auth_only(login_args(), &paths).await.unwrap();

    let exchanges = fake.requests("POST", "/api/v1/auth/machine-token/");
    assert_eq!(exchanges.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&exchanges[0].body).unwrap();
    assert_ne!(body["dev_machine_id"], OLD_MACHINE);

    assert!(
        runner_names(&paths).is_empty(),
        "old runners must not run as u2"
    );
    let parked = parked_files(&paths);
    assert_eq!(parked.len(), 1, "{parked:?}");
    assert!(parked[0].starts_with("unknown-"), "{parked:?}");
    let text = std::fs::read_to_string(paths.parked_dir().join(&parked[0])).unwrap();
    assert!(text.contains("alpha"), "parked runners are kept: {text}");
    assert_eq!(
        file::load_config(&paths)
            .unwrap()
            .cli
            .unwrap()
            .user_id
            .as_deref(),
        Some("u2")
    );
}
