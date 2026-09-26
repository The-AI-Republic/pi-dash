use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const QUALIFIER: &str = "so";
const ORG: &str = "pidash";
const APP: &str = "pidash";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

/// Per-instance filesystem paths under `data_dir/runners/<runner_id>/`. Each
/// runner instance owns its own history, logs, and identity file; this newtype
/// keeps a runner_id baked in so call sites can't accidentally write to
/// another runner's tree.
#[derive(Debug, Clone)]
pub struct RunnerPaths {
    pub runner_id: Uuid,
    base_dir: PathBuf,
}

/// Compare two paths for identity, resolving symlinks where possible.
/// `canonicalize` fails on paths that don't exist yet (a fresh install's
/// data dir, most test paths), so fall back to the raw path — `Path`
/// equality still normalises `.` components and repeated separators.
fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Decide whether a data-dir override marks this daemon as an isolated
/// install (own PID file + IPC socket under `<data_dir>/runtime/`).
/// Only an override that names a *different* directory than the platform
/// default counts: service units used to export the default as
/// `PIDASH_DATA_DIR`, which moved the daemon's socket away from where an
/// override-less CLI resolves it (PDASHOSS01-230).
fn is_isolated(data_override: Option<&Path>, default_data_dir: &Path) -> bool {
    data_override.is_some_and(|dir| !same_path(dir, default_data_dir))
}

impl Paths {
    pub fn resolve(
        config_override: Option<PathBuf>,
        data_override: Option<PathBuf>,
    ) -> Result<Self> {
        let dirs = ProjectDirs::from(QUALIFIER, ORG, APP)
            .context("unable to resolve XDG project directories")?;
        let config_dir = config_override.unwrap_or_else(|| dirs.config_dir().to_path_buf());
        // An isolated daemon must also get an isolated PID and IPC socket.
        // Otherwise the desktop and a personal installation claim the same
        // XDG runtime path even though their configuration and data differ.
        // See `is_isolated` for why an override naming the default data dir
        // does NOT count as isolated.
        let default_data_dir = dirs.data_dir().to_path_buf();
        let isolated = is_isolated(data_override.as_deref(), &default_data_dir);
        let data_dir = data_override.unwrap_or(default_data_dir);
        let runtime_dir = if isolated {
            data_dir.join("runtime")
        } else {
            dirs.runtime_dir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| data_dir.join("runtime"))
        };
        Ok(Self {
            config_dir,
            data_dir,
            runtime_dir,
        })
    }

    /// True when this instance's data dir is the platform-default
    /// (`ProjectDirs`) data directory rather than a genuine override.
    /// Service installers use this to leave `PIDASH_DATA_DIR` out of the
    /// unit they write: baking the default into the unit made the daemon
    /// look like an isolated install and moved its IPC socket away from
    /// where an override-less CLI expects it (PDASHOSS01-230).
    pub fn data_dir_is_default(&self) -> bool {
        ProjectDirs::from(QUALIFIER, ORG, APP)
            .map(|dirs| same_path(&self.data_dir, dirs.data_dir()))
            .unwrap_or(false)
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn credentials_path(&self) -> PathBuf {
        self.config_dir.join("credentials.toml")
    }

    /// Daemon-level logs directory. Used only for service-supervisor
    /// stdout/stderr (launchd / systemd unit redirection); per-runner
    /// logs live under `RunnerPaths::logs_dir()`.
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn pid_path(&self) -> PathBuf {
        self.runtime_dir.join("pid")
    }

    /// Crash-safe journal of `RunFailed` signals the daemon could not
    /// deliver on shutdown (see `daemon::drain_journal`).
    pub fn drain_journal_path(&self) -> PathBuf {
        self.data_dir.join("pending_run_failures.json")
    }

    pub fn ipc_socket_path(&self) -> PathBuf {
        self.runtime_dir.join("pidash.sock")
    }

    pub fn default_working_dir(&self) -> PathBuf {
        let base = std::env::var("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join(".pidash")
    }

    /// Per-runner data directory: `data_dir/runners/<runner_id>/`.
    pub fn runner_dir(&self, runner_id: Uuid) -> PathBuf {
        self.data_dir.join("runners").join(runner_id.to_string())
    }

    /// Build a `RunnerPaths` rooted at this runner's data directory.
    pub fn for_runner(&self, runner_id: Uuid) -> RunnerPaths {
        RunnerPaths {
            runner_id,
            base_dir: self.runner_dir(runner_id),
        }
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.runtime_dir,
            &self.logs_dir(),
        ] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {dir:?}"))?;
        }
        Ok(())
    }
}

impl RunnerPaths {
    /// Root of this runner's per-instance tree.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// `<base>/history/`.
    pub fn history_dir(&self) -> PathBuf {
        self.base_dir.join("history")
    }

    /// `<base>/history/runs/` — one file per run.
    pub fn runs_dir(&self) -> PathBuf {
        self.history_dir().join("runs")
    }

    /// `<base>/history/runs_index.json`.
    pub fn runs_index_path(&self) -> PathBuf {
        self.history_dir().join("runs_index.json")
    }

    /// `<base>/logs/`.
    pub fn logs_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }

    /// `<base>/identity.toml` — runner_id, name, registered_at, workspace_slug.
    pub fn identity_path(&self) -> PathBuf {
        self.base_dir.join("identity.toml")
    }

    /// `<base>/credentials.toml` — legacy per-runner refresh-token state.
    pub fn credentials_path(&self) -> PathBuf {
        self.base_dir.join("credentials.toml")
    }

    /// Create the runner's directory tree on disk.
    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.base_dir, &self.runs_dir(), &self.logs_dir()] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {dir:?}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_override_keeps_the_shared_socket_path() {
        // Regression for PDASHOSS01-230: service units written by
        // `pidash install` exported PIDASH_DATA_DIR=<default>, which made
        // the daemon resolve an "isolated" socket under <data>/runtime/
        // while the CLI (no override) looked in the XDG runtime dir.
        // An override that *is* the default must resolve identically to
        // no override at all.
        let default_data_dir = ProjectDirs::from(QUALIFIER, ORG, APP)
            .unwrap()
            .data_dir()
            .to_path_buf();
        let plain = Paths::resolve(None, None).unwrap();
        let defaulted = Paths::resolve(None, Some(default_data_dir)).unwrap();
        assert_eq!(defaulted.ipc_socket_path(), plain.ipc_socket_path());
        assert_eq!(defaulted.pid_path(), plain.pid_path());
        assert_eq!(defaulted.runtime_dir, plain.runtime_dir);
    }

    #[test]
    fn is_isolated_only_for_a_genuinely_different_override() {
        // Deterministic core of the PDASHOSS01-230 regression: the
        // resolve-level test above can't tell old from new behavior on a
        // host without XDG_RUNTIME_DIR (both branches fall back to
        // <data>/runtime there), so pin the decision itself.
        let tmp = tempfile::tempdir().unwrap();
        let default = tmp.path().join("default-data");
        assert!(!is_isolated(None, &default));
        assert!(
            !is_isolated(Some(&default), &default),
            "an override naming the default data dir must not isolate the socket"
        );
        assert!(is_isolated(Some(&tmp.path().join("app-data")), &default));
        // A symlinked spelling of the default is still the default.
        std::fs::create_dir(&default).unwrap();
        let link = tmp.path().join("default-link");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&default, &link).unwrap();
            assert!(!is_isolated(Some(&link), &default));
        }
    }

    #[test]
    fn same_path_sees_through_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(unix)]
        assert!(same_path(&link, &real));
        // Non-existent paths fall back to lexical comparison.
        let ghost = tmp.path().join("ghost");
        assert!(same_path(&ghost, &ghost.clone()));
        assert!(!same_path(&ghost, &real));
    }

    #[test]
    fn data_dir_is_default_distinguishes_override_from_default() {
        let plain = Paths::resolve(None, None).unwrap();
        assert!(plain.data_dir_is_default());
        let tmp = tempfile::tempdir().unwrap();
        let isolated = Paths::resolve(None, Some(tmp.path().join("app-data"))).unwrap();
        assert!(!isolated.data_dir_is_default());
    }

    #[test]
    fn data_override_isolates_daemon_socket_and_pid() {
        let tmp = tempfile::tempdir().unwrap();
        let first = Paths::resolve(None, Some(tmp.path().join("first"))).unwrap();
        let second = Paths::resolve(None, Some(tmp.path().join("second"))).unwrap();
        assert_eq!(first.runtime_dir, tmp.path().join("first/runtime"));
        assert_ne!(first.ipc_socket_path(), second.ipc_socket_path());
        assert_ne!(first.pid_path(), second.pid_path());
    }

    fn fixed_id() -> Uuid {
        Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap()
    }

    fn paths_at(base: &Path) -> Paths {
        Paths {
            config_dir: base.join("config"),
            data_dir: base.join("data"),
            runtime_dir: base.join("runtime"),
        }
    }

    #[test]
    fn runner_dir_is_under_data_runners_runner_id() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_at(tmp.path());
        let id = fixed_id();
        let dir = paths.runner_dir(id);
        assert_eq!(
            dir,
            tmp.path().join("data").join("runners").join(id.to_string())
        );
    }

    #[test]
    fn for_runner_bakes_in_runner_id_and_base_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_at(tmp.path());
        let id = fixed_id();
        let rp = paths.for_runner(id);
        assert_eq!(rp.runner_id, id);
        assert_eq!(rp.base_dir(), paths.runner_dir(id));
    }

    #[test]
    fn runner_paths_compose_history_runs_logs_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_at(tmp.path());
        let rp = paths.for_runner(fixed_id());
        let base = rp.base_dir().to_path_buf();
        assert_eq!(rp.history_dir(), base.join("history"));
        assert_eq!(rp.runs_dir(), base.join("history").join("runs"));
        assert_eq!(
            rp.runs_index_path(),
            base.join("history").join("runs_index.json")
        );
        assert_eq!(rp.logs_dir(), base.join("logs"));
        assert_eq!(rp.identity_path(), base.join("identity.toml"));
    }

    #[test]
    fn runner_paths_ensure_creates_history_runs_and_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_at(tmp.path());
        let rp = paths.for_runner(fixed_id());
        rp.ensure().unwrap();
        assert!(rp.runs_dir().is_dir(), "runs dir should be created");
        assert!(rp.logs_dir().is_dir(), "logs dir should be created");
        assert!(rp.base_dir().is_dir(), "base dir should be created");
    }

    #[test]
    fn two_runners_get_disjoint_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_at(tmp.path());
        let a = paths.for_runner(Uuid::new_v4());
        let b = paths.for_runner(Uuid::new_v4());
        assert_ne!(a.base_dir(), b.base_dir());
        assert_ne!(a.runs_dir(), b.runs_dir());
        assert_ne!(a.logs_dir(), b.logs_dir());
    }
}
