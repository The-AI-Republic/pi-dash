// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A one-shot `pidash` command whose stdout reader goes away early
//! (`pidash … | head -c 0`) must exit quietly instead of panicking with
//! "failed printing to stdout: Broken pipe".
//!
//! The test drives the real binary against a loopback HTTP fake. The
//! child's stdout is a pipe whose read end is closed before the child is
//! spawned, so its first write hits EPIPE exactly as it does behind `head`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

/// Serve every request on `listener` with `body` as JSON, until the
/// listener is dropped with the test process.
fn serve_json(listener: TcpListener, body: String) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            // Drain the request head; GET requests carry no body.
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
}

/// Run `pidash <args>` with a stdout whose read end is already closed,
/// and return its exit status and stderr. `PIDASH_RUN_ID` is cleared
/// unless `envs` sets it.
fn run_with_closed_stdout(
    config_dir: &std::path::Path,
    envs: &[(&str, String)],
    args: &[&str],
) -> (std::process::ExitStatus, String) {
    // Close the read end before spawning, so the child can never write
    // into a still-open pipe however the scheduler orders the two.
    let (reader, writer) = std::io::pipe().expect("create stdout pipe");
    drop(reader);

    let mut command = Command::new(env!("CARGO_BIN_EXE_pidash"));
    command
        .env("PIDASH_CONFIG_DIR", config_dir)
        .env("PIDASH_DATA_DIR", config_dir.join("data"))
        .env_remove("PIDASH_RUN_ID")
        .env_remove("PIDASH_ALLOW_OPERATOR")
        .env_remove("PIDASH_ISSUE_IDENTIFIER")
        .env_remove("RUST_BACKTRACE")
        .envs(envs.iter().map(|(k, v)| (*k, v.as_str())))
        .args(args)
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn pidash");
    // Drop the parent's copy of the write end with the builder.
    drop(command);

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    (child.wait().unwrap(), stderr)
}

/// No panic or error text on stderr, and exit status 141 (128 + SIGPIPE).
fn assert_quiet_broken_pipe_exit(status: std::process::ExitStatus, stderr: &str) {
    assert!(
        !stderr.contains("panicked") && !stderr.contains("Broken pipe"),
        "expected no panic text on stderr, got: {stderr}"
    );
    assert!(stderr.is_empty(), "expected empty stderr, got: {stderr}");
    assert_eq!(
        status.code(),
        Some(141),
        "expected exit 141, got {status:?} (stderr: {stderr})"
    );
}

/// Serve one JSON body for every request and return the env that points
/// `pidash` at it.
fn fake_api(body: serde_json::Value) -> Vec<(&'static str, String)> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    serve_json(listener, body.to_string());
    vec![
        ("PIDASH_API_URL", format!("http://{addr}")),
        ("PIDASH_WORKSPACE_SLUG", "acme".to_string()),
        ("PIDASH_TOKEN", "test-token".to_string()),
    ]
}

#[test]
fn workspace_me_with_closed_stdout_exits_quietly() {
    let envs = fake_api(serde_json::json!({ "id": "u-1", "padding": "x".repeat(256 * 1024) }));
    let config_dir = tempfile::tempdir().unwrap();
    let (status, stderr) = run_with_closed_stdout(config_dir.path(), &envs, &["workspace", "me"]);
    assert_quiet_broken_pipe_exit(status, &stderr);
}

/// A second `println!` command, one that resolves the issue first: the
/// fake answers both requests with a body that serves as the issue and
/// as the comment list.
#[test]
fn comment_list_with_closed_stdout_exits_quietly() {
    let envs = fake_api(serde_json::json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "project": "22222222-2222-2222-2222-222222222222",
        "results": [],
    }));
    let config_dir = tempfile::tempdir().unwrap();
    let (status, stderr) =
        run_with_closed_stdout(config_dir.path(), &envs, &["comment", "list", "ENG-1"]);
    assert_quiet_broken_pipe_exit(status, &stderr);
}

/// Inside an agent run, `pidash --help`, `-h` and `help` print the
/// filtered help from `main`, before `cli::run` installs the hook.
#[test]
fn agent_run_top_level_help_with_closed_stdout_exits_quietly() {
    let config_dir = tempfile::tempdir().unwrap();
    for arg in ["--help", "-h", "help"] {
        let (status, stderr) = run_with_closed_stdout(
            config_dir.path(),
            &[("PIDASH_RUN_ID", "run-1".to_string())],
            &[arg],
        );
        assert_quiet_broken_pipe_exit(status, &stderr);
    }
}

/// Bare `pidash` on a configured host prints clap's help through a
/// fallible writer, not `println!`, so the panic hook alone does not
/// cover it.
#[test]
fn bare_pidash_help_with_closed_stdout_exits_quietly() {
    let config_dir = tempfile::tempdir().unwrap();
    std::fs::write(config_dir.path().join("config.toml"), "").unwrap();
    let (status, stderr) = run_with_closed_stdout(config_dir.path(), &[], &[]);
    assert_quiet_broken_pipe_exit(status, &stderr);
}
