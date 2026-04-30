use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::schema::{Config, Credentials};
use crate::util::paths::Paths;

pub fn load_config(paths: &Paths) -> Result<Config> {
    let path = paths.config_path();
    let text = fs::read_to_string(&path).with_context(|| format!("reading config at {path:?}"))?;
    let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {path:?}"))?;
    Ok(cfg)
}

pub fn load_config_opt(paths: &Paths) -> Result<Option<Config>> {
    match load_config(paths) {
        Ok(c) => Ok(Some(c)),
        Err(e) => {
            if matches!(
                e.downcast_ref::<std::io::Error>().map(|e| e.kind()),
                Some(std::io::ErrorKind::NotFound)
            ) {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

pub fn load_credentials(paths: &Paths) -> Result<Credentials> {
    let path = paths.credentials_path();
    let text =
        fs::read_to_string(&path).with_context(|| format!("reading credentials at {path:?}"))?;
    let creds: Credentials = toml::from_str(&text).with_context(|| format!("parsing {path:?}"))?;
    Ok(creds)
}

pub fn load_all(paths: &Paths) -> Result<(Config, Credentials)> {
    Ok((load_config(paths)?, load_credentials(paths)?))
}

pub fn write_config(paths: &Paths, config: &Config) -> Result<()> {
    paths.ensure()?;
    let s = toml::to_string_pretty(config)?;
    write_private(&paths.config_path(), s.as_bytes())
}

pub fn write_credentials(paths: &Paths, creds: &Credentials) -> Result<()> {
    paths.ensure()?;
    let s = toml::to_string_pretty(creds)?;
    write_private(&paths.credentials_path(), s.as_bytes())
}

pub fn remove_all(paths: &Paths) -> Result<()> {
    for p in [paths.config_path(), paths.credentials_path()] {
        if p.exists() {
            fs::remove_file(&p).with_context(|| format!("removing {p:?}"))?;
        }
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {parent:?}"))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        use std::io::Write;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // `rename` preserves the destination's mode on some filesystems; enforce 0600 again.
    let mut perm = fs::metadata(path)?.permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o600);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Credentials, DaemonConfig, RunnerConfig, WorkspaceSection};
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn sample_runner(name: &str, working_dir: std::path::PathBuf) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: uuid::Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("TEST".into()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    #[test]
    fn writes_and_reads_config_with_0600() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
            },
            runners: vec![sample_runner("t", tmp.path().join("wd"))],
        };
        write_config(&paths, &cfg).unwrap();
        let loaded = load_config(&paths).unwrap();
        let primary = loaded.primary_runner().expect("runner present");
        assert_eq!(primary.name, "t");
        assert_eq!(primary.workspace_slug.as_deref(), Some("acme"));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths.config_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn config_without_workspace_slug_round_trips_via_serde_default() {
        // A config.toml written without a `workspace_slug = ...` line under
        // [[runner]] must still parse, with the field defaulting to None.
        // The CRUD subcommands will detect the None and error with a clear
        // "rerun pidash configure" message.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let body = format!(
            r#"
version = 2

[daemon]
cloud_url = "https://x"

[[runner]]
name = "t"
runner_id = "{}"

[runner.workspace]
working_dir = "/tmp/wd"

[runner.codex]
binary = "codex"
"#,
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), body).unwrap();
        let loaded = load_config(&paths).unwrap();
        let runner = loaded.primary_runner().expect("runner");
        assert_eq!(runner.workspace_slug, None);
        // model_default must stay absent unless the user opts in; the
        // runner is model-agnostic by design.
        assert_eq!(runner.codex.model_default, None);
    }

    #[test]
    fn writes_and_reads_credentials() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let creds = Credentials {
            connection_id: uuid::Uuid::new_v4(),
            connection_secret: "apd_cs_test".into(),
            connection_name: Some("connection_001".into()),
            api_token: Some("pi_dash_api_test".into()),
            issued_at: chrono::Utc::now(),
        };
        write_credentials(&paths, &creds).unwrap();
        let loaded = load_credentials(&paths).unwrap();
        assert_eq!(loaded.connection_secret, "apd_cs_test");
        assert_eq!(loaded.connection_name.as_deref(), Some("connection_001"));
        assert_eq!(loaded.api_token.as_deref(), Some("pi_dash_api_test"));
    }

    #[test]
    fn minimal_config_without_optional_sections_parses_with_defaults() {
        // A config.toml missing [runner.approval_policy] and the daemon-level
        // log fields must still parse — sections with `#[serde(default)]`
        // fall back to their Default impls.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let body = format!(
            r#"
version = 2

[daemon]
cloud_url = "https://x"

[[runner]]
name = "t"
runner_id = "{}"

[runner.workspace]
working_dir = "/tmp/wd"

[runner.codex]
binary = "codex"
"#,
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), body).unwrap();
        let loaded = load_config(&paths).unwrap();
        let primary = loaded.primary_runner().expect("runner");
        assert_eq!(primary.name, "t");
        // Defaults applied to the omitted fields:
        assert!(!primary.approval_policy.auto_approve_network);
        assert_eq!(loaded.daemon.log_level, "info");
        assert_eq!(loaded.daemon.log_retention_days, 14);
        assert_eq!(primary.codex.model_default, None);
    }

    #[test]
    fn config_without_required_field_fails_loudly() {
        // `daemon.cloud_url` is required. A config without it must fail to
        // parse so `__run` never boots a half-configured daemon.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let body = format!(
            r#"
version = 2

[daemon]

[[runner]]
name = "t"
runner_id = "{}"

[runner.workspace]
working_dir = "/tmp/wd"

[runner.codex]
binary = "codex"
"#,
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), body).unwrap();
        let err = load_config(&paths).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("cloud_url") || msg.contains("daemon"),
            "error should mention the missing field: {msg}",
        );
    }

    #[test]
    fn credentials_without_optional_fields_round_trip_via_serde_default() {
        // A minimal credentials.toml — connection_id + connection_secret +
        // issued_at — must still parse with optional fields defaulting.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let body = format!(
            "connection_id = \"{}\"\nconnection_secret = \"apd_cs_x\"\nissued_at = \"2026-04-01T00:00:00Z\"\n",
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.credentials_path(), body).unwrap();
        let loaded = load_credentials(&paths).unwrap();
        assert!(loaded.api_token.is_none());
        assert!(loaded.connection_name.is_none());
    }
}
