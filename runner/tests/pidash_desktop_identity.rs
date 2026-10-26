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
        "user_unknown",
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
    assert_eq!(v["user_unknown"], false);
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
    assert_eq!(
        v["user_unknown"], false,
        "signed out is not an unknown user"
    );
    let changed_at = v["changed_at"].as_str().unwrap();
    assert!(changed_at > "2026-10-01T10:00:00Z", "{changed_at}");
}

#[test]
fn auth_status_json_flags_a_signed_in_user_it_cannot_identify() {
    // Legacy config: a token but no recorded user, and the cloud can't be
    // asked (nothing listens on port 1). `user` is null as when signed out;
    // `user_unknown` tells the two apart.
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": uuid::Uuid::new_v4()},
        "cli": {"token": "mt_legacy", "workspace_slug": "acme"},
        "runner": [runner_json(root.path(), "legacy", uuid::Uuid::new_v4())],
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();

    let v = status_json(&paths);
    assert_shape(&v);
    assert_eq!(v["signed_in"], true);
    assert!(v["user"].is_null());
    assert_eq!(v["user_unknown"], true);

    // Signing out parks the unattributed runners instead of leaving them
    // for whoever signs in next.
    let output = pidash(&paths)
        .args(["auth", "logout", "--local-only", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(file::load_config(&paths).unwrap().runners.is_empty());
    let parked: Vec<String> = std::fs::read_dir(paths.parked_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(parked.len(), 1);
    assert!(parked[0].starts_with("unknown-"), "{parked:?}");
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
    assert_eq!(
        first,
        serde_json::json!({
            "adopted": [{"name": "desktop-proj", "workspace_id": "acme"}],
            "skipped": [{"name": "kept", "workspace_id": "acme", "reason": "already_present"}],
            "fully_adopted": true,
        })
    );

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
    assert_eq!(second["fully_adopted"], true);
    let reasons: Vec<_> = second["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("kept", "already_present"),
            ("desktop-proj", "already_present")
        ]
    );
}

fn write_legacy(dir: &std::path::Path, workspace: &str, runners: Vec<serde_json::Value>) {
    std::fs::create_dir_all(dir).unwrap();
    let legacy: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": uuid::Uuid::new_v4()},
        "cli": {"token": "mt_legacy", "workspace_slug": workspace},
        "runner": runners,
    }))
    .unwrap();
    std::fs::write(
        dir.join("config.toml"),
        toml::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();
}

fn adopt_json(paths: &Paths, dir: &std::path::Path) -> serde_json::Value {
    let output = pidash(paths)
        .args(["__managed", "adopt", "--from-config-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn managed_adopt_only_takes_the_signed_in_workspace() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "dev_machine_id": uuid::Uuid::new_v4()},
        "cli": {"token": "mt_own", "workspace_slug": "acme"},
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();

    // A legacy dir from another workspace: its runners would fail to
    // authenticate with acme's machine token.
    let mut other = runner_json(root.path(), "desktop-beta", uuid::Uuid::new_v4());
    other["workspace_slug"] = "beta".into();
    let legacy_dir = root.path().join("managed").join("beta");
    write_legacy(&legacy_dir, "beta", vec![other]);
    let before = std::fs::read_to_string(legacy_dir.join("config.toml")).unwrap();

    let v = adopt_json(&paths, &legacy_dir);
    assert_eq!(
        v,
        serde_json::json!({
            "adopted": [],
            "skipped": [{"name": "desktop-beta", "workspace_id": "beta", "reason": "workspace_mismatch"}],
            "fully_adopted": false,
        })
    );
    assert!(file::load_config(&paths).unwrap().runners.is_empty());
    assert_eq!(
        std::fs::read_to_string(legacy_dir.join("config.toml")).unwrap(),
        before,
        "the legacy config is left untouched"
    );

    // A mixed dir: the matching runner moves, the other stays behind.
    let mut mixed_other = runner_json(root.path(), "desktop-beta2", uuid::Uuid::new_v4());
    mixed_other["workspace_slug"] = "beta".into();
    let mixed_dir = root.path().join("managed").join("mixed");
    write_legacy(
        &mixed_dir,
        "acme",
        vec![
            runner_json(root.path(), "desktop-acme", uuid::Uuid::new_v4()),
            mixed_other,
        ],
    );
    let v = adopt_json(&paths, &mixed_dir);
    assert_eq!(
        v["adopted"],
        serde_json::json!([{"name": "desktop-acme", "workspace_id": "acme"}])
    );
    assert_eq!(v["skipped"][0]["reason"], "workspace_mismatch");
    assert_eq!(v["fully_adopted"], false);
}

#[test]
fn managed_adopt_takes_nothing_when_not_signed_in() {
    let root = tempfile::tempdir().unwrap();
    let paths = isolated_paths(root.path());
    let legacy_dir = root.path().join("managed").join("acme");
    write_legacy(
        &legacy_dir,
        "acme",
        vec![runner_json(
            root.path(),
            "desktop-acme",
            uuid::Uuid::new_v4(),
        )],
    );

    // No default config at all.
    let v = adopt_json(&paths, &legacy_dir);
    assert_eq!(
        v,
        serde_json::json!({
            "adopted": [],
            "skipped": [{"name": "desktop-acme", "workspace_id": "acme", "reason": "not_signed_in"}],
            "fully_adopted": false,
        })
    );
    assert!(!paths.config_path().exists(), "no config is created");

    // A signed-out default config.
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1"},
        "cli": {"workspace_slug": "acme"},
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();
    let v = adopt_json(&paths, &legacy_dir);
    assert_eq!(v["skipped"][0]["reason"], "not_signed_in");
    assert_eq!(v["fully_adopted"], false);
    assert!(file::load_config(&paths).unwrap().runners.is_empty());
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
