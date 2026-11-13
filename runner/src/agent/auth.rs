//! Credential presence checks per agent kind.
//!
//! Each check is cheap and local or near-local: no model call, no tokens
//! spent. Presence is determined from the daemon's actual environment
//! (the same login-shell environment `spawn_from_config` uses) and files
//! under the daemon's home directory. A key set only in an interactive
//! shell does not count — only what the daemon sees.
//!
//! The check is shared by `pidash doctor` and the pre-`Accept` gate in
//! `RunnerLoop` (see `daemon::supervisor`). Missing credentials block
//! the run before `spawn_from_config` ever launches the agent, so the
//! run ends with `FailureReason::AgentAuthMissing` and does not burn the
//! issue's tick budget.

use std::path::{Path, PathBuf};

use crate::config::schema::AgentKind;

/// Resolve the home directory the daemon uses for `~`.
/// Prefers `directories::BaseDirs::home_dir()` (the XDG/.pidash
/// convention the daemon already uses) and falls back to `$HOME`.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(bases) = directories::BaseDirs::new() {
        return Some(bases.home_dir().to_path_buf());
    }
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// True when the file at `path` exists and is a non-empty regular file.
fn file_present(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.len() > 0,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// The env var Codex reads for API credentials.
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";

/// Primary Codex auth file: `~/.codex/auth.json`.
pub fn codex_auth_file(home: &Path) -> PathBuf {
    home.join(".codex").join("auth.json")
}

/// Pure presence check for tests: env key and home dir.
pub fn codex_credentials_present(openai_api_key: Option<&str>, home: &Path) -> bool {
    if openai_api_key.is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if file_present(&codex_auth_file(home)) {
        return true;
    }
    // CODEX_HOME override, when set.
    if let Some(codex_home) = std::env::var_os("CODEX_HOME") {
        let path = PathBuf::from(codex_home).join("auth.json");
        if file_present(&path) {
            return true;
        }
    }
    false
}

pub fn has_codex_credentials() -> bool {
    let key = std::env::var(OPENAI_API_KEY_ENV).ok();
    if key.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if let Some(home) = home_dir() {
        if file_present(&codex_auth_file(&home)) {
            return true;
        }
        if let Some(codex_home) = std::env::var_os("CODEX_HOME") {
            let path = PathBuf::from(codex_home).join("auth.json");
            if file_present(&path) {
                return true;
            }
        }
    }
    false
}

pub const CODEX_FIX_HINT: &str =
    "run `codex login` or set OPENAI_API_KEY for the daemon's environment";

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

pub const ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// Candidate auth file locations for Claude Code. Presence of any
/// non-empty file counts.
pub fn claude_auth_files(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude.json"),
        home.join(".claude").join("credentials.json"),
        home.join(".config").join("claude").join("auth.json"),
    ]
}

pub fn claude_credentials_present(anthropic_api_key: Option<&str>, home: &Path) -> bool {
    if anthropic_api_key.is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    for p in claude_auth_files(home) {
        if file_present(&p) {
            return true;
        }
    }
    false
}

pub fn has_claude_credentials() -> bool {
    let key = std::env::var(ANTHROPIC_API_KEY_ENV).ok();
    if key.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if let Some(home) = home_dir() {
        return claude_credentials_present(None, &home);
    }
    false
}

pub const CLAUDE_FIX_HINT: &str =
    "run `claude /login` or set ANTHROPIC_API_KEY for the daemon's environment";

// ---------------------------------------------------------------------------
// Cursor Agent
// ---------------------------------------------------------------------------

pub const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";

pub fn cursor_auth_files(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".cursor").join("auth.json"),
        home.join(".config").join("cursor-agent").join("auth.json"),
    ]
}

pub fn cursor_credentials_present(cursor_api_key: Option<&str>, home: &Path) -> bool {
    if cursor_api_key.is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    for p in cursor_auth_files(home) {
        if file_present(&p) {
            return true;
        }
    }
    false
}

pub fn has_cursor_credentials() -> bool {
    let key = std::env::var(CURSOR_API_KEY_ENV).ok();
    if key.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if let Some(home) = home_dir() {
        return cursor_credentials_present(None, &home);
    }
    false
}

pub const CURSOR_FIX_HINT: &str =
    "run `cursor-agent login` or set CURSOR_API_KEY for the daemon's environment";

// ---------------------------------------------------------------------------
// Grok
// ---------------------------------------------------------------------------

pub const XAI_API_KEY_ENV: &str = "XAI_API_KEY";

pub fn grok_auth_files(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".grok").join("auth.json"),
        home.join(".config").join("grok").join("auth.json"),
    ]
}

pub fn grok_credentials_present(xai_api_key: Option<&str>, home: &Path) -> bool {
    if xai_api_key.is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    for p in grok_auth_files(home) {
        if file_present(&p) {
            return true;
        }
    }
    false
}

pub fn has_grok_credentials() -> bool {
    let key = std::env::var(XAI_API_KEY_ENV).ok();
    if key.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if let Some(home) = home_dir() {
        return grok_credentials_present(None, &home);
    }
    false
}

pub const GROK_FIX_HINT: &str = "set XAI_API_KEY or run `grok login` for the daemon's environment";

// ---------------------------------------------------------------------------
// Muse Code (delegated to crate::muse_code::auth for the single source of truth)
// ---------------------------------------------------------------------------

pub fn has_muse_credentials() -> bool {
    crate::muse_code::auth::has_muse_credentials()
}

pub const MUSE_FIX_HINT: &str = crate::muse_code::auth::FIX_HINT;

// ---------------------------------------------------------------------------
// OpenClaw / Engine — no reliable presence check; skip blocking.
// ---------------------------------------------------------------------------

pub const OPENCLAW_FIX_HINT: &str =
    "ensure OpenClaw is authenticated and an OpenClaw Gateway is reachable";

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// True when the daemon should let a run for `kind` start. `false` means
/// credentials are provably missing and the Assign must be NACKed
/// before `Accept`.
///
/// `OpenClaw` and `Engine` have no reliable cheap local check: OpenClaw is
/// driven via `acpx` + an OpenClaw Gateway (auth + gateway reachability
/// have no non-interactive probe), and Engine's sign-in is managed by Pi
/// Dash itself. Both are treated as "unknown — skip" and let through so
/// runs are never blocked by a guess.
pub fn has_credentials(kind: AgentKind) -> bool {
    match kind {
        AgentKind::Codex => has_codex_credentials(),
        AgentKind::ClaudeCode => has_claude_credentials(),
        AgentKind::CursorAgent => has_cursor_credentials(),
        AgentKind::Grok => has_grok_credentials(),
        AgentKind::MuseCode => has_muse_credentials(),
        AgentKind::OpenClaw | AgentKind::Engine => true,
    }
}

/// Fix hint for a missing-credentials detail. Callers format this with
/// the agent and runner context.
pub fn fix_hint(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Codex => CODEX_FIX_HINT,
        AgentKind::ClaudeCode => CLAUDE_FIX_HINT,
        AgentKind::CursorAgent => CURSOR_FIX_HINT,
        AgentKind::Grok => GROK_FIX_HINT,
        AgentKind::MuseCode => MUSE_FIX_HINT,
        AgentKind::OpenClaw => OPENCLAW_FIX_HINT,
        AgentKind::Engine => "run `pidash login --engine` for the daemon's environment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn empty_home(dir: &TempDir) -> PathBuf {
        dir.path().to_path_buf()
    }

    fn write_file(home: &Path, rel: &str, contents: &str) {
        let path = home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }

    // ---- Codex ----
    #[test]
    fn codex_env_var_counts_as_present() {
        let dir = TempDir::new().unwrap();
        assert!(codex_credentials_present(Some("sk-x"), &empty_home(&dir)));
    }

    #[test]
    fn codex_auth_file_counts_as_present() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".codex/auth.json", "{\"token\":\"x\"}");
        assert!(codex_credentials_present(None, &home));
    }

    #[test]
    fn codex_empty_auth_file_does_not_count() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".codex/auth.json", "");
        assert!(!codex_credentials_present(None, &home));
    }

    #[test]
    fn codex_neither_is_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!codex_credentials_present(None, &empty_home(&dir)));
        assert!(!codex_credentials_present(Some(""), &empty_home(&dir)));
        assert!(!codex_credentials_present(Some("   "), &empty_home(&dir)));
    }

    #[test]
    fn codex_env_wins_over_empty_file() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".codex/auth.json", "");
        assert!(codex_credentials_present(Some("sk-x"), &home));
    }

    // ---- Claude Code ----
    #[test]
    fn claude_env_var_counts_as_present() {
        let dir = TempDir::new().unwrap();
        assert!(claude_credentials_present(
            Some("sk-ant-x"),
            &empty_home(&dir)
        ));
    }

    #[test]
    fn claude_auth_file_counts_as_present() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".claude.json", "{\"oauth\":\"x\"}");
        assert!(claude_credentials_present(None, &home));
    }

    #[test]
    fn claude_empty_file_does_not_count() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".claude.json", "");
        assert!(!claude_credentials_present(None, &home));
    }

    #[test]
    fn claude_neither_is_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!claude_credentials_present(None, &empty_home(&dir)));
    }

    // ---- Cursor Agent ----
    #[test]
    fn cursor_env_var_counts_as_present() {
        let dir = TempDir::new().unwrap();
        assert!(cursor_credentials_present(Some("cur-x"), &empty_home(&dir)));
    }

    #[test]
    fn cursor_auth_file_counts_as_present() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".cursor/auth.json", "{\"k\":\"x\"}");
        assert!(cursor_credentials_present(None, &home));
    }

    #[test]
    fn cursor_empty_file_does_not_count() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".cursor/auth.json", "");
        assert!(!cursor_credentials_present(None, &home));
    }

    #[test]
    fn cursor_neither_is_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!cursor_credentials_present(None, &empty_home(&dir)));
    }

    // ---- Grok ----
    #[test]
    fn grok_env_var_counts_as_present() {
        let dir = TempDir::new().unwrap();
        assert!(grok_credentials_present(Some("xai-x"), &empty_home(&dir)));
    }

    #[test]
    fn grok_auth_file_counts_as_present() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".grok/auth.json", "{\"t\":\"x\"}");
        assert!(grok_credentials_present(None, &home));
    }

    #[test]
    fn grok_empty_file_does_not_count() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        write_file(&home, ".grok/auth.json", "");
        assert!(!grok_credentials_present(None, &home));
    }

    #[test]
    fn grok_neither_is_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!grok_credentials_present(None, &empty_home(&dir)));
    }

    // ---- Dispatcher ----
    #[test]
    fn openclaw_and_engine_are_always_allowed() {
        assert!(has_credentials(AgentKind::OpenClaw));
        assert!(has_credentials(AgentKind::Engine));
    }

    #[test]
    fn fix_hints_are_non_empty() {
        for kind in [
            AgentKind::Codex,
            AgentKind::ClaudeCode,
            AgentKind::CursorAgent,
            AgentKind::Grok,
            AgentKind::MuseCode,
        ] {
            assert!(!fix_hint(kind).is_empty(), "fix hint for {kind:?}");
        }
    }
}
