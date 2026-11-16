//! The daemon's control socket must be reachable however deep its data
//! directory is (PIDESKAPP-48).
//!
//! The desktop app starts the daemon with `PIDASH_DATA_DIR` re-rooted under
//! `<app data>/managed/pidash/<workspace>/data`. On macOS that prefix alone is
//! about as long as `sockaddr_un.sun_path` (104 bytes; 108 on Linux), so a
//! socket bound under the data dir cannot be bound at all.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use pidash::cli::RunArgs;
use pidash::ipc::client::Client;
use pidash::util::paths::Paths;

/// A data dir shaped like the desktop's on macOS, for user `irichard` in
/// workspace `ai-republic`, re-rooted under `root`.
fn desktop_like_data_dir(root: &Path) -> PathBuf {
    root.join("Users/irichard/Library/Application Support/com.airepublic.pidash.oss")
        .join("managed/pidash/ai-republic/data")
}

fn write_config(paths: &Paths, root: &Path) {
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    let working_dir = root.join("wd");
    std::fs::write(
        paths.config_path(),
        format!(
            r#"
version = 2

[daemon]
cloud_url = "https://x"

[[runner]]
name = "t"
runner_id = "{}"
workspace_slug = "ai-republic"
project_slug = "TEST"

[runner.workspace]
working_dir = "{}"
"#,
            uuid::Uuid::new_v4(),
            working_dir.display(),
        ),
    )
    .unwrap();
}

/// Whether the daemon's control socket accepts a connection within a few
/// seconds of the daemon starting.
async fn socket_answers(socket: &Path) -> bool {
    socket_answers_within(socket, Duration::from_secs(4)).await
}

async fn socket_answers_within(socket: &Path, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if Client::connect(socket).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// The desktop's own launch, end to end: the real `pidash` binary, started
/// with the data dir deep under the app-data tree and the short socket the
/// app assigned, both in the environment. Without the assignment the daemon
/// would try `<data_dir>/runtime/pidash.sock`, which is too long to bind, and
/// run on with no control socket at all.
#[tokio::test]
async fn desktop_launched_daemon_is_reachable_under_a_deep_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let short_dir = tempfile::tempdir_in("/tmp").unwrap();
    let data_dir = desktop_like_data_dir(tmp.path());
    let default_socket = data_dir.join("runtime/pidash.sock");
    assert!(
        pidash_ipc::check_socket_path(&default_socket).is_err(),
        "the data dir must be deep enough to reproduce the bug: {default_socket:?}"
    );
    let socket = pidash_ipc::short_socket_path(short_dir.path(), &data_dir);
    let config_dir = data_dir.parent().unwrap().to_path_buf();
    let paths = Paths::resolve(Some(config_dir.clone()), Some(data_dir.clone())).unwrap();
    write_config(&paths, tmp.path());

    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_pidash"))
        .args(["__run", "--offline"])
        .env("PIDASH_CONFIG_DIR", &config_dir)
        .env("PIDASH_DATA_DIR", &data_dir)
        .env(pidash_ipc::SOCKET_ENV, &socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let reachable = socket_answers_within(&socket, Duration::from_secs(20)).await;
    let _ = daemon.kill();
    let _ = daemon.wait();
    assert!(
        reachable,
        "control socket {socket:?} never accepted a connection"
    );
}

/// What the desktop does: keep the data where it is and assign the daemon a
/// short socket path.
#[tokio::test]
async fn daemon_serves_the_socket_it_was_assigned() {
    let tmp = tempfile::tempdir().unwrap();
    let short_dir = tempfile::tempdir_in("/tmp").unwrap();
    let data_dir = desktop_like_data_dir(tmp.path());
    let socket = pidash_ipc::short_socket_path(short_dir.path(), &data_dir);
    let paths = Paths::resolve(
        Some(data_dir.parent().unwrap().to_path_buf()),
        Some(data_dir),
    )
    .unwrap()
    .with_ipc_socket(Some(socket.clone()));
    write_config(&paths, tmp.path());

    let daemon_paths = paths.clone();
    let daemon = tokio::spawn(async move {
        pidash::cli::run_for_tests(RunArgs { offline: true }, &daemon_paths).await
    });

    let reachable = socket_answers(&socket).await;
    assert!(
        reachable,
        "assigned socket {socket:?} never accepted a connection"
    );
    // Only the socket moved: the PID file is still under the data dir.
    assert!(paths.pid_path().starts_with(&paths.data_dir));

    // A swept socket file (macOS clears its temp dir of files left untouched
    // for days) comes back without restarting the daemon.
    std::fs::remove_file(&socket).unwrap();
    let rebound = socket_answers_within(&socket, Duration::from_secs(15)).await;
    daemon.abort();
    assert!(
        rebound,
        "socket {socket:?} was not bound again after being removed"
    );
}

/// A daemon that cannot bind the socket its host assigned exits with the
/// reason, rather than running on unreachable.
#[tokio::test]
async fn daemon_refuses_to_start_on_an_unbindable_assigned_socket() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = desktop_like_data_dir(tmp.path());
    let too_long = data_dir.join("runtime/pidash.sock");
    let paths = Paths::resolve(
        Some(data_dir.parent().unwrap().to_path_buf()),
        Some(data_dir),
    )
    .unwrap()
    .with_ipc_socket(Some(too_long));
    write_config(&paths, tmp.path());

    let result = tokio::time::timeout(
        Duration::from_secs(10),
        pidash::cli::run_for_tests(RunArgs { offline: true }, &paths),
    )
    .await
    .expect("the daemon exits instead of running without a control socket");
    let msg = format!("{:#}", result.expect_err("startup fails"));
    assert!(
        msg.contains("PIDASH_IPC_SOCKET") && msg.contains("limited to"),
        "error should name the cause: {msg}"
    );
}
