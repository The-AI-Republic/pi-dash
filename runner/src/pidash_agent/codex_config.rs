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

use super::api::PidashAgentProfile;
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
pub fn render_engine_config(profile: &PidashAgentProfile, helper: &AuthHelper) -> String {
    render_engine_config_inner(profile, helper, None)
}

/// Render the config with an optional `model_catalog_json` path (NEWPIDASH-247).
/// `catalog_path` is the absolute path to the file written by
/// [`write_engine_config`]. When `None`, no `model_catalog_json` key is emitted.
pub fn render_engine_config_with_catalog(
    profile: &PidashAgentProfile,
    helper: &AuthHelper,
    catalog_path: Option<&Path>,
) -> String {
    render_engine_config_inner(profile, helper, catalog_path)
}

fn render_engine_config_inner(
    profile: &PidashAgentProfile,
    helper: &AuthHelper,
    catalog_path: Option<&Path>,
) -> String {
    // Command-backed auth refreshes during a running turn and after a 401.
    // No env_key or OpenAI login: both conflict with command-backed auth.
    // JSON string/array encoding is also valid TOML here, including Windows
    // backslashes and paths containing spaces or quotes.
    let command =
        serde_json::to_string(&helper.program.to_string_lossy()).expect("serialize helper path");
    let args = serde_json::to_string(&helper.args).expect("serialize helper args");
    let catalog_line = match catalog_path {
        Some(path) => {
            let quoted =
                serde_json::to_string(&path.to_string_lossy()).expect("serialize catalog path");
            format!("model_catalog_json = {quoted}\n")
        }
        None => String::new(),
    };
    format!(
        r#"# Managed by Pi Dash. Edits are overwritten on launch.
model_provider = "pidash"
model = "{model}"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
# Model gateways are not assumed to meter provider-hosted web-search tools.
# Leave task execution tools available without advertising that hosted tool.
web_search = "disabled"
{catalog_line}
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
        catalog_line = catalog_line,
    )
}

/// Build the `model_catalog.json` body (`{"models": [...]}` Codex `ModelsResponse`)
/// for `profile`. Returns `None` when the profile carries no usable metadata —
/// an absent/empty model or a zero window must never produce a catalog that
/// would break `thread/start` (NEWPIDASH-247).
///
/// The entry mirrors the minimal catalog PDASHOSS01-174 verified live against
/// `rust-v0.153.4`: every field the catalog reader requires is present, `shell_type`
/// is `unified_exec` and `supported_reasoning_levels` is empty, so the gateway
/// (Responses-to-Chat translation) never sees a `local_shell` or reasoning
/// request it would reject with 400 (PDASHOSS01-280).
pub fn render_model_catalog_json(profile: &PidashAgentProfile) -> Option<String> {
    let window = profile.context_window.filter(|&w| w > 0)?;
    let model = profile.model.trim();
    if model.is_empty() {
        return None;
    }
    // display_name: the slug itself is the safest. The engine matches on slug
    // (longest prefix), and display_name is only shown in pickers.
    let value = serde_json::json!({
        "models": [{
            "slug": model,
            "display_name": model,
            "description": null,
            "supported_reasoning_levels": [],
            "shell_type": "unified_exec",
            "visibility": "list",
            "supported_in_api": true,
            "priority": 1,
            "upgrade": null,
            "model_messages": {"instructions_template": ""},
            "default_reasoning_summary": "auto",
            "support_verbosity": false,
            "default_verbosity": null,
            "apply_patch_tool_type": null,
            "truncation_policy": {"mode": "tokens", "limit": 10000},
            "supports_image_detail_original": false,
            "context_window": window,
            "max_context_window": window,
            "auto_compact_token_limit": null,
            "effective_context_window_percent": 95,
            "experimental_supported_tools": []
        }]
    });
    serde_json::to_string(&value).ok()
}

/// Validate a `model_catalog.json` body before it is written. A malformed file
/// makes the engine reject the whole config and `thread/start` then fails with
/// -32600 (PDASHOSS01-174), so validation is strict: it must parse as JSON,
/// have a `models` array, and every entry must carry an instructions template.
pub fn validate_model_catalog_json(json: &str) -> anyhow::Result<()> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("catalog is not valid JSON: {e}"))?;
    let models = value
        .get("models")
        .and_then(|m| m.as_array())
        .ok_or_else(|| anyhow::anyhow!("catalog missing `models` array"))?;
    if models.is_empty() {
        anyhow::bail!("catalog `models` array is empty");
    }
    for (i, entry) in models.iter().enumerate() {
        let entry = entry
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("catalog entry {i} is not an object"))?;
        let slug = entry
            .get("slug")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("catalog entry {i} missing `slug`"))?;
        // Instructions: either base_instructions or model_messages.instructions_template.
        // PDASHOSS01-174 verified that the whole config is rejected when neither
        // is present, so this is a hard validation error, not a warning.
        let has_base = entry
            .get("base_instructions")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let has_template = entry
            .get("model_messages")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("instructions_template"))
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        // Note: empty string template ("") is the verified minimal entry from
        // PDASHOSS01-174 and is NOT treated as missing — the key's presence is
        // what the engine checks. So presence, not non-emptiness beyond the
        // key existing, is the rule; empty string counts as present.
        // Re-check with presence-only semantics:
        let has_template_key = entry
            .get("model_messages")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("instructions_template"))
            .is_some_and(|v| v.is_string());
        let has_base_key = entry
            .get("base_instructions")
            .is_some_and(|v| v.is_string());
        if !has_template_key && !has_base_key {
            anyhow::bail!(
                "catalog entry {i} (slug {slug}) missing instructions template (needs `model_messages.instructions_template` or `base_instructions`)"
            );
        }
        let _ = (has_base, has_template);
        // Context window must be a positive integer, and max must match (the
        // engine reports `effective_window = context_window * 95%`, but a
        // mismatched max would let the fallback clamp hide the real window).
        let ctx = entry
            .get("context_window")
            .and_then(|v| v.as_u64())
            .filter(|&v| v > 0)
            .ok_or_else(|| {
                anyhow::anyhow!("catalog entry {i} (slug {slug}) missing positive `context_window`")
            })?;
        if let Some(max) = entry.get("max_context_window").and_then(|v| v.as_u64())
            && max != ctx
        {
            anyhow::bail!(
                "catalog entry {i} (slug {slug}) `max_context_window` {max} != `context_window` {ctx}"
            );
        }
        // Reasoning / local_shell must stay off: the gateway rejects both with 400
        // in translation mode (PDASHOSS01-280), so a catalog that enables either
        // would break turns that work today on fallback metadata.
        if let Some(levels) = entry
            .get("supported_reasoning_levels")
            .and_then(|v| v.as_array())
            && !levels.is_empty()
        {
            anyhow::bail!(
                "catalog entry {i} (slug {slug}) must not advertise reasoning levels (gateway 400s in translation mode)"
            );
        }
        if let Some(shell) = entry.get("shell_type").and_then(|v| v.as_str())
            && shell == "local_shell"
        {
            anyhow::bail!(
                "catalog entry {i} (slug {slug}) must not use `local_shell` (gateway 400s in translation mode)"
            );
        }
    }
    Ok(())
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

/// Write `CODEX_HOME/config.toml` for an available profile, plus the
/// `model_catalog.json` it points at when the profile carries real limits
/// (NEWPIDASH-247). Refuses an unavailable profile rather than writing a
/// config that cannot run.
///
/// The catalog is written first and only when it validates: it is read only
/// at engine startup, but a malformed file makes the engine reject the whole
/// config and `thread/start` fails with -32600, so an invalid catalog is
/// never written — the config then simply has no `model_catalog_json` key and
/// the engine falls back (which the bridge refuses at turn time).
pub fn write_engine_config(
    codex_home: &Path,
    profile: &PidashAgentProfile,
    helper: &AuthHelper,
) -> anyhow::Result<PathBuf> {
    if !profile.available {
        anyhow::bail!(
            "profile unavailable ({}); refusing to write an engine config that cannot run",
            profile.reason_code
        );
    }
    let catalog_path = codex_home.join("model_catalog.json");
    let validated_catalog = render_model_catalog_json(profile).and_then(|json| {
        match validate_model_catalog_json(&json) {
            Ok(()) => Some(json),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    model = %profile.model,
                    "refusing to write invalid model catalog; continuing without model_catalog_json"
                );
                None
            }
        }
    });
    let config_catalog_path = if let Some(json) = validated_catalog {
        // Write the catalog before the config that points at it. If the write
        // fails, fall back to no catalog rather than pointing at a missing file.
        match atomic_write(&catalog_path, json.as_bytes()) {
            Ok(()) => Some(catalog_path.clone()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %catalog_path.display(),
                    "failed to write model catalog; continuing without model_catalog_json"
                );
                None
            }
        }
    } else {
        // No usable limits: remove a stale catalog so it cannot be picked up
        // by a future config that forgets to clear the key (best-effort).
        let _ = std::fs::remove_file(&catalog_path);
        None
    };
    let path = codex_home.join("config.toml");
    let text = render_engine_config_inner(profile, helper, config_catalog_path.as_deref());
    atomic_write(&path, text.as_bytes())?;
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

    fn profile() -> PidashAgentProfile {
        PidashAgentProfile {
            available: true,
            lane: "openhub".into(),
            base_url: "https://gateway.example/v1".into(),
            model: "gpt-5.5".into(),
            reason_code: String::new(),
            engine_sign_in_state: None,
            context_window: None,
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

    #[test]
    fn catalog_is_written_and_validated_for_a_real_window() {
        let mut p = profile();
        p.model = "deepseek/deepseek-v4-flash".into();
        p.context_window = Some(1_048_576);
        let json = render_model_catalog_json(&p).expect("catalog for real window");
        validate_model_catalog_json(&json).expect("generated catalog validates");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let entry = &value["models"][0];
        assert_eq!(entry["slug"], "deepseek/deepseek-v4-flash");
        assert_eq!(entry["context_window"], 1_048_576);
        assert_eq!(entry["max_context_window"], 1_048_576);
        assert_eq!(entry["effective_context_window_percent"], 95);
        assert_eq!(entry["shell_type"], "unified_exec");
        assert!(
            entry["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(entry["model_messages"]["instructions_template"].is_string());
        // Effective window at 95% matches the issue's 996,147.
        assert_eq!(
            entry["context_window"].as_u64().unwrap() * 95 / 100,
            996_147
        );
    }

    #[test]
    fn empty_profile_produces_no_catalog_file() {
        let mut p = profile();
        p.context_window = None;
        assert!(render_model_catalog_json(&p).is_none());
        let tmp = tempfile::tempdir().unwrap();
        let helper = AuthHelper::for_paths(PathBuf::from("pidash"), &paths(tmp.path()));
        let config_path = write_engine_config(tmp.path(), &p, &helper).unwrap();
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(!text.contains("model_catalog_json"), "{text}");
        assert!(!tmp.path().join("model_catalog.json").exists());
        // Parsed config is valid TOML without the key.
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert!(parsed.get("model_catalog_json").is_none());
    }

    #[test]
    fn zero_window_produces_no_catalog() {
        let mut p = profile();
        p.context_window = Some(0);
        assert!(render_model_catalog_json(&p).is_none());
        let mut p2 = profile();
        p2.model = String::new();
        p2.context_window = Some(1_048_576);
        assert!(render_model_catalog_json(&p2).is_none());
    }

    #[test]
    fn write_engine_config_writes_catalog_and_points_at_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = profile();
        p.model = "deepseek/deepseek-v4-flash".into();
        p.context_window = Some(1_048_576);
        let helper = AuthHelper::for_paths(PathBuf::from("pidash"), &paths(tmp.path()));
        let config_path = write_engine_config(tmp.path(), &p, &helper).unwrap();
        let catalog_path = tmp.path().join("model_catalog.json");
        assert!(catalog_path.is_file(), "catalog written next to config");
        let catalog_text = std::fs::read_to_string(&catalog_path).unwrap();
        validate_model_catalog_json(&catalog_text).unwrap();
        let config_text = std::fs::read_to_string(&config_path).unwrap();
        let parsed: toml::Value = toml::from_str(&config_text).unwrap();
        let cat = parsed["model_catalog_json"]
            .as_str()
            .expect("catalog key present");
        assert_eq!(cat, catalog_path.to_string_lossy());
        // Catalog exists before config was parsed: both are on disk.
        assert!(std::path::Path::new(cat).is_file());
    }

    #[test]
    fn validation_rejects_missing_instructions_template() {
        let bad = serde_json::json!({
            "models": [{
                "slug": "deepseek/deepseek-v4-flash",
                "context_window": 1048576,
                "max_context_window": 1048576,
                "shell_type": "unified_exec",
                "supported_reasoning_levels": []
            }]
        });
        let err = validate_model_catalog_json(&bad.to_string()).unwrap_err();
        assert!(err.to_string().contains("instructions template"), "{err}");
    }

    #[test]
    fn validation_rejects_reasoning_and_local_shell() {
        let with_reasoning = serde_json::json!({
            "models": [{
                "slug": "m",
                "context_window": 1000,
                "max_context_window": 1000,
                "model_messages": {"instructions_template": ""},
                "supported_reasoning_levels": ["high"],
                "shell_type": "unified_exec"
            }]
        });
        assert!(validate_model_catalog_json(&with_reasoning.to_string()).is_err());
        let with_shell = serde_json::json!({
            "models": [{
                "slug": "m",
                "context_window": 1000,
                "max_context_window": 1000,
                "model_messages": {"instructions_template": ""},
                "supported_reasoning_levels": [],
                "shell_type": "local_shell"
            }]
        });
        assert!(validate_model_catalog_json(&with_shell.to_string()).is_err());
    }

    #[test]
    fn empty_profile_still_writes_valid_config_without_catalog() {
        let mut p = profile();
        p.model = "gpt-5.5".into();
        p.context_window = None;
        let helper = AuthHelper {
            program: PathBuf::from("pidash"),
            args: vec!["__engine".into(), "model-token".into()],
        };
        let text = render_engine_config_with_catalog(&p, &helper, None);
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert!(parsed.get("model_catalog_json").is_none());
        assert_eq!(parsed["model"].as_str(), Some("gpt-5.5"));
    }
}
