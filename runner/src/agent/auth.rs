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

use crate::agent::keychain::{KeychainPresence, keychain_item_present};
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

/// True when the runner is a Pi Dash-managed Codex runner (desktop host).
///
/// Managed runners carry Pi Dash-managed auth via `codex_home` +
/// `model_token_file` and the user's `~/.codex` is ignored, so the
/// presence gate and doctor check must not run for them — the same
/// out-of-scope class as `Engine`. Config validation requires the fields
/// are set together, so checking `codex_home` or `model_token_file` is
/// enough; both are checked to be explicit.
pub fn is_managed_codex(codex: &crate::config::schema::CodexSection) -> bool {
    codex.codex_home.is_some() || codex.model_token_file.is_some()
}

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
// Antigravity (agy)
// ---------------------------------------------------------------------------

/// The env var `agy` reads for API-key auth.
pub const GEMINI_API_KEY_ENV: &str = "GEMINI_API_KEY";

/// Set to `true` for the Google Cloud application-default-credentials
/// fallback on headless servers whose keyring is not running or stays
/// locked.
pub const AGY_ADC_AUTH_ENV: &str = "AGY_ADC_AUTH";

/// Keychain service name `agy` stores its Google sign-in under. Matched on
/// the service alone (see [`crate::agent::keychain`]) so per-account
/// variants all count; see the Step 0 recording on NEWPIDASH-397.
pub const ANTIGRAVITY_KEYCHAIN_SERVICE: &str = "gemini";

/// Extra Secret Service attributes narrowing the Linux search. The `service`
/// attribute is always set from [`ANTIGRAVITY_KEYCHAIN_SERVICE`] as well.
pub const ANTIGRAVITY_KEYCHAIN_ATTRS: &[(&str, &str)] = &[("service", "gemini")];

pub const ANTIGRAVITY_FIX_HINT: &str = "run `agy` and sign in (or `agy acp --login` over SSH), or set GEMINI_API_KEY with modelProvider=gemini, for the daemon's user";

pub const ANTIGRAVITY_KEYRING_LOCKED_HINT: &str =
    "Antigravity sign-in found but the keyring is locked; unlock it or use GEMINI_API_KEY";

/// True when `GEMINI_API_KEY` is set in the daemon's environment and the agy
/// CLI settings select the Gemini provider. `agy` refuses to start with
/// `modelProvider` set to gemini and no key, so the key alone is not
/// enough; conversely a key with any other provider is ignored by `agy`.
pub fn antigravity_api_key_present(
    gemini_api_key: Option<&str>,
    model_provider_is_gemini: bool,
) -> bool {
    model_provider_is_gemini && gemini_api_key.is_some_and(|v| !v.trim().is_empty())
}

/// True when the headless-server fallback is selected: Google Cloud
/// application default credentials.
pub fn antigravity_adc_present(agy_adc_auth: Option<&str>) -> bool {
    agy_adc_auth.is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

/// Pure presence check for tests: env values, the parsed settings file, and
/// the keychain probe result.
///
/// Passes when the API key path holds, the ADC fallback is selected, the
/// keychain holds an `agy` sign-in item (locked or unlocked — the daemon
/// may still reach it once the session unlocks the keyring), or the
/// keychain did not answer (never block on a guess, the same as OpenClaw).
/// Fails only when nothing is found: no key, no ADC credentials, and no
/// keychain item.
pub fn antigravity_credentials_present(
    gemini_api_key: Option<&str>,
    model_provider_is_gemini: bool,
    agy_adc_auth: Option<&str>,
    keychain: KeychainPresence,
) -> bool {
    if antigravity_api_key_present(gemini_api_key, model_provider_is_gemini) {
        return true;
    }
    if antigravity_adc_present(agy_adc_auth) {
        return true;
    }
    match keychain {
        KeychainPresence::Present { .. } | KeychainPresence::Unavailable => true,
        KeychainPresence::Absent => false,
    }
}

/// Which Antigravity setup the daemon sees, for `pidash doctor`. Every
/// variant except [`AntigravityAuthDetail::Missing`] passes the pre-pickup
/// gate; the detail names what was found so the operator knows what the
/// gate passed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntigravityAuthDetail {
    ApiKey,
    GoogleCloudCredentials,
    KeychainItem,
    KeychainLocked,
    KeychainDidNotAnswer,
    Missing,
}

impl std::fmt::Display for AntigravityAuthDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AntigravityAuthDetail::ApiKey => write!(
                f,
                "Antigravity credentials present (GEMINI_API_KEY with modelProvider=gemini)"
            ),
            AntigravityAuthDetail::GoogleCloudCredentials => write!(
                f,
                "Antigravity credentials present (Google Cloud application default credentials)"
            ),
            AntigravityAuthDetail::KeychainItem => {
                write!(f, "Antigravity sign-in found in the OS keychain")
            }
            AntigravityAuthDetail::KeychainLocked => write!(f, "{ANTIGRAVITY_KEYRING_LOCKED_HINT}"),
            AntigravityAuthDetail::KeychainDidNotAnswer => write!(
                f,
                "OS keychain did not answer; letting the Antigravity run through"
            ),
            AntigravityAuthDetail::Missing => {
                write!(f, "missing Antigravity credentials: {ANTIGRAVITY_FIX_HINT}")
            }
        }
    }
}

/// Pure detail decision for tests; priority is explicit config (API key,
/// then ADC) over ambient state (keychain).
pub fn antigravity_auth_detail(
    gemini_api_key: Option<&str>,
    model_provider_is_gemini: bool,
    agy_adc_auth: Option<&str>,
    keychain: KeychainPresence,
) -> AntigravityAuthDetail {
    if antigravity_api_key_present(gemini_api_key, model_provider_is_gemini) {
        return AntigravityAuthDetail::ApiKey;
    }
    if antigravity_adc_present(agy_adc_auth) {
        return AntigravityAuthDetail::GoogleCloudCredentials;
    }
    match keychain {
        KeychainPresence::Present { locked: false } => AntigravityAuthDetail::KeychainItem,
        KeychainPresence::Present { locked: true } => AntigravityAuthDetail::KeychainLocked,
        KeychainPresence::Unavailable => AntigravityAuthDetail::KeychainDidNotAnswer,
        KeychainPresence::Absent => AntigravityAuthDetail::Missing,
    }
}

/// Fix hint for the Antigravity gate: the locked hint when the keychain
/// holds a sign-in but is locked, the nothing-found hint otherwise.
pub fn antigravity_fix_hint(detail: AntigravityAuthDetail) -> &'static str {
    match detail {
        AntigravityAuthDetail::KeychainLocked => ANTIGRAVITY_KEYRING_LOCKED_HINT,
        _ => ANTIGRAVITY_FIX_HINT,
    }
}

/// Reads `~/.gemini/antigravity-cli/settings.json` under `home` and reports
/// whether it selects the Gemini provider (`"modelProvider": "gemini"`).
/// Missing, unreadable, or unparsable files count as "not gemini": the API
/// key path then stays closed and the other paths decide.
pub fn antigravity_model_provider_is_gemini(home: &Path) -> bool {
    let path = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return false,
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => value.get("modelProvider").and_then(|m| m.as_str()) == Some("gemini"),
        Err(_) => false,
    }
}

/// Live check against the daemon's own environment, home directory, and OS
/// keychain. Presence only: never reads a secret, never validates a token.
pub fn has_antigravity_credentials() -> bool {
    antigravity_live_detail() != AntigravityAuthDetail::Missing
}

/// Live version of [`antigravity_auth_detail`] for `pidash doctor`: names
/// which setup was found (API key / Google Cloud credentials / keychain
/// item / keychain locked / keychain didn't answer).
pub fn antigravity_live_detail() -> AntigravityAuthDetail {
    let gemini_api_key = std::env::var(GEMINI_API_KEY_ENV).ok();
    let agy_adc_auth = std::env::var(AGY_ADC_AUTH_ENV).ok();
    let model_provider_is_gemini = home_dir()
        .map(|home| antigravity_model_provider_is_gemini(&home))
        .unwrap_or(false);
    let keychain = keychain_item_present(ANTIGRAVITY_KEYCHAIN_SERVICE, ANTIGRAVITY_KEYCHAIN_ATTRS);
    antigravity_auth_detail(
        gemini_api_key.as_deref(),
        model_provider_is_gemini,
        agy_adc_auth.as_deref(),
        keychain,
    )
}

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
        AgentKind::Antigravity => has_antigravity_credentials(),
        AgentKind::MuseCode => has_muse_credentials(),
        AgentKind::OpenClaw | AgentKind::PidashAgent => true,
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
        AgentKind::Antigravity => antigravity_fix_hint(antigravity_live_detail()),
        AgentKind::MuseCode => MUSE_FIX_HINT,
        AgentKind::OpenClaw => OPENCLAW_FIX_HINT,
        AgentKind::PidashAgent => "run `pidash login --engine` for the daemon's environment",
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

    // ---- Antigravity ----
    use crate::agent::keychain::KeychainPresence;

    const ABSENT: KeychainPresence = KeychainPresence::Absent;
    const UNANSWERED: KeychainPresence = KeychainPresence::Unavailable;
    const UNLOCKED: KeychainPresence = KeychainPresence::Present { locked: false };
    const LOCKED: KeychainPresence = KeychainPresence::Present { locked: true };

    // API-key path: key AND modelProvider=gemini.
    #[test]
    fn antigravity_api_key_with_gemini_provider_passes() {
        assert!(antigravity_credentials_present(
            Some("AIza-x"),
            true,
            None,
            ABSENT
        ));
    }

    #[test]
    fn antigravity_api_key_needs_gemini_provider() {
        // A key alone is not enough: with any other provider agy ignores it.
        assert!(!antigravity_credentials_present(
            Some("AIza-x"),
            false,
            None,
            ABSENT
        ));
    }

    #[test]
    fn antigravity_gemini_provider_without_key_fails() {
        // agy refuses to start in this state, so blocking it is safe.
        assert!(!antigravity_credentials_present(None, true, None, ABSENT));
        assert!(!antigravity_credentials_present(
            Some(""),
            true,
            None,
            ABSENT
        ));
        assert!(!antigravity_credentials_present(
            Some("   "),
            true,
            None,
            ABSENT
        ));
    }

    #[test]
    fn antigravity_blank_key_never_counts() {
        assert!(!antigravity_api_key_present(Some(""), true));
        assert!(!antigravity_api_key_present(Some("  "), true));
        assert!(!antigravity_api_key_present(None, true));
        assert!(!antigravity_api_key_present(Some("AIza-x"), false));
        assert!(antigravity_api_key_present(Some("AIza-x"), true));
    }

    // ADC path.
    #[test]
    fn antigravity_adc_passes_without_anything_else() {
        assert!(antigravity_credentials_present(
            None,
            false,
            Some("true"),
            ABSENT
        ));
    }

    #[test]
    fn antigravity_adc_value_is_case_insensitive_true() {
        assert!(antigravity_adc_present(Some("true")));
        assert!(antigravity_adc_present(Some("TRUE")));
        assert!(antigravity_adc_present(Some(" True ")));
        assert!(!antigravity_adc_present(Some("1")));
        assert!(!antigravity_adc_present(Some("yes")));
        assert!(!antigravity_adc_present(Some("")));
        assert!(!antigravity_adc_present(None));
    }

    // Keychain path: present (locked or unlocked) passes, unanswered
    // passes, absent fails.
    #[test]
    fn antigravity_keychain_item_passes_locked_or_unlocked() {
        assert!(antigravity_credentials_present(None, false, None, UNLOCKED));
        assert!(antigravity_credentials_present(None, false, None, LOCKED));
    }

    #[test]
    fn antigravity_unanswered_keychain_never_blocks() {
        assert!(antigravity_credentials_present(
            None, false, None, UNANSWERED
        ));
        // Even in the state agy itself refuses to start in: without an
        // answer there is nothing to prove missing.
        assert!(antigravity_credentials_present(
            None, true, None, UNANSWERED
        ));
    }

    #[test]
    fn antigravity_nothing_found_fails() {
        assert!(!antigravity_credentials_present(None, false, None, ABSENT));
        assert!(!antigravity_credentials_present(
            None,
            false,
            Some("false"),
            ABSENT
        ));
    }

    // Paths compose: any single passing input is enough.
    #[test]
    fn antigravity_paths_compose() {
        assert!(antigravity_credentials_present(
            Some("AIza-x"),
            true,
            None,
            LOCKED
        ));
        assert!(antigravity_credentials_present(
            None,
            false,
            Some("true"),
            ABSENT
        ));
        assert!(antigravity_credentials_present(
            Some("AIza-x"),
            false,
            None,
            UNLOCKED
        ));
    }

    // Detail decision: names which setup was found.
    #[test]
    fn antigravity_detail_names_each_setup() {
        assert_eq!(
            antigravity_auth_detail(Some("AIza-x"), true, None, ABSENT),
            AntigravityAuthDetail::ApiKey
        );
        assert_eq!(
            antigravity_auth_detail(None, false, Some("true"), ABSENT),
            AntigravityAuthDetail::GoogleCloudCredentials
        );
        assert_eq!(
            antigravity_auth_detail(None, false, None, UNLOCKED),
            AntigravityAuthDetail::KeychainItem
        );
        assert_eq!(
            antigravity_auth_detail(None, false, None, LOCKED),
            AntigravityAuthDetail::KeychainLocked
        );
        assert_eq!(
            antigravity_auth_detail(None, false, None, UNANSWERED),
            AntigravityAuthDetail::KeychainDidNotAnswer
        );
        assert_eq!(
            antigravity_auth_detail(None, false, None, ABSENT),
            AntigravityAuthDetail::Missing
        );
    }

    #[test]
    fn antigravity_detail_prefers_explicit_config() {
        // API key wins over a locked keychain item.
        assert_eq!(
            antigravity_auth_detail(Some("AIza-x"), true, None, LOCKED),
            AntigravityAuthDetail::ApiKey
        );
        // ADC wins over a keychain item.
        assert_eq!(
            antigravity_auth_detail(None, false, Some("true"), UNLOCKED),
            AntigravityAuthDetail::GoogleCloudCredentials
        );
    }

    #[test]
    fn antigravity_detail_and_gate_agree() {
        // Every non-missing detail passes the gate; missing fails it.
        for (detail, key, provider, adc, keychain) in [
            (AntigravityAuthDetail::ApiKey, Some("k"), true, None, ABSENT),
            (
                AntigravityAuthDetail::GoogleCloudCredentials,
                None,
                false,
                Some("true"),
                ABSENT,
            ),
            (
                AntigravityAuthDetail::KeychainItem,
                None,
                false,
                None,
                UNLOCKED,
            ),
            (
                AntigravityAuthDetail::KeychainLocked,
                None,
                false,
                None,
                LOCKED,
            ),
            (
                AntigravityAuthDetail::KeychainDidNotAnswer,
                None,
                false,
                None,
                UNANSWERED,
            ),
            (AntigravityAuthDetail::Missing, None, false, None, ABSENT),
        ] {
            assert_eq!(
                antigravity_auth_detail(key, provider, adc, keychain),
                detail
            );
            assert_eq!(
                antigravity_credentials_present(key, provider, adc, keychain),
                detail != AntigravityAuthDetail::Missing,
                "gate and detail disagree for {detail:?}"
            );
        }
    }

    // Settings-file parser.
    #[test]
    fn antigravity_settings_parser_reads_model_provider() {
        let dir = TempDir::new().unwrap();
        let home = empty_home(&dir);
        // Missing file counts as not-gemini.
        assert!(!antigravity_model_provider_is_gemini(&home));
        // Unparsable file counts as not-gemini.
        write_file(&home, ".gemini/antigravity-cli/settings.json", "{oops");
        assert!(!antigravity_model_provider_is_gemini(&home));
        // Missing key counts as not-gemini.
        write_file(&home, ".gemini/antigravity-cli/settings.json", "{}");
        assert!(!antigravity_model_provider_is_gemini(&home));
        // Non-string value counts as not-gemini.
        write_file(
            &home,
            ".gemini/antigravity-cli/settings.json",
            "{\"modelProvider\": 42}",
        );
        assert!(!antigravity_model_provider_is_gemini(&home));
        // Another provider counts as not-gemini.
        write_file(
            &home,
            ".gemini/antigravity-cli/settings.json",
            "{\"modelProvider\": \"other\"}",
        );
        assert!(!antigravity_model_provider_is_gemini(&home));
        // Nested noise is fine as long as the top-level key matches.
        write_file(
            &home,
            ".gemini/antigravity-cli/settings.json",
            "{\"theme\": \"dark\", \"modelProvider\": \"gemini\"}",
        );
        assert!(antigravity_model_provider_is_gemini(&home));
    }

    // Fix hints.
    #[test]
    fn antigravity_hints_are_non_empty_and_distinct() {
        assert!(!ANTIGRAVITY_FIX_HINT.is_empty());
        assert!(!ANTIGRAVITY_KEYRING_LOCKED_HINT.is_empty());
        assert_ne!(ANTIGRAVITY_FIX_HINT, ANTIGRAVITY_KEYRING_LOCKED_HINT);
        assert_eq!(
            antigravity_fix_hint(AntigravityAuthDetail::KeychainLocked),
            ANTIGRAVITY_KEYRING_LOCKED_HINT
        );
        assert_eq!(
            antigravity_fix_hint(AntigravityAuthDetail::Missing),
            ANTIGRAVITY_FIX_HINT
        );
        // Pass-state details still resolve to a hint (doctor advisory).
        for detail in [
            AntigravityAuthDetail::ApiKey,
            AntigravityAuthDetail::GoogleCloudCredentials,
            AntigravityAuthDetail::KeychainItem,
            AntigravityAuthDetail::KeychainDidNotAnswer,
        ] {
            assert!(!antigravity_fix_hint(detail).is_empty());
        }
    }

    #[test]
    fn antigravity_detail_display_mentions_setup() {
        assert!(
            AntigravityAuthDetail::ApiKey
                .to_string()
                .contains("GEMINI_API_KEY")
        );
        assert!(
            AntigravityAuthDetail::GoogleCloudCredentials
                .to_string()
                .contains("application default")
        );
        assert!(
            AntigravityAuthDetail::KeychainItem
                .to_string()
                .contains("keychain")
        );
        assert!(
            AntigravityAuthDetail::KeychainLocked
                .to_string()
                .contains("locked")
        );
        assert!(
            AntigravityAuthDetail::KeychainDidNotAnswer
                .to_string()
                .contains("did not answer")
        );
        assert!(
            AntigravityAuthDetail::Missing
                .to_string()
                .contains("missing")
        );
    }

    // ---- Managed Codex ----
    #[test]
    fn managed_codex_is_detected() {
        let mut codex = crate::config::schema::CodexSection::default();
        assert!(!is_managed_codex(&codex));
        codex.codex_home = Some(PathBuf::from("/app/managed/codex-home"));
        assert!(is_managed_codex(&codex));
        codex.codex_home = None;
        codex.model_token_file = Some(PathBuf::from("/app/managed/model.token"));
        assert!(is_managed_codex(&codex));
    }

    // ---- Dispatcher ----
    #[test]
    fn openclaw_and_engine_are_always_allowed() {
        assert!(has_credentials(AgentKind::OpenClaw));
        assert!(has_credentials(AgentKind::PidashAgent));
    }

    #[test]
    fn fix_hints_are_non_empty() {
        for kind in [
            AgentKind::Codex,
            AgentKind::ClaudeCode,
            AgentKind::CursorAgent,
            AgentKind::Grok,
            AgentKind::Antigravity,
            AgentKind::MuseCode,
        ] {
            assert!(!fix_hint(kind).is_empty(), "fix hint for {kind:?}");
        }
    }
}
