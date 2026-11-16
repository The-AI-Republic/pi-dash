//! PIDESKAPP-122: one daemon for several workspaces, runner changes
//! without a restart, and the `Hello` IPC — end to end against a real
//! `pidash __run --offline` daemon.

use std::process::Stdio;
use std::time::Duration;

use pidash::config::{
    file,
    schema::{Config, RunnerConfig},
};
use pidash::ipc::{
    client::Client,
    protocol::{EngineState, IPC_VERSION, Request, Response},
};
use pidash::util::paths::Paths;

fn runner(root: &std::path::Path, name: &str, workspace: &str) -> RunnerConfig {
    serde_json::from_value(serde_json::json!({
        "name": name, "runner_id": uuid::Uuid::new_v4(),
        "project_slug": name.to_uppercase(), "workspace_slug": workspace,
        "workspace": {"working_dir": root.join(format!("wd-{name}"))},
    }))
    .unwrap()
}

async fn pidash(paths: &Paths, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_pidash"))
        .env("PIDASH_CONFIG_DIR", &paths.config_dir)
        .env("PIDASH_DATA_DIR", &paths.data_dir)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap()
}

async fn runner_names(ipc: &mut Client) -> Vec<String> {
    let Response::Status(status) = ipc.call(Request::StatusGet).await.unwrap() else {
        panic!("status expected")
    };
    let mut names: Vec<String> = status.runners.into_iter().map(|r| r.name).collect();
    names.sort();
    names
}

#[tokio::test]
async fn one_daemon_serves_two_workspaces_and_changes_runners_without_restarting() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::resolve(
        Some(root.path().join("config")),
        Some(root.path().join("data")),
    )
    .unwrap();
    paths.ensure().unwrap();
    let alpha = runner(root.path(), "alpha", "acme");
    let beta = runner(root.path(), "beta", "beta");
    let machine_id = uuid::Uuid::new_v4();
    let config: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "auto_update": false,
                   "dev_machine_id": machine_id},
        "cli": {"token": "mt_acme", "workspace_slug": "acme",
                "account_email": "dev@example.com",
                "workspaces": [{"slug": "beta", "token": "mt_beta"}]},
        "runner": [alpha, beta],
    }))
    .unwrap();
    file::write_config(&paths, &config).unwrap();

    let mut daemon = tokio::process::Command::new(env!("CARGO_BIN_EXE_pidash"))
        .env("PIDASH_CONFIG_DIR", &paths.config_dir)
        .env("PIDASH_DATA_DIR", &paths.data_dir)
        .args(["__run", "--offline"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = daemon.id();
    let mut ipc = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(client) = Client::connect(paths.ipc_socket_path()).await {
                break client;
            }
            assert!(
                daemon.try_wait().unwrap().is_none(),
                "daemon exited before IPC became ready"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    // Hello (design §4.4).
    let Response::Hello(hello) = ipc.call(Request::Hello).await.unwrap() else {
        panic!("hello expected")
    };
    assert_eq!(hello.ipc_version, IPC_VERSION);
    assert_eq!(hello.daemon_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hello.machine_id, Some(machine_id));
    assert_eq!(hello.engine, EngineState::Unavailable);
    assert_eq!(
        hello.account.as_ref().and_then(|a| a.email.as_deref()),
        Some("dev@example.com")
    );
    let workspaces: Vec<(String, bool, usize)> = hello
        .workspaces
        .iter()
        .map(|w| (w.slug.clone(), w.signed_in, w.runners))
        .collect();
    assert_eq!(
        workspaces,
        vec![("acme".into(), true, 1), ("beta".into(), true, 1)]
    );
    assert_eq!(runner_names(&mut ipc).await, vec!["alpha", "beta"]);

    // Per-runner reconfigure: a new working dir, no daemon restart.
    let moved = root.path().join("moved-alpha");
    let out = pidash(
        &paths,
        &[
            "runner",
            "set-working-dir",
            "alpha",
            moved.to_str().unwrap(),
        ],
    )
    .await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(daemon.try_wait().unwrap().is_none());
    assert_eq!(daemon.id(), pid, "same daemon process");
    let on_disk = file::load_config(&paths).unwrap();
    let alpha_on_disk = on_disk.runners.iter().find(|r| r.name == "alpha").unwrap();
    assert_eq!(alpha_on_disk.workspace.working_dir, moved);
    assert!(moved.is_dir());
    assert_eq!(runner_names(&mut ipc).await, vec!["alpha", "beta"]);

    // Removal cleans the registry; re-adding the same id brings it back.
    let out = pidash(
        &paths,
        &["runner", "remove", "beta", "--local-only", "--yes"],
    )
    .await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(runner_names(&mut ipc).await, vec!["alpha"]);
    file::mutate_config(&paths, |cfg| {
        cfg.runners.push(beta.clone());
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        ipc.call(Request::RunnerActivateLocal {
            runner: "beta".into()
        })
        .await
        .unwrap(),
        Response::Ack
    ));
    assert_eq!(runner_names(&mut ipc).await, vec!["alpha", "beta"]);
    assert_eq!(daemon.id(), pid, "still the same daemon process");

    daemon.kill().await.unwrap();
    daemon.wait().await.unwrap();
}
