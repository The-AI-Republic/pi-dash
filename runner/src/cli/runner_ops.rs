//! Shared local-side plumbing for both `pidash connect` (legacy
//! enrollment-token flow) and `pidash runner add` (CLI-token flow).
//!
//! After a successful cloud-side mint (`EnrollResponse`), both paths need
//! to write `[[runner]]` to `config.toml` and install the OS service unit
//! on first add. Legacy `pidash connect` responses also write per-runner
//! `credentials.toml`; new `pidash runner add` responses rely on the
//! shared dev-machine token in `[cli].token`.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::cloud::http::{EnrollResponse, RunnerCredentials, write_runner_credentials};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CursorAgentSection, CliSection, CodexSection,
    Config, DaemonConfig, RunnerConfig, WorkspaceSection,
};
use crate::util::paths::Paths;
use std::io::IsTerminal;
use uuid::Uuid;

/// Codex reasoning-effort tiers accepted by `--reasoning-effort` / the
/// cloud dropdown. Passed verbatim to codex `turn/start`; the exact set a
/// given codex build honours can vary, so an unrecognized value here is a
/// soft warning, not a hard error.
const CODEX_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];

/// Print an orange (256-color 208) warning to stderr. Used when a
/// `--model` / `--reasoning-effort` value can't be applied to the chosen
/// agent: we never fail the enrollment over it, just tell the operator we
/// fell back to the agent's default. Color is suppressed when stderr is
/// not a TTY (logs / CI) and when `NO_COLOR` is set.
fn warn_model_fallback(msg: &str) {
    let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    if color {
        eprintln!("\x1b[38;5;208m⚠ {msg}\x1b[0m");
    } else {
        eprintln!("⚠ {msg}");
    }
}

/// Whether a model id is meaningful for the given agent. The cloud "add
/// runner" dropdown only offers each agent's own models, so this guards
/// the hand-typed-CLI case (`--agent claude-code --model gpt-5.5`).
///
/// - **Claude Code** drives Anthropic models only (`claude-…`).
/// - **Codex** drives OpenAI models (`gpt-…`, `o3`/`o4`, `codex…`).
/// - **Cursor** has a broad, account-specific slug space (`claude-*`,
///   `gpt-*`, `gemini-*`, `grok-*`, `composer-*`, `auto`, `kimi-*`, …),
///   so we accept any non-empty value and let `cursor-agent` reject
///   unknown slugs itself rather than wrongly dropping a valid one.
///
/// A bare dash-terminated prefix (`"claude-"`, `"gpt-"`) is rejected — it
/// is an incomplete slug, not a model — so it can't be written as a bogus
/// `model_default`.
fn model_applies_to_agent(kind: AgentKind, model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    // `prefix` followed by at least one more character.
    let has = |prefix: &str| m.strip_prefix(prefix).is_some_and(|rest| !rest.is_empty());
    match kind {
        AgentKind::ClaudeCode => has("claude-"),
        // `o3` / `o4` are valid bare model names; the dash-terminated
        // families (`gpt-`, `codex`) require a suffix.
        AgentKind::Codex => has("gpt-") || m == "o3" || m == "o4" || has("o3-") || has("o4-") || has("codex"),
        AgentKind::CursorAgent => !m.is_empty(),
    }
}

/// Build the three per-agent config sections for a fresh `[[runner]]`,
/// applying the operator's `--model` / `--reasoning-effort` to whichever
/// agent was selected. Inapplicable combinations are non-fatal: an orange
/// warning is printed and that knob falls back to the agent default (left
/// unset in the config). Shared by `pidash runner add` and the deprecated
/// `pidash connect`.
pub fn agent_sections_for(
    agent_kind: AgentKind,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> (CodexSection, ClaudeCodeSection, CursorAgentSection) {
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    let effort = reasoning_effort.map(str::trim).filter(|s| !s.is_empty());

    // Resolve the model first: drop it (with a warning) when it isn't
    // applicable to the chosen agent. `effort` below is keyed off the
    // resolved model, so this has to run first.
    let model = match model {
        None => None,
        Some(m) if model_applies_to_agent(agent_kind, m) => Some(m.to_string()),
        Some(m) => {
            warn_model_fallback(&format!(
                "model {m:?} is not applicable to the {} agent; \
                 using the agent's default model instead.",
                agent_kind.display_name()
            ));
            None
        }
    };

    // Reasoning effort only applies to Codex, and only alongside an explicit
    // model — it is meaningless on its own (see `CodexSection::effort_default`),
    // so it is dropped when the model was absent or rejected above.
    let effort = match (agent_kind, effort) {
        (_, None) => None,
        (AgentKind::Codex, Some(e))
            if !CODEX_EFFORTS.contains(&e.to_ascii_lowercase().as_str()) =>
        {
            warn_model_fallback(&format!(
                "reasoning effort {e:?} is not a recognized Codex tier \
                 (expected one of {}); using the model's default effort.",
                CODEX_EFFORTS.join(", ")
            ));
            None
        }
        (AgentKind::Codex, Some(e)) if model.is_some() => Some(e.to_ascii_lowercase()),
        (AgentKind::Codex, Some(_)) => {
            // Valid tier, but no applicable model to attach it to.
            warn_model_fallback(
                "--reasoning-effort needs an applicable --model for the codex agent; ignoring it.",
            );
            None
        }
        (_, Some(_)) => {
            warn_model_fallback(&format!(
                "--reasoning-effort only applies to the codex agent, not {}; ignoring it.",
                agent_kind.display_name()
            ));
            None
        }
    };

    let mut codex = CodexSection::default();
    let mut claude_code = ClaudeCodeSection::default();
    let mut cursor_agent = CursorAgentSection::default();
    match agent_kind {
        AgentKind::Codex => {
            codex.model_default = model;
            codex.effort_default = effort;
        }
        AgentKind::ClaudeCode => claude_code.model_default = model,
        AgentKind::CursorAgent => cursor_agent.model_default = model,
    }
    (codex, claude_code, cursor_agent)
}

/// Read the user's CLI token from `[cli].token` in `config.toml`.
/// Returns `Ok(None)` when no config exists yet or the section is
/// absent; the caller is expected to surface a friendly "run `pidash
/// auth login` first" message in that case.
pub fn load_cli_token(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.token))
}

/// Write `[cli].token` into `config.toml`, creating the file if it
/// doesn't exist yet. Used by `pidash auth login` after the device-code
/// exchange returns an `APIToken`.
///
/// First-run bootstrap: when no config exists, we seed a minimal
/// `[daemon]` block with the cloud URL the login was performed against.
/// No `[[runner]]` blocks are added — the runner is registered
/// separately via `pidash runner add`.
///
/// Preserves any pre-existing `[cli].workspace_slug` so a re-login
/// (e.g. token refresh) doesn't wipe the host's workspace binding.
pub fn write_cli_token(paths: &Paths, cloud_url: &str, token: &str) -> Result<()> {
    let mut cfg = if paths.config_path().exists() {
        file::load_config(paths)?
    } else {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: cloud_url.to_string(),
                dev_machine_id: None,
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners: vec![],
            cli: None,
        }
    };
    // Pre-existing config? Don't quietly rebind it to a different cloud.
    if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
        anyhow::bail!(
            "this host is already enrolled with cloud {} — refusing to overwrite [cli] for a different cloud {}",
            cfg.daemon.cloud_url,
            cloud_url
        );
    }
    if cfg.daemon.cloud_url.is_empty() {
        cfg.daemon.cloud_url = cloud_url.to_string();
    }
    let preserved_workspace = cfg.cli.as_ref().and_then(|c| c.workspace_slug.clone());
    let preserved_default_project = cfg.cli.as_ref().and_then(|c| c.default_project.clone());
    cfg.cli = Some(CliSection {
        token: Some(token.to_string()),
        workspace_slug: preserved_workspace,
        default_project: preserved_default_project,
    });
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Return the stable cloud identity for this dev machine, minting and
/// persisting it once when an older config does not have one yet.
pub fn ensure_dev_machine_id(paths: &Paths) -> Result<Uuid> {
    let cfg = file::mutate_config(paths, |cfg| {
        if cfg.daemon.dev_machine_id.is_none() {
            cfg.daemon.dev_machine_id = Some(Uuid::new_v4());
        }
        Ok(())
    })?;
    cfg.daemon
        .dev_machine_id
        .ok_or_else(|| anyhow::anyhow!("dev_machine_id was not persisted to config.toml"))
}

/// Read `[cli].workspace_slug` from `config.toml`.
///
/// Returns `Ok(None)` if no config exists, the `[cli]` section is
/// absent, or `workspace_slug` was never set. `pidash runner add` uses
/// this as the default workspace when the caller omits `--workspace`.
pub fn load_cli_workspace(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.workspace_slug))
}

/// Write `[cli].workspace_slug` into `config.toml`. Called by
/// `pidash auth login` after the workspace picker resolves which
/// workspace this host is bound to. Requires a pre-existing config
/// (the token write always happens first).
///
/// Rebinding to a different workspace is allowed — the login flow
/// itself decides whether to keep the existing binding (slug still in
/// the user's memberships) or pick a new one. Callers that need a
/// safety check should compare before calling.
pub fn write_cli_workspace(paths: &Paths, workspace_slug: &str) -> Result<()> {
    if !paths.config_path().exists() {
        anyhow::bail!(
            "no config.toml — `pidash auth login` must write the CLI token before the workspace binding"
        );
    }
    let mut cfg = file::load_config(paths)?;
    let mut cli = cfg.cli.unwrap_or_default();
    cli.workspace_slug = Some(workspace_slug.to_string());
    cfg.cli = Some(cli);
    file::write_config(paths, &cfg)?;
    Ok(())
}

pub fn load_cli_default_project(paths: &Paths) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg.cli.and_then(|c| c.default_project))
}

pub fn write_cli_default_project(paths: &Paths, project: &str) -> Result<()> {
    if !paths.config_path().exists() {
        anyhow::bail!("no config.toml — run `pidash auth login` before setting a default project");
    }
    let mut cfg = file::load_config(paths)?;
    let mut cli = cfg.cli.unwrap_or_default();
    cli.default_project = Some(project.to_string());
    cfg.cli = Some(cli);
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Clear `[cli].token` from `config.toml`. Used by `pidash auth logout`.
/// Leaves `[[runner]]` blocks untouched so the daemon keeps running
/// under its own identity. No-op if no config exists.
pub fn clear_cli_token(paths: &Paths) -> Result<()> {
    if !paths.config_path().exists() {
        return Ok(());
    }
    let mut cfg = file::load_config(paths)?;
    if let Some(ref mut cli) = cfg.cli {
        cli.token = None;
    }
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Result of applying a mint locally: the new `RunnerConfig` block and
/// whether this was the host's first runner (so callers can decide
/// whether to install the OS service unit / restart it).
#[derive(Debug)]
pub struct AppliedRunner {
    pub runner: RunnerConfig,
    pub is_first_runner: bool,
}

/// Apply an `EnrollResponse` (from either the legacy enroll endpoint or
/// the new CLI-initiated create endpoint) to local disk:
///
/// 1. Append a new `[[runner]]` block to `config.toml`.
/// 2. Write legacy per-runner refresh credentials only when the cloud
///    returns them.
///
/// The caller is responsible for:
/// - Validating the cloud URL up front.
/// - Restarting the service afterwards (subsequent runners) or installing
///   it (first runner) — see `is_first_runner` on the return.
pub async fn apply_enroll_response(
    paths: &Paths,
    resp: &EnrollResponse,
    cloud_url: &str,
    working_dir: Option<PathBuf>,
    agent_kind: AgentKind,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<AppliedRunner> {
    let working_dir =
        working_dir.unwrap_or_else(|| paths.runner_dir(resp.runner_id).join("workspace"));

    let (codex, claude_code, cursor_agent) =
        agent_sections_for(agent_kind, model, reasoning_effort);
    let new_runner = RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: WorkspaceSection { working_dir },
        agent: AgentSection { kind: agent_kind },
        codex,
        claude_code,
        cursor_agent,
        approval_policy: ApprovalPolicySection::default(),
    };

    // Bootstrap closure for the no-config-yet case (first runner on a
    // fresh host that ran `pidash auth login` but not `pidash connect`).
    // Seeds a minimal `[daemon]` block from the response's cloud URL.
    let bootstrap_cloud_url = cloud_url.to_string();
    let init_cfg = move || {
        Some(Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: bootstrap_cloud_url.clone(),
                dev_machine_id: None,
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners: vec![],
            cli: None,
        })
    };

    // Persist the `[[runner]]` block under the host-wide `.config.lock`
    // so a concurrent `pidash runner add` / daemon `RemoveRunner` strip
    // can't last-writer-wins this write. The closure also performs the
    // cloud-URL mismatch check inside the lock — doing it before
    // `mutate_config_or_init` would race with a concurrent `auth login
    // --url <different>` that may be rewriting `[cli]` at the same time.
    let new_runner_for_closure = new_runner.clone();
    let mut is_first_runner = false;
    file::mutate_config_or_init(paths, init_cfg, |cfg| {
        if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
            anyhow::bail!(
                "this host is enrolled with cloud {} — refusing to add a runner pointing at {}",
                cfg.daemon.cloud_url,
                cloud_url
            );
        }
        if cfg.daemon.cloud_url.is_empty() {
            cfg.daemon.cloud_url = cloud_url.to_string();
        }
        is_first_runner = cfg.runners.is_empty();
        cfg.runners.push(new_runner_for_closure);
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("persisting [[runner]] block under config lock: {e}"))?;

    // Credentials write happens AFTER config so a config-write failure
    // (cloud-URL mismatch, validation error, IO) doesn't leave an orphan
    // per-runner credentials file pointing at a runner the supervisor
    // never sees. The flipside — config block exists but credentials
    // don't — is recoverable: the supervisor logs the missing-file
    // error and `pidash runner remove <name>` can clean up by name.
    // (The caller's rollback umbrella in cli/runner.rs also strips the
    // partial `[[runner]]` block on credentials-write failure, see the
    // `Err(persist_err)` arm there.)
    let runner_paths = paths.for_runner(resp.runner_id);
    runner_paths.ensure()?;
    if let Some(machine_token) = resp
        .machine_token
        .as_deref()
        .filter(|t| !t.trim().is_empty())
    {
        write_cli_token(paths, cloud_url, machine_token)
            .context("writing shared dev-machine token to [cli].token")?;
    }
    if resp.refresh_token.trim().is_empty() {
        let has_machine_token = load_cli_token(paths)?
            .as_deref()
            .map(|token| token.starts_with("mt_"))
            .unwrap_or(false);
        if !has_machine_token {
            anyhow::bail!(
                "cloud did not return per-runner credentials and no dev-machine token is configured"
            );
        }
        return Ok(AppliedRunner {
            runner: new_runner,
            is_first_runner,
        });
    }
    write_runner_credentials(
        runner_paths.credentials_path(),
        RunnerCredentials {
            runner_id: resp.runner_id,
            name: resp.runner_name.clone(),
            refresh_token: resp.refresh_token.clone(),
            refresh_token_generation: resp.refresh_token_generation,
        },
    )
    .await
    .context("writing per-runner credentials failed")?;

    Ok(AppliedRunner {
        runner: new_runner,
        is_first_runner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::http::EnrollResponse;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn model_routes_to_selected_agent_section() {
        let (codex, claude, cursor) =
            agent_sections_for(AgentKind::ClaudeCode, Some("claude-opus-4-8"), None);
        assert_eq!(claude.model_default.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(codex.model_default, None);
        assert_eq!(cursor.model_default, None);
    }

    #[test]
    fn codex_model_and_effort_are_applied_together() {
        let (codex, _, _) = agent_sections_for(AgentKind::Codex, Some("gpt-5.5"), Some("High"));
        assert_eq!(codex.model_default.as_deref(), Some("gpt-5.5"));
        // Effort is normalized to lowercase.
        assert_eq!(codex.effort_default.as_deref(), Some("high"));
    }

    #[test]
    fn mismatched_model_falls_back_to_agent_default() {
        // The user's example: a Codex model handed to the Claude agent.
        // Non-fatal — the model is dropped (warning printed) and the
        // section is left at its default (None).
        let (_, claude, _) = agent_sections_for(AgentKind::ClaudeCode, Some("gpt-5.5"), None);
        assert_eq!(claude.model_default, None);
    }

    #[test]
    fn effort_ignored_for_non_codex_agents() {
        let (_, claude, _) =
            agent_sections_for(AgentKind::ClaudeCode, Some("claude-opus-4-8"), Some("high"));
        // Model still applies; effort has no home on the claude section.
        assert_eq!(claude.model_default.as_deref(), Some("claude-opus-4-8"));
    }

    #[test]
    fn unknown_codex_effort_is_dropped() {
        let (codex, _, _) =
            agent_sections_for(AgentKind::Codex, Some("gpt-5.5"), Some("turbo"));
        assert_eq!(codex.model_default.as_deref(), Some("gpt-5.5"));
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn cursor_accepts_any_nonempty_slug() {
        let (_, _, cursor) =
            agent_sections_for(AgentKind::CursorAgent, Some("claude-opus-4-8-thinking-high"), None);
        assert_eq!(
            cursor.model_default.as_deref(),
            Some("claude-opus-4-8-thinking-high")
        );
    }

    #[test]
    fn blank_model_is_treated_as_unset() {
        let (codex, _, _) = agent_sections_for(AgentKind::Codex, Some("   "), None);
        assert_eq!(codex.model_default, None);
    }

    #[test]
    fn codex_effort_dropped_when_model_inapplicable() {
        // A Claude model handed to codex: the model is dropped, and the
        // effort meant for it must not survive onto codex's default model.
        let (codex, _, _) =
            agent_sections_for(AgentKind::Codex, Some("claude-opus-4-8"), Some("high"));
        assert_eq!(codex.model_default, None);
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn codex_effort_dropped_without_a_model() {
        // Effort is meaningless without an explicit model (see the
        // CodexSection::effort_default contract).
        let (codex, _, _) = agent_sections_for(AgentKind::Codex, None, Some("high"));
        assert_eq!(codex.model_default, None);
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn bare_prefix_models_are_rejected() {
        // A dash-terminated prefix with nothing after it is an incomplete
        // slug, not a model — it must not become a bogus model_default.
        let (_, claude, _) = agent_sections_for(AgentKind::ClaudeCode, Some("claude-"), None);
        assert_eq!(claude.model_default, None);
        let (codex, _, _) = agent_sections_for(AgentKind::Codex, Some("gpt-"), None);
        assert_eq!(codex.model_default, None);
    }

    #[test]
    fn bare_openai_reasoning_models_are_accepted() {
        // `o3` / `o4` are valid bare model names; the bare-prefix guard
        // must not reject them.
        let (codex, _, _) = agent_sections_for(AgentKind::Codex, Some("o3"), None);
        assert_eq!(codex.model_default.as_deref(), Some("o3"));
    }

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn sample_response(runner_name: &str) -> EnrollResponse {
        EnrollResponse {
            runner_id: Uuid::new_v4(),
            runner_name: runner_name.into(),
            refresh_token: "refresh".into(),
            access_token: "access".into(),
            access_token_expires_at: "2099-01-01T00:00:00+00:00".into(),
            refresh_token_generation: 1,
            workspace_slug: "acme".into(),
            pod_slug: "default".into(),
            project_identifier: "WEB".into(),
            long_poll_interval_secs: 25,
            protocol_version: 4,
            machine_token: None,
            machine_token_minted: false,
        }
    }

    #[tokio::test]
    async fn first_runner_bootstraps_config_when_none_exists() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let resp = sample_response("r1");
        let applied = apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            Some(tmp.path().join("wd")),
            AgentKind::Codex,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(applied.is_first_runner);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.daemon.cloud_url, "https://example.com");
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(cfg.runners[0].name, "r1");
    }

    #[tokio::test]
    async fn second_runner_appends_and_flags_not_first() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r1 = sample_response("r1");
        apply_enroll_response(
            &paths,
            &r1,
            "https://example.com",
            Some(tmp.path().join("wd1")),
            AgentKind::Codex,
            None,
            None,
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let applied = apply_enroll_response(
            &paths,
            &r2,
            "https://example.com",
            Some(tmp.path().join("wd2")),
            AgentKind::Codex,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!applied.is_first_runner);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 2);
    }

    #[tokio::test]
    async fn cloud_url_mismatch_does_not_leave_orphan_credentials() {
        // The whole point of the URL-check-before-write reorder. If the
        // user is enrolled with cloud A and a misuse points us at cloud
        // B, we MUST refuse before writing per-runner credentials —
        // otherwise an orphan file is left for the operator to clean up.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let r1 = sample_response("r1");
        apply_enroll_response(
            &paths,
            &r1,
            "https://cloud-a.example.com",
            Some(tmp.path().join("wd1")),
            AgentKind::Codex,
            None,
            None,
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let err = apply_enroll_response(
            &paths,
            &r2,
            "https://cloud-b.example.com",
            Some(tmp.path().join("wd2")),
            AgentKind::Codex,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(format!("{err}").contains("refusing to add a runner pointing at"));
        // No credentials file should exist for r2 — the bail happened
        // before the write.
        let r2_creds = paths.for_runner(r2.runner_id).credentials_path();
        assert!(
            !r2_creds.exists(),
            "orphan credentials file at {r2_creds:?}"
        );
    }

    #[test]
    fn write_then_load_cli_token_roundtrips() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        let token = load_cli_token(&paths).unwrap();
        assert_eq!(token.as_deref(), Some("pi_dash_api_xxx"));
    }

    #[test]
    fn ensure_dev_machine_id_mints_once_and_persists() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        let first = ensure_dev_machine_id(&paths).unwrap();
        let second = ensure_dev_machine_id(&paths).unwrap();
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(first, second);
        assert_eq!(cfg.daemon.dev_machine_id, Some(first));
    }

    #[test]
    fn write_then_load_cli_workspace_roundtrips() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        assert_eq!(load_cli_workspace(&paths).unwrap(), None);
        write_cli_workspace(&paths, "acme").unwrap();
        assert_eq!(load_cli_workspace(&paths).unwrap().as_deref(), Some("acme"));
    }

    #[test]
    fn write_cli_token_preserves_existing_workspace_slug() {
        // Re-login (e.g. token refresh) must not clobber the host's
        // existing workspace binding — otherwise users hit the picker
        // every time their token rolls.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "tok-1").unwrap();
        write_cli_workspace(&paths, "acme").unwrap();
        write_cli_token(&paths, "https://example.com", "tok-2").unwrap();
        let cfg = file::load_config(&paths).unwrap();
        let cli = cfg.cli.expect("cli section");
        assert_eq!(cli.token.as_deref(), Some("tok-2"));
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
    }

    #[test]
    fn write_cli_workspace_rebinds_when_called_with_new_slug() {
        // Workspace rebinding is intentionally allowed (membership
        // changes, user pick a different one on re-login). The login
        // flow gates this; the helper itself does not refuse.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "tok").unwrap();
        write_cli_workspace(&paths, "acme").unwrap();
        write_cli_workspace(&paths, "zenith").unwrap();
        assert_eq!(
            load_cli_workspace(&paths).unwrap().as_deref(),
            Some("zenith")
        );
    }

    #[test]
    fn clear_cli_token_keeps_runner_blocks() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        // Seed config with a runner block + token via the normal flow.
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx").unwrap();
        // Append a runner block manually by writing config.
        let mut cfg = file::load_config(&paths).unwrap();
        cfg.runners.push(RunnerConfig {
            name: "r1".into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("WEB".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: tmp.path().join("wd"),
            },
            agent: AgentSection::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        });
        file::write_config(&paths, &cfg).unwrap();

        clear_cli_token(&paths).unwrap();
        let after = file::load_config(&paths).unwrap();
        assert!(after.cli.as_ref().and_then(|c| c.token.as_ref()).is_none());
        assert_eq!(after.runners.len(), 1, "runner block must be preserved");
    }
}
