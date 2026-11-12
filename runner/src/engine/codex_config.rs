//! The engine's own Codex configuration, written by the daemon (design §5).
//!
//! The engine runs with a private `CODEX_HOME` —
//! `<daemon data>/engine/<account>/codex-home`, never `~/.codex` — whose
//! `config.toml` points Codex at the Pi Dash model gateway from the profile
//! (`GET /api/v1/runner/engine/profile/`). Ported from the desktop app's
//! `render_engine_config` (`desktop/src-tauri/src/managed_runner.rs`); the
//! file is the same except for the command-backed auth helper, which is now
//! `pidash __engine model-token`: it asks the daemon over the local socket
//! for the model token the daemon holds in memory, so no short-lived secret
//! is written to disk.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::api::EngineProfile;
use crate::util::paths::Paths;

/// Directory holding everything the engine keeps per account.
pub fn engine_dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join("engine")
}

/// `<daemon data>/engine/<account>/codex-home`. `account` is the signed-in
/// user (id, else e-mail); an install that never recorded one uses
/// `default`.
pub fn codex_home(paths: &Paths, account: Option<&str>) -> PathBuf {
    engine_dir(paths)
        .join(account_dir_name(account))
        .join("codex-home")
}

/// A safe single path component for `account`.
pub fn account_dir_name(account: Option<&str>) -> String {
    let raw = account.map(str::trim).filter(|s| !s.is_empty());
    let Some(raw) = raw else {
        return "default".to_string();
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

/// The auth helper the engine runs: `pidash [--config-dir D --data-dir D]
/// __engine model-token`. The directories are passed explicitly so the
/// helper reaches this daemon's socket whatever environment the engine
/// gives it.
pub struct AuthHelper {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl AuthHelper {
    pub fn for_paths(program: PathBuf, paths: &Paths) -> Self {
        Self {
            program,
            args: vec![
                "--config-dir".to_string(),
                paths.config_dir.to_string_lossy().into_owned(),
                "--data-dir".to_string(),
                paths.data_dir.to_string_lossy().into_owned(),
                "__engine".to_string(),
                "model-token".to_string(),
            ],
        }
    }
}

/// Render the engine's `CODEX_HOME/config.toml` from the profile.
pub fn render_engine_config(profile: &EngineProfile, helper: &AuthHelper) -> String {
    // Command-backed auth refreshes during a running turn and after a 401.
    // No env_key or OpenAI login: both conflict with command-backed auth.
    // JSON string/array encoding is also valid TOML here, including Windows
    // backslashes and paths containing spaces or quotes.
    let command =
        serde_json::to_string(&helper.program.to_string_lossy()).expect("serialize helper path");
    let args = serde_json::to_string(&helper.args).expect("serialize helper args");
    format!(
        r#"# Managed by Pi Dash. Edits are overwritten on launch.
model_provider = "pidash"
model = "{model}"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
# Model gateways are not assumed to meter provider-hosted web-search tools.
# Leave task execution tools available without advertising that hosted tool.
web_search = "disabled"

[model_providers.pidash]
name = "Pi Dash"
base_url = "{base_url}"
wire_api = "responses"
[model_providers.pidash.auth]
command = {command}
args = {args}
timeout_ms = 5000
refresh_interval_ms = 30000
"#,
        model = toml_value(&profile.model),
        base_url = toml_value(&profile.base_url),
    )
}

/// Sanitise a server-supplied string for a TOML basic-string value.
///
/// These values come from our own API, so this is defence in depth rather
/// than a threat model — but the file it lands in controls the engine's
/// sandbox policy and credential source, so a value that could close its
/// quote and open a new key is worth one line to prevent. Quotes, backslashes
/// and anything that could start a new line are dropped rather than escaped:
/// no legitimate model name or URL contains them, so silently losing one is
/// preferable to emitting a file whose meaning depends on escaping rules.
fn toml_value(raw: &str) -> String {
    raw.chars()
        .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
        .take(512)
        .collect()
}

/// Write `CODEX_HOME/config.toml` for an available profile. Refuses an
/// unavailable one rather than writing a config that cannot run.
pub fn write_engine_config(
    codex_home: &Path,
    profile: &EngineProfile,
    helper: &AuthHelper,
) -> anyhow::Result<PathBuf> {
    if !profile.available {
        anyhow::bail!(
            "profile unavailable ({}); refusing to write an engine config that cannot run",
            profile.reason_code
        );
    }
    let path = codex_home.join("config.toml");
    atomic_write(&path, render_engine_config(profile, helper).as_bytes())?;
    Ok(path)
}

/// Write via a temp file and rename, so a reader never sees a half-written
/// config. 0600 on Unix.
fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(bytes)?;
        file.sync_all().ok();
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> EngineProfile {
        EngineProfile {
            available: true,
            lane: "openhub".into(),
            base_url: "https://gateway.example/v1".into(),
            model: "gpt-5.5".into(),
            reason_code: String::new(),
            engine_sign_in_state: None,
        }
    }

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    #[test]
    fn renders_the_desktop_config_with_the_daemon_auth_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        let helper = AuthHelper::for_paths(PathBuf::from("/opt/pidash/bin/pidash"), &paths);
        let text = render_engine_config(&profile(), &helper);
        let parsed: toml::Value = toml::from_str(&text).expect("valid TOML");
        assert_eq!(parsed["model_provider"].as_str(), Some("pidash"));
        assert_eq!(parsed["model"].as_str(), Some("gpt-5.5"));
        assert_eq!(parsed["approval_policy"].as_str(), Some("on-request"));
        assert_eq!(parsed["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(parsed["web_search"].as_str(), Some("disabled"));
        let provider = &parsed["model_providers"]["pidash"];
        assert_eq!(
            provider["base_url"].as_str(),
            Some("https://gateway.example/v1")
        );
        assert_eq!(provider["wire_api"].as_str(), Some("responses"));
        let auth = &provider["auth"];
        assert_eq!(auth["command"].as_str(), Some("/opt/pidash/bin/pidash"));
        let args: Vec<&str> = auth["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(&args[args.len() - 2..], ["__engine", "model-token"]);
        assert!(args.contains(&paths.data_dir.to_str().unwrap()));
        assert!(!text.contains("--file"), "no token file any more: {text}");
        assert_eq!(auth["timeout_ms"].as_integer(), Some(5000));
        assert_eq!(auth["refresh_interval_ms"].as_integer(), Some(30000));
    }

    #[test]
    fn server_values_cannot_inject_keys() {
        let mut p = profile();
        p.model = "gpt\"\nsandbox_mode = \"danger-full-access".into();
        let helper = AuthHelper {
            program: PathBuf::from(r"C:\Program Files\pidash\pidash.exe"),
            args: vec!["__engine".into(), "model-token".into()],
        };
        let text = render_engine_config(&p, &helper);
        let parsed: toml::Value = toml::from_str(&text).expect("valid TOML");
        assert_eq!(parsed["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(
            parsed["model_providers"]["pidash"]["auth"]["command"].as_str(),
            Some(r"C:\Program Files\pidash\pidash.exe")
        );
    }

    #[test]
    fn codex_home_is_per_account_under_the_daemon_data_dir_never_dot_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        let home = codex_home(&paths, Some("Dev@Example.com"));
        assert_eq!(
            home,
            paths
                .data_dir
                .join("engine")
                .join("dev@example.com")
                .join("codex-home")
        );
        assert_eq!(
            codex_home(&paths, None),
            paths.data_dir.join("engine/default/codex-home")
        );
        assert_eq!(account_dir_name(Some("../../etc")), "_.._etc");
        assert!(!home.to_string_lossy().contains(".codex"));
    }

    #[test]
    fn an_unavailable_profile_is_not_written() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = profile();
        p.available = false;
        p.reason_code = "profile_unavailable".into();
        let helper = AuthHelper::for_paths(PathBuf::from("pidash"), &paths(tmp.path()));
        assert!(write_engine_config(tmp.path(), &p, &helper).is_err());
        assert!(!tmp.path().join("config.toml").exists());
        p.available = true;
        let written = write_engine_config(tmp.path(), &p, &helper).unwrap();
        assert!(written.is_file());
    }
}
