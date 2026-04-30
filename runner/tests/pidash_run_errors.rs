//! PR 2: verify `pidash __run` bails with an action-guiding error when the
//! machine hasn't been configured yet. This is the error users see when
//! systemd/launchd starts the service before `pidash configure` has run.
//!
//! We call the handler directly (not through a subprocess) so the test is
//! fast and platform-independent.

use pidash::cli::RunArgs;
use pidash::util::paths::Paths;
use tempfile::tempdir;

fn empty_paths(root: &std::path::Path) -> Paths {
    Paths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        runtime_dir: root.join("runtime"),
    }
}

fn ensure_dirs(paths: &Paths) {
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();
}

#[tokio::test]
async fn run_errors_when_config_missing() {
    let tmp = tempdir().unwrap();
    let paths = empty_paths(tmp.path());
    ensure_dirs(&paths);
    let args = RunArgs { offline: true };
    let err = pidash::cli::run_for_tests(args, &paths)
        .await
        .expect_err("__run with no config should fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("config.toml"),
        "error should mention config.toml: {msg}",
    );
    assert!(
        msg.contains("pidash connect") || msg.contains("pidash install"),
        "error should point at `pidash connect` or `pidash install`: {msg}",
    );
}

#[tokio::test]
async fn run_errors_when_creds_missing_but_config_present() {
    let tmp = tempdir().unwrap();
    let paths = empty_paths(tmp.path());
    ensure_dirs(&paths);
    // Config exists, credentials do not.
    std::fs::write(
        paths.config_path(),
        r#"
version = 1

[runner]
name = "t"
cloud_url = "https://x"

[workspace]
working_dir = "/tmp/wd"
"#,
    )
    .unwrap();
    let args = RunArgs { offline: true };
    let err = pidash::cli::run_for_tests(args, &paths)
        .await
        .expect_err("__run with config but no creds should fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("credentials.toml"),
        "error should mention credentials.toml: {msg}",
    );
    assert!(
        msg.contains("pidash connect"),
        "error should point at `pidash connect`: {msg}",
    );
}
