//! The OSS package seam configures a supplied executable without installing,
//! probing or authenticating it, and leaves ordinary runner behavior intact.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pidash::agent::package::AgentPackage;
use pidash::config::{
    file,
    schema::{AgentKind, Config, RunnerConfig},
};
use pidash::util::paths::Paths;

fn runner(root: &Path, name: &str) -> RunnerConfig {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "runner_id": uuid::Uuid::new_v4(),
        "workspace_slug": "test",
        "project_slug": "TASKS",
        "workspace": {"working_dir": root.join(name)},
    }))
    .unwrap()
}

fn setup(root: &Path) -> Paths {
    let paths = Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        runtime_dir: root.join("runtime"),
    };
    paths.ensure().unwrap();
    let config: Config = serde_json::from_value(serde_json::json!({
        "version": 2,
        "daemon": {"cloud_url": "http://127.0.0.1:1", "auto_update": false},
        "cli": {"token": "test-existing-credential", "workspace_slug": "test"},
        "runner": [runner(root, "custom"), runner(root, "untouched")],
    }))
    .unwrap();
    file::write_config(&paths, &config).unwrap();
    paths
}

fn manifest(root: &Path, text: &str) -> PathBuf {
    let path = root.join("agent-package.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn supplied_binary(root: &Path) -> PathBuf {
    let target = root.join("my engine.exe");
    // An existing executable fixture; no network, downloaded runtime, or
    // personal agent installation is involved in these tests.
    std::fs::copy(std::env::current_exe().unwrap(), &target).unwrap();
    target
}

fn use_package(paths: &Paths, name: &str, manifest: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pidash"))
        .env("PIDASH_CONFIG_DIR", &paths.config_dir)
        .env("PIDASH_DATA_DIR", &paths.data_dir)
        .args(["runner", "use-package", name, "--manifest"])
        .arg(manifest)
        .output()
        .unwrap()
}

#[test]
fn configures_every_supported_protocol_without_changing_other_runner_fields() {
    let root = tempfile::tempdir().unwrap();
    let executable = supplied_binary(root.path()).canonicalize().unwrap();
    for (protocol, kind) in [
        ("codex", AgentKind::Codex),
        ("claude_code", AgentKind::ClaudeCode),
        ("cursor_agent", AgentKind::CursorAgent),
        ("open_claw", AgentKind::OpenClaw),
        ("grok", AgentKind::Grok),
        ("muse_code", AgentKind::MuseCode),
    ] {
        let path = manifest(
            root.path(),
            &format!("version = 1\nprotocol = {protocol:?}\nexecutable = 'my engine.exe'\n"),
        );
        let package = AgentPackage::from_manifest(&path).unwrap();
        let mut actual = runner(root.path(), "custom");
        let mut expected = actual.clone();
        expected.agent.kind = kind;
        let binary = executable.to_str().unwrap().to_string();
        match kind {
            AgentKind::Codex => expected.codex.binary = binary,
            AgentKind::ClaudeCode => expected.claude_code.binary = binary,
            AgentKind::CursorAgent => expected.cursor_agent.binary = binary,
            AgentKind::OpenClaw => expected.openclaw.binary = binary,
            AgentKind::Grok => expected.grok.binary = binary,
            AgentKind::MuseCode => expected.muse_code.binary = binary,
        }
        package.apply_to_runner(&mut actual).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}

#[test]
fn cli_changes_only_the_selected_runner_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let paths = setup(root.path());
    let executable = supplied_binary(root.path());
    let path = manifest(
        root.path(),
        "version = 1\nprotocol = 'codex'\nexecutable = 'my engine.exe'\n",
    );
    let mut expected = file::load_config(&paths).unwrap();
    expected.runners[0].codex.binary = executable.canonicalize().unwrap().to_str().unwrap().into();
    for _ in 0..2 {
        let output = use_package(&paths, "custom", &path);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Restart the owning daemon"));
        let actual = file::load_config(&paths).unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
    assert!(
        !root.path().join("custom").exists(),
        "configuration must not create task files"
    );
}

#[test]
fn invalid_manifests_leave_config_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let paths = setup(root.path());
    supplied_binary(root.path());
    let before = std::fs::read(paths.config_path()).unwrap();
    for text in [
        "version = 2\nprotocol = 'codex'\nexecutable = 'my engine.exe'",
        "version = 1\nprotocol = 'unknown-protocol'\nexecutable = 'my engine.exe'",
        "version = 1\nprotocol = 'codex'\nexecutable = ''",
        "version = 1\nprotocol = 'codex'\nexecutable = '.'",
        "version = 1\nprotocol = 'codex'\nexecutable = 'missing'",
        "version = 1\nprotocol = 'codex'\nexecutable = 'https://example.invalid/engine'",
        "version = 1\nprotocol = 'codex'\nexecutable = 'my engine.exe'\ninstall = 'some command'",
        "version = 1\nprotocol = 'codex'\nexecutable = 'my engine.exe'\ntoken = 'not-supported'",
    ] {
        let path = manifest(root.path(), text);
        assert!(
            !use_package(&paths, "custom", &path).status.success(),
            "accepted {text}"
        );
        assert_eq!(std::fs::read(paths.config_path()).unwrap(), before);
    }
}

#[test]
fn unknown_runner_and_managed_host_are_not_modified() {
    let root = tempfile::tempdir().unwrap();
    let paths = setup(root.path());
    supplied_binary(root.path());
    let path = manifest(
        root.path(),
        "version = 1\nprotocol = 'codex'\nexecutable = 'my engine.exe'",
    );
    let before = std::fs::read(paths.config_path()).unwrap();
    assert!(!use_package(&paths, "missing", &path).status.success());
    assert_eq!(std::fs::read(paths.config_path()).unwrap(), before);
    file::mutate_config(&paths, |config| {
        let codex = &mut config.runners[0].codex;
        codex.codex_home = Some(root.path().join("managed-home"));
        codex.path_prepend = Some(root.path().to_path_buf());
        codex.model_token_file = Some(root.path().join("managed-token"));
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(paths.config_path()).unwrap();
    let output = use_package(&paths, "custom", &path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("owned by a managed host"));
    assert_eq!(std::fs::read(paths.config_path()).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn non_executable_package_is_refused_without_chmod() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("engine");
    std::fs::write(&binary, "not executable").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o600)).unwrap();
    let path = manifest(
        root.path(),
        "version = 1\nprotocol = 'codex'\nexecutable = 'engine'",
    );
    assert!(AgentPackage::from_manifest(&path).is_err());
    assert_eq!(
        binary.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[tokio::test]
async fn supplied_package_runs_through_the_normal_bridge_without_git() {
    use pidash::agent::{AgentBridge, BridgeEvent, RunPayload};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let paths = setup(root.path());
    let binary = root.path().join("my engine");
    std::fs::write(
        &binary,
        r#"#!/bin/sh
set -eu
test "$1" = app-server
printf 'started' > package-started
read -r init
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
read -r initialized
read -r thread
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"package-test"}}'
read -r turn
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"conclusion":"success"}}'
while read -r rest; do :; done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = manifest(
        root.path(),
        "version = 1\nprotocol = 'codex'\nexecutable = 'my engine'",
    );
    let output = use_package(&paths, "custom", &path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cfg = file::load_config(&paths).unwrap();
    let runner = &cfg.runners[0];
    let cwd = &runner.workspace.working_dir;
    assert!(!cwd.exists(), "configuration must not execute the package");
    std::fs::create_dir_all(cwd).unwrap();
    assert!(!cwd.join("package-started").exists());
    std::fs::write(cwd.join("notes.txt"), "keep this task input").unwrap();
    let mut bridge = AgentBridge::spawn_from_config(runner, cwd, None)
        .await
        .unwrap();
    let payload = RunPayload {
        run_id: uuid::Uuid::new_v4(),
        prompt: "ordinary task".into(),
        model: None,
    };
    let mut cursor = tokio::time::timeout(Duration::from_secs(5), bridge.run(&payload, cwd))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(events) = bridge.next_events(&mut cursor).await {
            if events
                .iter()
                .any(|event| matches!(event, BridgeEvent::Completed { .. }))
            {
                return;
            }
        }
        panic!("package did not complete the task");
    })
    .await
    .unwrap();
    bridge.shutdown(Duration::from_secs(1)).await.unwrap();
    assert!(cwd.join("package-started").exists());
    assert_eq!(
        std::fs::read_to_string(cwd.join("notes.txt")).unwrap(),
        "keep this task input"
    );
    assert!(!cwd.join(".git").exists());
}
