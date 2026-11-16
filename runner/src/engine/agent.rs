//! `agent.kind = "engine"`: how an engine runner's block becomes a Codex
//! launch (design §5).
//!
//! An engine block names no paths. At spawn the daemon supplies them:
//!
//! - the engine binary resolved once at daemon start (`engine::locate`);
//! - `CODEX_HOME` = `<daemon data>/engine/<account>/codex-home`, whose
//!   `config.toml` the engine manager writes from the profile;
//! - the directory of the running `pidash` prepended to the agent's `PATH`,
//!   so the agent's own `pidash issue|comment|workpad` calls (and the auth
//!   helper) are this release's CLI.
//!
//! The result runs through the existing Codex driver unchanged.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::schema::{AgentKind, RunnerConfig};

/// Launch context the engine manager publishes for this daemon process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLaunch {
    /// The account's `CODEX_HOME`, holding a written `config.toml`.
    pub codex_home: PathBuf,
}

static LAUNCH: RwLock<Option<EngineLaunch>> = RwLock::new(None);

/// Publish (or, with `None`, withdraw) the launch context. Set by the
/// engine manager whenever it writes the engine config.
pub fn set_launch(launch: Option<EngineLaunch>) {
    *LAUNCH.write().expect("engine launch lock poisoned") = launch;
}

pub fn launch() -> Option<EngineLaunch> {
    LAUNCH.read().expect("engine launch lock poisoned").clone()
}

/// The engine binary as the daemon reports it (version probe, status).
pub fn binary_display() -> &'static str {
    super::locate::resolved()
        .and_then(|e| e.binary.to_str())
        .unwrap_or(super::ENGINE_BINARY_NAME)
}

/// Turn an engine block into the Codex settings it launches with. Pure, so
/// it is testable without a daemon.
pub fn engine_runner_config(
    runner: &RunnerConfig,
    binary: &Path,
    codex_home: &Path,
    path_prepend: Option<PathBuf>,
) -> RunnerConfig {
    let mut out = runner.clone();
    out.codex.binary = binary.to_string_lossy().into_owned();
    out.codex.codex_home = Some(codex_home.to_path_buf());
    out.codex.path_prepend = path_prepend;
    // No token file: the auth helper asks the daemon over the socket.
    out.codex.model_token_file = None;
    // The model comes from the profile, through CODEX_HOME/config.toml.
    out.codex.model_default = None;
    out.codex.effort_default = None;
    out
}

/// Directory of the running `pidash`, for the agent's `PATH`.
fn pidash_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The settings an engine runner spawns with, or why it cannot start.
pub fn prepare(runner: &RunnerConfig) -> anyhow::Result<RunnerConfig> {
    debug_assert_eq!(runner.agent.kind, AgentKind::Engine);
    let engine = super::locate::resolved().ok_or_else(|| {
        anyhow::anyhow!(
            "{}: the built-in engine was not found when the daemon started; run `pidash engine install` and restart the daemon",
            super::api::reason::ENGINE_NOT_INSTALLED
        )
    })?;
    let launch = launch().ok_or_else(|| {
        anyhow::anyhow!(
            "{}: the built-in engine is not ready on this computer (no model profile yet); see `pidash status`",
            super::api::reason::PROFILE_PENDING
        )
    })?;
    Ok(engine_runner_config(
        runner,
        &engine.binary,
        &launch.codex_home,
        pidash_dir(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{AgentSection, WorkspaceSection};

    #[test]
    fn engine_block_launches_codex_with_the_daemon_supplied_paths() {
        let runner = RunnerConfig {
            name: "builtin-laptop-acme-web".into(),
            runner_id: uuid::Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("WEB".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: PathBuf::from("/tasks/acme/WEB"),
            },
            agent: AgentSection {
                kind: AgentKind::Engine,
            },
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        };
        let home = PathBuf::from("/data/engine/u1/codex-home");
        let out = engine_runner_config(
            &runner,
            Path::new("/lib/pidash/engine/0.153.4/pidash-agent-engine"),
            &home,
            Some(PathBuf::from("/bin")),
        );
        assert_eq!(
            out.codex.binary,
            "/lib/pidash/engine/0.153.4/pidash-agent-engine"
        );
        assert_eq!(out.codex.codex_home.as_deref(), Some(home.as_path()));
        assert!(out.codex.model_token_file.is_none());
        assert_eq!(out.agent.kind, AgentKind::Engine);
        // The agent environment exports this CODEX_HOME, never ~/.codex.
        let env = crate::agent::agent_env_for_config(&out);
        assert_eq!(env.codex_home.as_deref(), Some(home.as_path()));
        assert!(env.model_token_file.is_none());
        // The block on disk is untouched: it holds no paths.
        assert!(runner.codex.codex_home.is_none());
        assert_eq!(runner.codex.binary, "codex");
    }

    #[test]
    fn engine_block_round_trips_as_kind_engine_with_no_paths() {
        let toml_text = r#"
            name = "e"
            runner_id = "00000000-0000-0000-0000-000000000001"
            project_slug = "WEB"
            [workspace]
            working_dir = "/tasks/acme/WEB"
            [agent]
            kind = "engine"
        "#;
        let r: RunnerConfig = toml::from_str(toml_text).unwrap();
        assert_eq!(r.agent.kind, AgentKind::Engine);
        let back = toml::to_string(&r).unwrap();
        assert!(back.contains("kind = \"engine\""), "{back}");
        assert!(!back.contains("codex_home"), "{back}");
        assert_eq!(
            <AgentKind as clap::ValueEnum>::from_str("engine", false).unwrap(),
            AgentKind::Engine
        );
    }
}
