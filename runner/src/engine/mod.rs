//! The built-in agent engine that ships with the CLI (design
//! `.ai_design/built_engine_as_runner/design.md` §4.5).
//!
//! The engine is the open-source OpenAI Codex binary, unmodified, renamed to
//! `pidash-agent-engine`. Every CLI release archive carries it under
//! `engine/<version>/`, and it is installed next to `pidash` but never on
//! `PATH`:
//!
//! | Installer                | Engine directory                                  |
//! | ------------------------ | ------------------------------------------------- |
//! | shell (macOS, Linux)     | `<install>/../lib/pidash/engine/<version>/`       |
//! | PowerShell, MSI          | `<install dir>\engine\<version>\`                 |
//! | extracted archive        | `<dir of pidash>/engine/<version>/`               |
//!
//! - [`locate`] finds the engine relative to the running executable (or the
//!   `[engine] binary` config override) once, at daemon start.
//! - [`install`] places a release's engine directory, for `pidash engine
//!   install` and for `pidash update` before it replaces `pidash`.
//! - [`auto_install`] installs a missing engine at daemon start on an
//!   updater-managed install, for installs updated in place from a `pidash`
//!   that predates the engine (its `pidash update` had no engine step).
//! - [`cleanup`] removes engine versions older than the running one.
//!
//! Running it (PIDESKAPP-121, design §4.3 and §5):
//!
//! - [`api`] is the server contract (profile, model token, credential
//!   revoke, pause) in one place.
//! - [`manager`] holds the machine's engine state in the daemon: the
//!   engine credential, the profile (fetched at start, on credential issue
//!   and every 15 minutes) and the in-memory model token.
//! - [`codex_config`] writes the engine's private `CODEX_HOME`.
//! - [`free_quota`] classifies the OpenHub free-lane answers by error code.
//! - [`agent`] turns an `agent.kind = "engine"` block into a Codex launch.
//! - [`provision`] turns the engine on for a project (`EngineRunnerAdd`,
//!   `pidash runner add --agent engine`).
//! - [`workdir`] owns `[engine] task_root`: validation and folder allocation.
//!
//! The pinned engine release is `runner/ENGINE_VERSION`, the single source of
//! truth shared with the CLI release pipeline (`runner/scripts/stage_engine.py`)
//! and the desktop build scripts.

pub mod agent;
pub mod api;
pub mod auto_install;
pub mod cleanup;
pub mod codex_config;
pub mod free_quota;
pub mod install;
pub mod locate;
pub mod manager;
pub mod provision;
#[cfg(test)]
pub(crate) mod test_server;
pub mod tombstones;
pub mod workdir;

/// Upstream release tag of the pinned engine, e.g. `rust-v0.153.4`.
const ENGINE_TAG_FILE: &str = include_str!("../../ENGINE_VERSION");

/// File name of the engine executable on this platform.
pub const ENGINE_BINARY_NAME: &str = if cfg!(windows) {
    "pidash-agent-engine.exe"
} else {
    "pidash-agent-engine"
};

/// The pinned upstream Codex release tag (`rust-v<version>`).
pub fn engine_tag() -> &'static str {
    ENGINE_TAG_FILE.trim()
}

/// The engine version this `pidash` was released with, as it names the
/// per-version directory: the upstream tag without its `rust-v` prefix.
pub fn engine_version() -> &'static str {
    version_from_tag(engine_tag())
}

/// `rust-v0.153.4` -> `0.153.4`. Mirrors `version_from_tag` in
/// `runner/scripts/stage_engine.py`; the two must agree on directory names.
pub fn version_from_tag(tag: &str) -> &str {
    tag.strip_prefix("rust-v").unwrap_or(tag)
}

/// Engine executable file name for a platform. Tests pass `windows`
/// explicitly so every layout is covered on every host.
pub(crate) fn binary_name(windows: bool) -> &'static str {
    if windows {
        "pidash-agent-engine.exe"
    } else {
        "pidash-agent-engine"
    }
}

/// A directory name that is a valid engine version (`0.153.4`,
/// `0.154.0-alpha.1`). Anything else under the engine root is ignored by the
/// locator, the installer and the cleanup.
pub(crate) fn parse_version(name: &str) -> Option<semver::Version> {
    semver::Version::parse(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_engine_version_is_a_semver_directory_name() {
        assert!(engine_tag().starts_with("rust-v"), "{}", engine_tag());
        assert!(
            parse_version(engine_version()).is_some(),
            "ENGINE_VERSION must hold `rust-v<semver>`, got {:?}",
            engine_tag()
        );
    }

    #[test]
    fn binary_name_matches_platform_constant() {
        assert_eq!(binary_name(cfg!(windows)), ENGINE_BINARY_NAME);
    }
}
