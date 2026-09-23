use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn bootstrap_preserves_server_machine_identity_and_workspace() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let data = root.path().join("data");
    let machine_id = uuid::Uuid::new_v4();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pidash"))
        .env("PIDASH_CONFIG_DIR", &config)
        .env("PIDASH_DATA_DIR", &data)
        .args([
            "__managed",
            "bootstrap",
            "--cloud-url",
            "http://localhost:18002",
            "--workspace",
            "desktop-e2e",
            "--dev-machine-id",
            &machine_id.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"mt-local-test-token\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let paths = pidash::util::paths::Paths::resolve(Some(config), Some(data)).unwrap();
    let cfg = pidash::config::file::load_config(&paths).unwrap();
    assert_eq!(cfg.daemon.dev_machine_id, Some(machine_id));
    let cli = cfg.cli.unwrap();
    assert_eq!(cli.workspace_slug.as_deref(), Some("desktop-e2e"));
    assert_eq!(cli.token.as_deref(), Some("mt-local-test-token"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("mt-local-test-token"));
}
