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

impl Paths {
    pub fn resolve(
        config_override: Option<PathBuf>,
        data_override: Option<PathBuf>,
    ) -> Result<Self> {
        let dirs = ProjectDirs::from(QUALIFIER, ORG, APP)
            .context("unable to resolve XDG project directories")?;
        let config_dir = config_override.unwrap_or_else(|| dirs.config_dir().to_path_buf());
        let data_dir = data_override.unwrap_or_else(|| dirs.data_dir().to_path_buf());
        let runtime_dir = dirs
            .runtime_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| data_dir.join("runtime"));
        Ok(Self {
            config_dir,
            data_dir,
            runtime_dir,
        })
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

    /// `<base>/credentials.toml` — per-runner refresh-token state.
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
