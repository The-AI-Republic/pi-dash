// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CLI contract the desktop app relies on: `auth status --json` and
//! `__managed adopt` (JSON on stdout, stable shapes).

use pidash::config::{file, schema::Config};
use pidash::util::paths::Paths;
use std::process::Command;

fn isolated_paths(root: &std::path::Path) -> Paths {
    let paths = Paths::resolve(Some(root.join("config")), Some(root.join("data"))).unwrap();
    paths.ensure().unwrap();
    paths
}

fn pidash(paths: &Paths) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pidash"));
    command
        .env("PIDASH_CONFIG_DIR", &paths.config_dir)
        .env("PIDASH_DATA_DIR", &paths.data_dir);
    command
}

fn status_json(paths: &Paths) -> serde_json::Value {
    let output = pidash(paths)
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("auth status --json prints JSON")
}

fn assert_shape(v: &serde_json::Value) {
    for key in [
        "signed_in",
        "cloud_url",
        "user",
        "workspace_slug",
        "dev_machine_id",
        "changed_at",
        "daemon_running",
        "active_runs",
    ] {
        assert!(v.get(key).is_some(), "missing {key} in {v}");
    }
    assert!(v["daemon_running"].is_boolean());
    assert!(v["active_runs"].is_u64());
}

fn runner_json(root: &std::path::Path, name: &str, id: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "name": name, "runner_id": id, "workspace_slug": "acme", "project_slug": name.to_uppercase(),
        "workspace": {"working_dir": root.join(name)},
        "codex": {"binary": "/bundle/engine", "codex_home": "/bundle/home", "path_prepend": "/bundle", "model_token_file": "/bundle/token"},
    })
}

#[test]
fn auth_status_json_without_config() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let v = status_json(&paths);
    assert_shape(&v);
    assert_eq!(v["signed_in"], false);
    assert!(v["cloud_url"].is_null());
    assert!(v["user"].is_null());
    assert!(v["changed_at"].is_null());
}

#[test]
fn auth_status_json_signed_in_and_signed_out() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let machine = uuid::Uuid::new_v4();
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": machine},
        "cli": {
            "token": "mt_fake", "workspace_slug": "acme",
            "user_id": "1e6a7a56-0000-0000-0000-000000000001", "user_email": "a@example.com",
            "signed_in_at": "2026-10-01T10:00:00Z", "signed_out_at": "2026-09-01T10:00:00Z",
        },
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();

    let v = status_json(&paths);
    assert_shape(&v);
    assert_eq!(v["signed_in"], true);
    assert_eq!(v["cloud_url"], "http://127.0.0.1:1");
    assert_eq!(v["user"]["id"], "1e6a7a56-0000-0000-0000-000000000001");
    assert_eq!(v["user"]["email"], "a@example.com");
    assert_eq!(v["workspace_slug"], "acme");
    assert_eq!(v["dev_machine_id"], machine.to_string());
    assert_eq!(v["changed_at"], "2026-10-01T10:00:00Z");

    // Local-only sign-out: no cloud to talk to.
    let output = pidash(&paths)
        .args(["auth", "logout", "--local-only", "--yes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v = status_json(&paths);
    assert_eq!(v["signed_in"], false);
    assert!(v["user"].is_null());
    let changed_at = v["changed_at"].as_str().unwrap();
    assert!(changed_at > "2026-10-01T10:00:00Z", "{changed_at}");
}

#[test]
fn managed_adopt_merges_runners_once_without_identity() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let own_machine = uuid::Uuid::new_v4();
    let existing = uuid::Uuid::new_v4();
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": own_machine},
        "cli": {"token": "mt_own", "workspace_slug": "acme"},
        "runner": [runner_json(root.path(), "kept", existing)],
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();

    let legacy_dir = root.path().join("managed").join("acme");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let new_id = uuid::Uuid::new_v4();
    let legacy: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": uuid::Uuid::new_v4()},
        "cli": {"token": "mt_legacy", "workspace_slug": "acme"},
        "runner": [
            runner_json(root.path(), "kept", existing),
            runner_json(root.path(), "desktop-proj", new_id),
        ],
    }))
    .unwrap();
    std::fs::write(
        legacy_dir.join("config.toml"),
        toml::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let adopt = || {
        let output = pidash(&paths)
            .args(["__managed", "adopt", "--from-config-dir"])
            .arg(&legacy_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let first = adopt();
    assert_eq!(first["adopted"], serde_json::json!(["desktop-proj"]));
    assert_eq!(first["skipped"], serde_json::json!(["kept"]));

    let merged = file::load_config(&paths).unwrap();
    assert_eq!(merged.runners.len(), 2);
    assert_eq!(merged.daemon.dev_machine_id, Some(own_machine));
    assert_eq!(
        merged.cli.as_ref().unwrap().token.as_deref(),
        Some("mt_own")
    );
    let adopted = merged
        .runners
        .iter()
        .find(|r| r.runner_id == new_id)
        .unwrap();
    assert!(
        adopted.codex.codex_home.is_some(),
        "managed fields carried over"
    );

    let second = adopt();
    assert_eq!(second["adopted"], serde_json::json!([]));
    assert_eq!(
        second["skipped"],
        serde_json::json!(["kept", "desktop-proj"])
    );
}

#[test]
fn managed_adopt_reports_json_error() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let output = pidash(&paths)
        .args(["__managed", "adopt", "--from-config-dir"])
        .arg(root.path().join("missing"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["error"], "adopt_failed");
}

#[test]
fn desktop_handoff_signed_out_reports_not_signed_in() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let output = pidash(&paths)
        .args(["__desktop", "handoff"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["error"], "not_signed_in");
}
