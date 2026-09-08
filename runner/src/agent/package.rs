//! Developer-owned agent packages, adapted through an existing runner protocol.
//!
//! This is a configuration seam, not an installer or plugin loader. The caller
//! supplies trusted local executable bytes and owns their credentials, updates,
//! licensing and runtime configuration. No package code is executed here.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config::schema::{AgentKind, RunnerConfig};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    protocol: AgentKind,
    executable: PathBuf,
}

/// A validated local entry point compatible with one of Pi Dash's bridges.
///
/// `protocol` describes the CLI/wire contract, not the package's vendor. For
/// example, a developer's own engine can implement the Codex app-server
/// contract and be driven by the existing Codex bridge.
#[derive(Debug, Clone)]
pub struct AgentPackage {
    protocol: AgentKind,
    executable: PathBuf,
}

impl AgentPackage {
    /// Read a v1 TOML manifest. Relative executables resolve beside the
    /// manifest, never relative to an issue's working directory or PATH.
    /// Resolving a manifest does not run even a `--version` probe.
    pub fn from_manifest(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).context("reading agent package manifest")?;
        let manifest: Manifest = toml::from_str(&text).context("parsing agent package manifest")?;
        if manifest.version != 1 {
            bail!(
                "unsupported agent package manifest version {}; expected 1",
                manifest.version
            );
        }
        if manifest.executable.as_os_str().is_empty() {
            bail!("agent package executable must not be empty");
        }
        let executable = if manifest.executable.is_absolute() {
            manifest.executable
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(manifest.executable)
        };
        let executable = executable
            .canonicalize()
            .context("resolving agent package executable; install the package yourself first")?;
        if !executable.is_file() {
            bail!("agent package executable must be a file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if executable.metadata()?.permissions().mode() & 0o111 == 0 {
                bail!(
                    "agent package executable is not executable; package permissions are owned by the integrator"
                );
            }
        }
        if executable.to_str().is_none() {
            bail!("agent package executable path must be valid UTF-8");
        }
        Ok(Self {
            protocol: manifest.protocol,
            executable,
        })
    }

    /// Configure an already-enrolled, user-owned runner. The daemon's normal
    /// AgentBridge path consumes the resulting config after restart, including
    /// its existing approval, cancellation and task-directory behavior.
    /// No cloud identity, workspace, policy, credential or model is changed.
    pub fn apply_to_runner(&self, runner: &mut RunnerConfig) -> Result<()> {
        // A desktop host owns these fields and would overwrite a manual edit.
        // Do not silently mix a third-party package with its model credential.
        if runner.codex.codex_home.is_some()
            || runner.codex.path_prepend.is_some()
            || runner.codex.model_token_file.is_some()
        {
            bail!(
                "this runner is owned by a managed host; configure its package through that host instead"
            );
        }
        let executable = self
            .executable
            .to_str()
            .context("agent executable path must be UTF-8")?
            .to_owned();
        match self.protocol {
            AgentKind::Codex => runner.codex.binary = executable,
            AgentKind::ClaudeCode => runner.claude_code.binary = executable,
            AgentKind::CursorAgent => runner.cursor_agent.binary = executable,
            AgentKind::OpenClaw => runner.openclaw.binary = executable,
            AgentKind::Grok => runner.grok.binary = executable,
        }
        runner.agent.kind = self.protocol;
        Ok(())
    }
}
