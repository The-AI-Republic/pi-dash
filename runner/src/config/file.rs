use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use nix::fcntl::{Flock, FlockArg};

use super::schema::{Config, Credentials};
use crate::util::paths::Paths;

pub fn load_config(paths: &Paths) -> Result<Config> {
    let path = paths.config_path();
    let text = fs::read_to_string(&path).with_context(|| format!("reading config at {path:?}"))?;
    let mut cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {path:?}"))?;
    // Migrate the prior hardcoded default. `gpt-5-codex` was written into
    // every config produced by older runner builds; on ChatGPT-account auth
    // the codex app-server 400s on it. Treat the literal string as a
    // poisoned default and let codex's own config pick the model.
    for runner in &mut cfg.runners {
        if runner.codex.model_default.as_deref() == Some("gpt-5-codex") {
            runner.codex.model_default = None;
        }
    }
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

/// Path to the host-wide `config.toml` mutation lock. Held by
/// [`mutate_config`] while it does its read-modify-write.
fn config_lock_path(paths: &Paths) -> std::path::PathBuf {
    paths.config_dir.join(".config.lock")
}

/// Read-modify-write `config.toml` under an exclusive `flock` on
/// `<config_dir>/.config.lock`.
///
/// All `config.toml` mutators (CLI subcommands plus the daemon's
/// per-runner cascade-uninstall) must go through this helper — without
/// it, two concurrent writers will lose changes via last-writer-wins.
/// The lock is host-wide (not per-runner) so a `pidash runner add` and
/// the daemon's RemoveRunner handler can't race each other.
///
/// The closure receives `&mut Config` already loaded from disk and
/// migrated; on `Ok(())` we run `Config::validate` and, if that
/// passes, serialize + atomically rewrite the file. On any error the
/// on-disk file is untouched. Returns the post-mutation `Config` so
/// callers can introspect (e.g. "was that the last runner?").
///
/// Validation runs inside the lock so a closure can't write a config
/// the loader would later reject; today's callers (retain/remove and
/// the new add path) all produce valid configs, but defending the
/// invariant here keeps future callers honest.
pub fn mutate_config<F>(paths: &Paths, mutate: F) -> Result<Config>
where
    F: FnOnce(&mut Config) -> Result<()>,
{
    paths.ensure()?;
    let lock_path = config_lock_path(paths);
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        // The lock file is opened only to hand its fd to `flock`; we
        // never read or write its contents, so explicitly opt out of
        // truncation. Without this, a concurrent `mutate_config` could
        // see a 0-byte stat between the open and the flock acquisition.
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .with_context(|| format!("opening config lock {lock_path:?}"))?;
    let _guard = Flock::lock(lock_file, FlockArg::LockExclusive)
        .map_err(|(_, errno)| anyhow::anyhow!("flock({lock_path:?}) failed: {errno}"))?;
    let mut cfg = load_config(paths)?;
    mutate(&mut cfg)?;
    cfg.validate()
        .with_context(|| "config invalid after mutate_config closure")?;
    write_config(paths, &cfg)?;
    Ok(cfg)
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
                agent_observability_v1: false,
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
    fn legacy_gpt_5_codex_default_is_migrated_to_none() {
        // Older runner builds wrote `model_default = "gpt-5-codex"` into
        // every config. That value 400s on ChatGPT-account auth, so on load
        // we coerce it to None and let codex pick its own default.
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
model_default = "gpt-5-codex"
"#,
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), body).unwrap();
        let loaded = load_config(&paths).unwrap();
        let primary = loaded.primary_runner().expect("runner");
        assert_eq!(primary.codex.model_default, None);
    }

    #[test]
    fn other_model_defaults_are_preserved() {
        // Migration must only neutralise the poisoned default; an explicitly
        // chosen model still passes through.
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
model_default = "o4-mini"
"#,
            uuid::Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), body).unwrap();
        let loaded = load_config(&paths).unwrap();
        let primary = loaded.primary_runner().expect("runner");
        assert_eq!(primary.codex.model_default.as_deref(), Some("o4-mini"));
    }

    #[test]
    fn mutate_config_round_trips_under_lock() {
        // mutate_config takes the exclusive flock on .config.lock, applies
        // the closure to the loaded config, and writes it back atomically.
        // The lock guard drops at function end, so a second call works.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners: vec![
                sample_runner("a", tmp.path().join("wd-a")),
                sample_runner("b", tmp.path().join("wd-b")),
            ],
        };
        write_config(&paths, &cfg).unwrap();

        let after = mutate_config(&paths, |c| {
            c.runners.retain(|r| r.name != "a");
            Ok(())
        })
        .unwrap();
        assert_eq!(after.runners.len(), 1);
        assert_eq!(after.runners[0].name, "b");

        // Lock guard released; a second mutation succeeds.
        let after2 = mutate_config(&paths, |c| {
            c.runners.clear();
            Ok(())
        })
        .unwrap();
        assert!(after2.runners.is_empty());
        let reloaded = load_config(&paths).unwrap();
        assert!(reloaded.runners.is_empty());
    }

    #[test]
    fn mutate_config_strips_one_runner_leaves_others() {
        // The daemon's `ServerMsg::RemoveRunner` handler and the CLI's
        // `pidash runner remove` both call `mutate_config` with a
        // closure that retains every block whose `runner_id` does NOT
        // match the target. Verify the on-disk file ends up with
        // exactly the other runners, and the systemd unit name (the
        // shared `pidash.service`) is not expressed in this layer at
        // all — config.toml is per-runner, the unit isn't touched.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners: vec![
                sample_runner("a", tmp.path().join("wd-a")),
                sample_runner("b", tmp.path().join("wd-b")),
                sample_runner("c", tmp.path().join("wd-c")),
            ],
        };
        write_config(&paths, &cfg).unwrap();
        let target = cfg.runners[1].runner_id;

        let after = mutate_config(&paths, |c| {
            c.runners.retain(|r| r.runner_id != target);
            Ok(())
        })
        .unwrap();
        assert_eq!(after.runners.len(), 2);
        assert!(after.runners.iter().all(|r| r.runner_id != target));
        let names: Vec<_> = after.runners.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c"]);
    }

    #[test]
    fn mutate_config_error_preserves_on_disk_state() {
        // If the closure returns Err, the on-disk file must not change.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners: vec![sample_runner("keep", tmp.path().join("wd"))],
        };
        write_config(&paths, &cfg).unwrap();

        let err = mutate_config(&paths, |c| {
            c.runners.clear();
            anyhow::bail!("simulated failure");
        })
        .unwrap_err();
        assert!(format!("{err:#}").contains("simulated failure"));
        let reloaded = load_config(&paths).unwrap();
        assert_eq!(reloaded.runners.len(), 1);
        assert_eq!(reloaded.runners[0].name, "keep");
    }

    #[test]
    fn mutate_config_serializes_concurrent_writers_no_lost_update() {
        // Two threads racing through `mutate_config` must both land
        // their changes — no last-writer-wins. The flock guarantees
        // serialization; this test exercises that the lock actually
        // holds across the read-modify-write window so the second
        // writer sees the first writer's committed state, not the
        // pre-race load.
        use std::sync::Arc;
        use std::sync::Barrier;

        let tmp = tempdir().unwrap();
        let paths = Arc::new(paths_for(tmp.path()));
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners: vec![
                sample_runner("a", tmp.path().join("wd-a")),
                sample_runner("b", tmp.path().join("wd-b")),
            ],
        };
        write_config(&paths, &cfg).unwrap();

        // Both threads start at the same instant via the barrier so
        // there's a real race for the flock. Each thread retains every
        // runner whose name is NOT its target — so a lost-update bug
        // (second mutate loaded the pre-race config) would leave one
        // of the targets still on disk.
        let barrier = Arc::new(Barrier::new(2));
        let p1 = Arc::clone(&paths);
        let b1 = Arc::clone(&barrier);
        let t1 = std::thread::spawn(move || {
            b1.wait();
            mutate_config(&p1, |c| {
                c.runners.retain(|r| r.name != "a");
                Ok(())
            })
            .unwrap();
        });
        let p2 = Arc::clone(&paths);
        let b2 = Arc::clone(&barrier);
        let t2 = std::thread::spawn(move || {
            b2.wait();
            mutate_config(&p2, |c| {
                c.runners.retain(|r| r.name != "b");
                Ok(())
            })
            .unwrap();
        });
        t1.join().unwrap();
        t2.join().unwrap();

        // Both writes landed: neither "a" nor "b" remain.
        let reloaded = load_config(&paths).unwrap();
        assert!(
            reloaded.runners.is_empty(),
            "both writers should have landed; got {:?}",
            reloaded
                .runners
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
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
