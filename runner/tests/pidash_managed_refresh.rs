use pidash::config::{file, schema::Config};
use pidash::util::paths::Paths;
use std::process::Command;

#[test]
fn reusing_enrollment_and_startup_rebind_refresh_all_managed_paths() {
    for verb in ["enroll", "rebind"] {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(
            Some(root.path().join("config")),
            Some(root.path().join("data")),
        )
        .unwrap();
        paths.ensure().unwrap();
        let mut runners = Vec::new();
        for project in ["FIRST", "SECOND", "MANUAL"] {
            let codex = if project == "MANUAL" {
                serde_json::json!({"binary": "personal-agent"})
            } else {
                serde_json::json!({"binary": "/old/mount/engine", "codex_home": "/old/home", "path_prepend": "/old/mount", "model_token_file": "/old/token"})
            };
            runners.push(serde_json::json!({
                "name": project, "runner_id": uuid::Uuid::new_v4(), "workspace_slug": "test", "project_slug": project,
                "workspace": {"working_dir": root.path().join(project)}, "codex": codex,
            }));
        }
        let cfg: Config = serde_json::from_value(serde_json::json!({
            "version": 2, "daemon": {"cloud_url": "http://127.0.0.1:1"},
            "cli": {"token": "mt_review_fake", "workspace_slug": "test"}, "runner": runners,
        }))
        .unwrap();
        file::write_config(&paths, &cfg).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_pidash"));
        command
            .env("PIDASH_CONFIG_DIR", &paths.config_dir)
            .env("PIDASH_DATA_DIR", &paths.data_dir)
            .args(["__managed", verb]);
        if verb == "enroll" {
            command
                .args(["--workspace", "test", "--project", "FIRST", "--working-dir"])
                .arg(root.path().join("FIRST"));
        }
        let output = command
            .arg("--engine")
            .arg(root.path().join("new mount/engine"))
            .arg("--codex-home")
            .arg(root.path().join("engine-home"))
            .arg("--path-prepend")
            .arg(root.path().join("new mount"))
            .arg("--model-token-file")
            .arg(root.path().join("model.token"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let updated = file::load_config(&paths).unwrap();
        for (before, after) in cfg.runners.iter().zip(&updated.runners) {
            assert_eq!(before.runner_id, after.runner_id);
            assert_eq!(before.workspace.working_dir, after.workspace.working_dir);
            if after.name == "MANUAL" {
                assert_eq!(after.codex.binary, "personal-agent");
                assert!(after.codex.codex_home.is_none());
            } else {
                assert_eq!(
                    after.codex.binary,
                    root.path().join("new mount/engine").to_str().unwrap()
                );
                assert_eq!(
                    after.codex.path_prepend.as_ref(),
                    Some(&root.path().join("new mount"))
                );
                assert_eq!(
                    after.codex.codex_home.as_ref(),
                    Some(&root.path().join("engine-home"))
                );
                assert_eq!(
                    after.codex.model_token_file.as_ref(),
                    Some(&root.path().join("model.token"))
                );
            }
        }
    }
}

#[test]
fn auth_helper_reads_rotations_and_refuses_missing_or_invalid_credentials() {
    let root = tempfile::tempdir().unwrap();
    let token = root.path().join("model.token");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_pidash"))
            .env("PIDASH_CONFIG_DIR", root.path().join("config"))
            .env("PIDASH_DATA_DIR", root.path().join("data"))
            .args(["__managed", "model-token", "--file"])
            .arg(&token)
            .output()
            .unwrap()
    };
    for value in ["old-credential", "new-credential"] {
        std::fs::write(&token, format!("{value}\n")).unwrap();
        let result = invoke();
        assert!(result.status.success());
        assert_eq!(String::from_utf8(result.stdout).unwrap().trim(), value);
        assert!(!String::from_utf8_lossy(&result.stderr).contains(value));
    }
    for value in ["", "bad credential", "bad\0credential"] {
        std::fs::write(&token, value).unwrap();
        let result = invoke();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    std::fs::remove_file(&token).unwrap();
    let result = invoke();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

#[test]
fn user_runner_on_same_project_is_left_untouched() {
    // A shared default config: the user's own runner serves FIRST. Enroll
    // must not adopt it as the managed runner (it tries to create a new one,
    // which fails here because there is no cloud), and remove / rebind must
    // leave it alone.
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::resolve(
        Some(root.path().join("config")),
        Some(root.path().join("data")),
    )
    .unwrap();
    paths.ensure().unwrap();
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "version": 2, "daemon": {"cloud_url": "http://127.0.0.1:1"},
        "cli": {"token": "mt_review_fake", "workspace_slug": "test"},
        "runner": [{
            "name": "mine", "runner_id": uuid::Uuid::new_v4(), "workspace_slug": "test",
            "project_slug": "FIRST", "workspace": {"working_dir": root.path().join("mine")},
            "agent": {"kind": "codex"}, "codex": {"binary": "my-own-codex"},
        }],
    }))
    .unwrap();
    file::write_config(&paths, &cfg).unwrap();
    let run = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pidash"));
        command
            .env("PIDASH_CONFIG_DIR", &paths.config_dir)
            .env("PIDASH_DATA_DIR", &paths.data_dir)
            .args(args);
        if args[1] != "remove" {
            command
                .arg("--engine")
                .arg(root.path().join("engine"))
                .arg("--codex-home")
                .arg(root.path().join("engine-home"))
                .arg("--path-prepend")
                .arg(root.path().join("bin"))
                .arg("--model-token-file")
                .arg(root.path().join("model.token"));
        }
        command.output().unwrap()
    };

    let enroll = run(&[
        "__managed",
        "enroll",
        "--workspace",
        "test",
        "--project",
        "FIRST",
        "--working-dir",
        root.path().join("desktop").to_str().unwrap(),
    ]);
    assert!(
        !enroll.status.success(),
        "enroll must create its own runner (no cloud here), not reuse the user's: {}",
        String::from_utf8_lossy(&enroll.stdout)
    );
    let rebind = run(&["__managed", "rebind"]);
    assert!(
        rebind.status.success(),
        "{}",
        String::from_utf8_lossy(&rebind.stderr)
    );
    let remove = run(&[
        "__managed",
        "remove",
        "--workspace",
        "test",
        "--project",
        "FIRST",
    ]);
    assert!(remove.status.success());
    assert_eq!(
        String::from_utf8_lossy(&remove.stdout).trim(),
        r#"{"ok":true,"removed":0}"#
    );

    // (Enroll may mint the local dev_machine_id; the runner set must not change.)
    let after = file::load_config(&paths).unwrap();
    assert_eq!(
        serde_json::to_value(&after.runners).unwrap(),
        serde_json::to_value(&cfg.runners).unwrap()
    );
}
