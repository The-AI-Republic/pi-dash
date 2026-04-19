use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

const QUALIFIER: &str = "so";
const ORG: &str = "pi-dash";
const APP: &str = "runner";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
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

    pub fn history_dir(&self) -> PathBuf {
        self.data_dir.join("history")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.history_dir().join("runs")
    }

    pub fn runs_index_path(&self) -> PathBuf {
        self.history_dir().join("runs_index.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn pid_path(&self) -> PathBuf {
        self.runtime_dir.join("pid")
    }

    pub fn ipc_socket_path(&self) -> PathBuf {
        self.runtime_dir.join("runner.sock")
    }

    pub fn default_working_dir(&self) -> PathBuf {
        let base = std::env::var("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join(".pi_dash")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.runtime_dir,
            &self.runs_dir(),
            &self.logs_dir(),
        ] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {dir:?}"))?;
        }
        Ok(())
    }
}
