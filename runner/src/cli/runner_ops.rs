//! Shared local-side plumbing for both `pidash connect` (legacy
//! enrollment-token flow) and `pidash runner add` (CLI-token flow).
//!
//! After a successful cloud-side mint (`EnrollResponse`), both paths need
//! to write `[[runner]]` to `config.toml` and install the OS service unit
//! on first add. Legacy `pidash connect` responses also write per-runner
//! `credentials.toml`; new `pidash runner add` responses rely on the
//! shared dev-machine token in `[cli].token`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::cloud::http::{EnrollResponse, RunnerCredentials, write_runner_credentials};
use crate::config::file;
use crate::config::schema::{
    AgentKind, AgentSection, ApprovalPolicySection, ClaudeCodeSection, CodexSection, Config,
    CursorAgentSection, DaemonConfig, GrokSection, MuseCodeSection, OpenClawSection, RunnerConfig,
    WorkspaceSection,
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
/// - **OpenClaw** is provider-agnostic — the model is resolved by the
///   OpenClaw Gateway from whatever provider was configured at onboarding,
///   so its slug space is just as open as Cursor's; accept any non-empty
///   value and let OpenClaw reject unknown slugs.
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
        AgentKind::Codex => {
            has("gpt-") || m == "o3" || m == "o4" || has("o3-") || has("o4-") || has("codex")
        }
        // Cursor, OpenClaw, Grok, and Muse Code pull their model slug from
        // their own provider's catalog; accept any non-empty value and let the
        // agent reject unknown slugs.
        AgentKind::CursorAgent | AgentKind::OpenClaw | AgentKind::Grok | AgentKind::MuseCode => {
            !m.is_empty()
        }
        // The engine's model comes from the server's profile (design §4.3).
        AgentKind::Engine => false,
    }
}

/// Build the per-agent config sections for a fresh `[[runner]]`, applying
/// the operator's `--model` / `--reasoning-effort` to whichever agent was
/// selected. Inapplicable combinations are non-fatal: an orange warning is
/// printed and that knob falls back to the agent default (left unset in the
/// config). Shared by `pidash runner add` and the deprecated `pidash
/// connect`.
pub fn agent_sections_for(
    agent_kind: AgentKind,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> (
    CodexSection,
    ClaudeCodeSection,
    CursorAgentSection,
    OpenClawSection,
    GrokSection,
    MuseCodeSection,
) {
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
    let mut openclaw = OpenClawSection::default();
    let mut grok = GrokSection::default();
    let mut muse_code = MuseCodeSection::default();
    match agent_kind {
        AgentKind::Codex => {
            codex.model_default = model;
            codex.effort_default = effort;
        }
        AgentKind::ClaudeCode => claude_code.model_default = model,
        AgentKind::CursorAgent => cursor_agent.model_default = model,
        AgentKind::OpenClaw => openclaw.model_default = model,
        AgentKind::Grok => grok.model_default = model,
        AgentKind::MuseCode => muse_code.model_default = model,
        AgentKind::Engine => {}
    }
    (codex, claude_code, cursor_agent, openclaw, grok, muse_code)
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
///
/// `force` controls the cloud-URL mismatch guard: normally we refuse to
/// overwrite `[cli]` when this host is already enrolled with a different
/// cloud, but `pidash auth login --force` sets this to rebind the host's
/// `[daemon].cloud_url` to the new cloud instead of bailing.
pub fn write_cli_token(paths: &Paths, cloud_url: &str, token: &str, force: bool) -> Result<()> {
    let mut cfg = if paths.config_path().exists() {
        file::load_config(paths)?
    } else {
        Config {
            engine: Default::default(),
            auth: Default::default(),
            version: 2,
            daemon: DaemonConfig {
                cloud_url: cloud_url.to_string(),
                dev_machine_id: None,
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                agent_watchdog: Default::default(),
                auto_update: true,
            },
            runners: vec![],
            cli: None,
        }
    };
    // Pre-existing config? Don't quietly rebind it to a different cloud
    // unless the caller explicitly forces it (`pidash auth login --force`).
    if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
        if !force {
            anyhow::bail!(
                "this host is already enrolled with cloud {} — refusing to overwrite [cli] for a different cloud {} (pass --force to re-enroll against the new cloud)",
                cfg.daemon.cloud_url,
                cloud_url
            );
        }
        // Forced re-enrollment: rebind the daemon to the new cloud.
        cfg.daemon.cloud_url = cloud_url.to_string();
    }
    if cfg.daemon.cloud_url.is_empty() {
        cfg.daemon.cloud_url = cloud_url.to_string();
    }
    let mut cli = cfg.cli.take().unwrap_or_default();
    cli.token = Some(token.to_string());
    cfg.cli = Some(cli);
    file::write_config(paths, &cfg)?;
    Ok(())
}

/// Store the dev-machine token `token` for workspace `slug`, under the
/// config lock (see [`CliSection::set_workspace_token`] for how `as_default`
/// decides between refreshing the default workspace and adding one).
/// Bootstraps `config.toml` with `[daemon].cloud_url` on a fresh host and
/// applies the same cloud-mismatch guard as [`write_cli_token`].
pub fn store_workspace_token(
    paths: &Paths,
    cloud_url: &str,
    slug: &str,
    token: &str,
    as_default: bool,
    force: bool,
) -> Result<Config> {
    file::mutate_config_or_init(
        paths,
        || Some(bootstrap_config(cloud_url)),
        |cfg| {
            apply_cloud_binding(cfg, cloud_url, force)?;
            let mut cli = cfg.cli.take().unwrap_or_default();
            cli.set_workspace_token(slug, token, as_default);
            cfg.cli = Some(cli);
            Ok(())
        },
    )
}

/// Make sure `config.toml` exists and is bound to `cloud_url`, before a
/// login asks the user to approve anything. A different cloud is refused
/// unless `force`, which rebinds and drops the old cloud's tokens.
pub fn bind_cloud(paths: &Paths, cloud_url: &str, force: bool) -> Result<()> {
    file::mutate_config_or_init(
        paths,
        || Some(bootstrap_config(cloud_url)),
        |cfg| apply_cloud_binding(cfg, cloud_url, force),
    )?;
    Ok(())
}

fn bootstrap_config(cloud_url: &str) -> Config {
    Config {
        engine: Default::default(),
        auth: Default::default(),
        version: 2,
        daemon: DaemonConfig {
            cloud_url: cloud_url.to_string(),
            ..DaemonConfig::default()
        },
        runners: vec![],
        cli: None,
    }
}

fn apply_cloud_binding(cfg: &mut Config, cloud_url: &str, force: bool) -> Result<()> {
    if !cfg.daemon.cloud_url.is_empty() && cfg.daemon.cloud_url != cloud_url {
        if !force {
            anyhow::bail!(
                "this host is already enrolled with cloud {} — refusing to sign in to a different cloud {} (pass --force to re-enroll against the new cloud)",
                cfg.daemon.cloud_url,
                cloud_url
            );
        }
        cfg.daemon.cloud_url = cloud_url.to_string();
        // Tokens of the old cloud are useless against the new one.
        if let Some(cli) = cfg.cli.as_mut() {
            cli.clear_all_tokens();
        }
    }
    if cfg.daemon.cloud_url.is_empty() {
        cfg.daemon.cloud_url = cloud_url.to_string();
    }
    Ok(())
}

/// Record the signed-in account (`pidash auth login`), for `Hello`.
pub fn store_account(paths: &Paths, email: Option<&str>, user_id: Option<&str>) -> Result<()> {
    if email.is_none() && user_id.is_none() {
        return Ok(());
    }
    file::mutate_config(paths, |cfg| {
        let mut cli = cfg.cli.take().unwrap_or_default();
        if let Some(email) = email {
            cli.account_email = Some(email.to_string());
        }
        if let Some(user_id) = user_id {
            cli.account_user_id = Some(user_id.to_string());
        }
        cfg.cli = Some(cli);
        Ok(())
    })?;
    Ok(())
}

/// The token for workspace `slug` (`None` = the default workspace), as
/// [`CliSection::token_for_workspace`] picks it.
pub fn load_workspace_token(paths: &Paths, slug: Option<&str>) -> Result<Option<String>> {
    if !paths.config_path().exists() {
        return Ok(None);
    }
    let cfg = file::load_config(paths)?;
    Ok(cfg
        .cli
        .as_ref()
        .and_then(|c| c.token_for_workspace(slug))
        .map(str::to_string))
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

/// Clear every workspace token (`[cli].token` and `[[cli.workspaces]]`) from
/// `config.toml`. Used by `pidash auth logout`.
/// Leaves `[[runner]]` blocks untouched so the daemon keeps running
/// under its own identity. No-op if no config exists.
pub fn clear_cli_token(paths: &Paths) -> Result<()> {
    if !paths.config_path().exists() {
        return Ok(());
    }
    file::mutate_config(paths, |cfg| {
        if let Some(ref mut cli) = cfg.cli {
            cli.clear_all_tokens();
        }
        Ok(())
    })?;
    Ok(())
}

/// Store (or with `None`, forget) the machine's engine credential and record
/// the engine sign-in state, under the config lock. The secret lands where
/// `[auth] credentials_store` says.
pub fn store_engine_credential(
    paths: &Paths,
    credential: Option<&crate::engine::api::IssuedEngineCredential>,
    state: crate::config::schema::EngineSignInState,
) -> Result<Config> {
    file::mutate_config(paths, |cfg| {
        let cli = cfg.cli.get_or_insert_with(Default::default);
        cli.engine_credential = credential.map(|c| crate::config::schema::EngineCredential {
            token: c.token.clone(),
            expires_at: c.expires_at,
        });
        cli.engine_sign_in_state = Some(state);
        Ok(())
    })
}

/// Forget one workspace's token (`pidash auth logout --workspace <slug>`).
/// Returns whether the workspace had a token.
pub fn clear_workspace_token(paths: &Paths, slug: &str) -> Result<bool> {
    if !paths.config_path().exists() {
        return Ok(false);
    }
    let mut removed = false;
    file::mutate_config(paths, |cfg| {
        if let Some(ref mut cli) = cfg.cli {
            removed = cli.remove_workspace_token(slug);
        }
        Ok(())
    })?;
    Ok(removed)
}

/// Result of applying a mint locally: the new `RunnerConfig` block and
/// whether this was the host's first runner (so callers can decide
/// whether to install the OS service unit / restart it).
#[derive(Debug)]
pub struct AppliedRunner {
    pub runner: RunnerConfig,
    pub is_first_runner: bool,
    /// The config already held a block with this `runner_id` before this
    /// call. The server answers a repeated create (same machine and
    /// project for the engine) with the runner it already has, so this is
    /// a successful no-op: the existing block is kept as it was.
    pub already_present: bool,
}

/// Attached (as context) to an [`apply_enroll_response`] error when a
/// block with the response's `runner_id` was already in the config. The
/// runner existed before this call, so a caller must not roll it back:
/// stripping the block or deleting the server row would remove a runner
/// that was working. See [`may_roll_back`].
#[derive(Debug)]
pub struct KeptExistingRunner;

impl std::fmt::Display for KeptExistingRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("this runner was already on this computer; it was left in place")
    }
}

/// Whether a caller may roll back (strip the `[[runner]]` block, delete
/// the server row) after `err` from [`apply_enroll_response`]. False when
/// the runner existed before the call.
pub fn may_roll_back(err: &anyhow::Error) -> bool {
    err.downcast_ref::<KeptExistingRunner>().is_none()
}

/// The runner ids in the config right now, read before a create so a later
/// rollback can tell a runner this call made from one that was already
/// here. An unreadable config gives an empty set.
pub fn runner_ids_before_create(paths: &Paths) -> std::collections::HashSet<Uuid> {
    file::load_config(paths)
        .map(|cfg| cfg.runners.iter().map(|r| r.runner_id).collect())
        .unwrap_or_default()
}

/// Whether this call made the runner in `resp`, so a local failure may
/// strip its block and delete its server row. False when the server says
/// it returned a runner it already had (`created: false`) or the config
/// held the id before the create: rolling back would remove a runner that
/// is working here, or one that lives on after a local sign-out.
pub fn created_by_this_call(
    resp: &EnrollResponse,
    before: &std::collections::HashSet<Uuid>,
) -> bool {
    resp.created.unwrap_or(true) && !before.contains(&resp.runner_id)
}

/// [`may_roll_back`] and [`created_by_this_call`] together: the one check
/// every create path makes before it rolls back after a local failure.
pub fn may_roll_back_create(
    err: &anyhow::Error,
    resp: &EnrollResponse,
    before: &std::collections::HashSet<Uuid>,
) -> bool {
    may_roll_back(err) && created_by_this_call(resp, before)
}

#[derive(Debug, Clone)]
pub struct ApplyEnrollOptions<'a> {
    pub working_dir: Option<PathBuf>,
    pub agent_kind: AgentKind,
    pub model: Option<&'a str>,
    pub reasoning_effort: Option<&'a str>,
}

/// The exclusive working directory for a runner created without an explicit
/// `--working-dir`. One dir per runner, under the data dir, named
/// `<project_slug>_<runner_slug>` with a short runner-id suffix so two runners
/// — even in workspaces whose project slugs collide — can never resolve to the
/// same path. Config validation ([`Config::validate`]) rejects any remaining
/// collision. See PDASHOSS01-134.
pub fn default_working_dir(
    paths: &Paths,
    project_slug: &str,
    runner_name: &str,
    runner_id: Uuid,
) -> PathBuf {
    default_working_dir_in(&paths.data_dir, project_slug, runner_name, runner_id)
}

/// [`default_working_dir`] against a bare `data_dir` rather than a full
/// [`Paths`]. Kept separate so the legacy-pool migration
/// ([`crate::config::migrate`]), which only has the data dir in hand, can
/// assign a displaced runner exactly the directory a fresh `runner add` would
/// have — the naming rule lives in one place.
pub fn default_working_dir_in(
    data_dir: &Path,
    project_slug: &str,
    runner_name: &str,
    runner_id: Uuid,
) -> PathBuf {
    fn slug(s: &str, fallback: &str) -> String {
        let out: String = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let out = out.trim_matches('-').to_string();
        if out.is_empty() {
            fallback.to_string()
        } else {
            out
        }
    }
    let proj = slug(project_slug, "project");
    let name = slug(runner_name, "runner");
    let short: String = runner_id.simple().to_string().chars().take(8).collect();
    data_dir
        .join("workspaces")
        .join(format!("{proj}_{name}_{short}"))
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
    options: ApplyEnrollOptions<'_>,
) -> Result<AppliedRunner> {
    let working_dir = options.working_dir.unwrap_or_else(|| {
        default_working_dir(
            paths,
            &resp.project_identifier,
            &resp.runner_name,
            resp.runner_id,
        )
    });

    let (codex, claude_code, cursor_agent, openclaw, grok, muse_code) =
        agent_sections_for(options.agent_kind, options.model, options.reasoning_effort);
    let new_runner = RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: WorkspaceSection { working_dir },
        agent: AgentSection {
            kind: options.agent_kind,
        },
        codex,
        claude_code,
        cursor_agent,
        openclaw,
        grok,
        muse_code,
        approval_policy: ApprovalPolicySection::default(),
    };

    // Bootstrap closure for the no-config-yet case (first runner on a
    // fresh host that ran `pidash auth login` but not `pidash connect`).
    // Seeds a minimal `[daemon]` block from the response's cloud URL.
    let bootstrap_cloud_url = cloud_url.to_string();
    let init_cfg = move || {
        Some(Config {
            engine: Default::default(),
            auth: Default::default(),
            version: 2,
            daemon: DaemonConfig {
                cloud_url: bootstrap_cloud_url.clone(),
                dev_machine_id: None,
                log_level: "info".to_string(),
                log_retention_days: 14,
                agent_observability_v1: false,
                agent_watchdog: Default::default(),
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
    let runner_id = new_runner.runner_id;
    let mut is_first_runner = false;
    let mut already_present = false;
    let cfg_after = file::mutate_config_or_init(paths, init_cfg, |cfg| {
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
        // A repeated create for a runner this host already has (the server
        // returns the existing runner_id, e.g. two engine adds for one
        // project): keep the block that is there and succeed.
        if cfg
            .runners
            .iter()
            .any(|r| r.runner_id == new_runner_for_closure.runner_id)
        {
            already_present = true;
            return Ok(());
        }
        // One engine runner per machine and project (design §5). The
        // caller checked before enrolling, but outside this lock: two adds
        // at the same time both pass that check. (The runner cap is checked
        // by `Config::validate` on this same write.)
        if new_runner_for_closure.agent.kind == AgentKind::Engine
            && let Some(existing) = cfg.runners.iter().find(|r| {
                r.runner_id != new_runner_for_closure.runner_id
                    && r.agent.kind == AgentKind::Engine
                    && r.workspace_slug == new_runner_for_closure.workspace_slug
                    && r.project_slug.as_deref().is_some_and(|p| {
                        new_runner_for_closure
                            .project_slug
                            .as_deref()
                            .is_some_and(|q| p.eq_ignore_ascii_case(q))
                    })
            })
        {
            anyhow::bail!(
                "the built-in engine is already on for {} in {} (runner {})",
                existing.project_slug.as_deref().unwrap_or_default(),
                existing.workspace_slug.as_deref().unwrap_or_default(),
                existing.name
            );
        }
        is_first_runner = cfg.runners.is_empty();
        cfg.runners.push(new_runner_for_closure);
        Ok(())
    });
    let cfg_after = match cfg_after {
        Ok(cfg) => cfg,
        Err(e) if already_present => {
            return Err(e
                .context(KeptExistingRunner)
                .context("persisting [[runner]] block under config lock"));
        }
        Err(e) => return Err(e.context("persisting [[runner]] block under config lock")),
    };
    // A runner added again is no longer one the user removed locally.
    crate::engine::tombstones::forget(paths, runner_id);
    let new_runner = cfg_after
        .runners
        .iter()
        .find(|runner| runner.runner_id == runner_id)
        .cloned()
        .unwrap_or(new_runner);
    let applied = AppliedRunner {
        runner: new_runner,
        is_first_runner,
        already_present,
    };
    match write_enroll_credentials(paths, resp, cloud_url).await {
        Ok(()) => Ok(applied),
        Err(e) if already_present => Err(e.context(KeptExistingRunner)),
        Err(e) => Err(e),
    }
}

/// The credential half of [`apply_enroll_response`], after the block is in
/// the config.
async fn write_enroll_credentials(
    paths: &Paths,
    resp: &EnrollResponse,
    cloud_url: &str,
) -> Result<()> {
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
        // The token belongs to the runner's workspace (design §4.9): store
        // it in that workspace's slot, never over another workspace's
        // default `[cli].token`.
        if resp.workspace_slug.trim().is_empty() {
            write_cli_token(paths, cloud_url, machine_token, false)
                .context("writing shared dev-machine token to [cli].token")?;
        } else {
            store_workspace_token(
                paths,
                cloud_url,
                &resp.workspace_slug,
                machine_token,
                false,
                false,
            )
            .context("writing the workspace's dev-machine token")?;
        }
    }
    if resp.refresh_token.trim().is_empty() {
        let workspace =
            (!resp.workspace_slug.trim().is_empty()).then_some(resp.workspace_slug.as_str());
        let has_machine_token = file::load_config(paths)?
            .cli
            .as_ref()
            .and_then(|cli| cli.token_for_workspace(workspace))
            .map(|token| token.starts_with("mt_"))
            .unwrap_or(false);
        if !has_machine_token {
            anyhow::bail!(
                "cloud did not return per-runner credentials and no dev-machine token is configured"
            );
        }
        return Ok(());
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::http::EnrollResponse;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn model_routes_to_selected_agent_section() {
        let (codex, claude, cursor, openclaw, _, _) =
            agent_sections_for(AgentKind::ClaudeCode, Some("claude-opus-4-8"), None);
        assert_eq!(claude.model_default.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(codex.model_default, None);
        assert_eq!(cursor.model_default, None);
        assert_eq!(openclaw.model_default, None);
    }

    #[test]
    fn codex_model_and_effort_are_applied_together() {
        let (codex, _, _, _, _, _) =
            agent_sections_for(AgentKind::Codex, Some("gpt-5.5"), Some("High"));
        assert_eq!(codex.model_default.as_deref(), Some("gpt-5.5"));
        // Effort is normalized to lowercase.
        assert_eq!(codex.effort_default.as_deref(), Some("high"));
    }

    #[test]
    fn mismatched_model_falls_back_to_agent_default() {
        // The user's example: a Codex model handed to the Claude agent.
        // Non-fatal — the model is dropped (warning printed) and the
        // section is left at its default (None).
        let (_, claude, _, _, _, _) =
            agent_sections_for(AgentKind::ClaudeCode, Some("gpt-5.5"), None);
        assert_eq!(claude.model_default, None);
    }

    #[test]
    fn effort_ignored_for_non_codex_agents() {
        let (_, claude, _, _, _, _) =
            agent_sections_for(AgentKind::ClaudeCode, Some("claude-opus-4-8"), Some("high"));
        // Model still applies; effort has no home on the claude section.
        assert_eq!(claude.model_default.as_deref(), Some("claude-opus-4-8"));
    }

    #[test]
    fn unknown_codex_effort_is_dropped() {
        let (codex, _, _, _, _, _) =
            agent_sections_for(AgentKind::Codex, Some("gpt-5.5"), Some("turbo"));
        assert_eq!(codex.model_default.as_deref(), Some("gpt-5.5"));
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn cursor_accepts_any_nonempty_slug() {
        let (_, _, cursor, _, _, _) = agent_sections_for(
            AgentKind::CursorAgent,
            Some("claude-opus-4-8-thinking-high"),
            None,
        );
        assert_eq!(
            cursor.model_default.as_deref(),
            Some("claude-opus-4-8-thinking-high")
        );
    }

    #[test]
    fn openclaw_accepts_any_nonempty_slug() {
        // OpenClaw's model space is provider-agnostic (resolved by the
        // Gateway), so any non-empty slug routes to its section.
        let (codex, claude, cursor, openclaw, _, _) =
            agent_sections_for(AgentKind::OpenClaw, Some("anthropic/claude-opus-4-8"), None);
        assert_eq!(
            openclaw.model_default.as_deref(),
            Some("anthropic/claude-opus-4-8")
        );
        assert_eq!(codex.model_default, None);
        assert_eq!(claude.model_default, None);
        assert_eq!(cursor.model_default, None);
    }

    #[test]
    fn blank_model_is_treated_as_unset() {
        let (codex, _, _, _, _, _) = agent_sections_for(AgentKind::Codex, Some("   "), None);
        assert_eq!(codex.model_default, None);
    }

    #[test]
    fn codex_effort_dropped_when_model_inapplicable() {
        // A Claude model handed to codex: the model is dropped, and the
        // effort meant for it must not survive onto codex's default model.
        let (codex, _, _, _, _, _) =
            agent_sections_for(AgentKind::Codex, Some("claude-opus-4-8"), Some("high"));
        assert_eq!(codex.model_default, None);
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn codex_effort_dropped_without_a_model() {
        // Effort is meaningless without an explicit model (see the
        // CodexSection::effort_default contract).
        let (codex, _, _, _, _, _) = agent_sections_for(AgentKind::Codex, None, Some("high"));
        assert_eq!(codex.model_default, None);
        assert_eq!(codex.effort_default, None);
    }

    #[test]
    fn bare_prefix_models_are_rejected() {
        // A dash-terminated prefix with nothing after it is an incomplete
        // slug, not a model — it must not become a bogus model_default.
        let (_, claude, _, _, _, _) =
            agent_sections_for(AgentKind::ClaudeCode, Some("claude-"), None);
        assert_eq!(claude.model_default, None);
        let (codex, _, _, _, _, _) = agent_sections_for(AgentKind::Codex, Some("gpt-"), None);
        assert_eq!(codex.model_default, None);
    }

    #[test]
    fn bare_openai_reasoning_models_are_accepted() {
        // `o3` / `o4` are valid bare model names; the bare-prefix guard
        // must not reject them.
        let (codex, _, _, _, _, _) = agent_sections_for(AgentKind::Codex, Some("o3"), None);
        assert_eq!(codex.model_default.as_deref(), Some("o3"));
    }

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn enroll_options(working_dir: PathBuf, agent_kind: AgentKind) -> ApplyEnrollOptions<'static> {
        ApplyEnrollOptions {
            working_dir: Some(working_dir),
            agent_kind,
            model: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn only_a_runner_this_call_created_may_be_rolled_back() {
        let mut resp = sample_response("r");
        let empty = std::collections::HashSet::new();
        // A server without the field never returns an existing runner.
        assert!(created_by_this_call(&resp, &empty));
        resp.created = Some(true);
        assert!(created_by_this_call(&resp, &empty));
        // The server returned one it already had.
        resp.created = Some(false);
        assert!(!created_by_this_call(&resp, &empty));
        // The config held the id before the create.
        resp.created = Some(true);
        let before: std::collections::HashSet<_> = [resp.runner_id].into_iter().collect();
        assert!(!created_by_this_call(&resp, &before));
        let err = anyhow::anyhow!("creating the folder failed");
        assert!(!may_roll_back_create(&err, &resp, &before));
        assert!(may_roll_back_create(&err, &resp, &empty));
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
            created: None,
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
            enroll_options(tmp.path().join("wd"), AgentKind::Codex),
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
            enroll_options(tmp.path().join("wd1"), AgentKind::Codex),
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let applied = apply_enroll_response(
            &paths,
            &r2,
            "https://example.com",
            enroll_options(tmp.path().join("wd2"), AgentKind::Codex),
        )
        .await
        .unwrap();
        assert!(!applied.is_first_runner);
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 2);
    }

    /// PIDESKAPP-122 review: enrolling a runner of a second workspace
    /// stores the returned machine token in that workspace's slot and
    /// leaves the default workspace's token alone.
    #[tokio::test]
    async fn enrolment_token_goes_to_the_runners_workspace_slot() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "legacy-user-token", false).unwrap();
        write_cli_workspace(&paths, "acme").unwrap();

        let mut resp = sample_response("b1");
        resp.workspace_slug = "beta".into();
        resp.refresh_token = String::new();
        resp.machine_token = Some("mt_beta".into());
        apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            enroll_options(tmp.path().join("wb"), AgentKind::Codex),
        )
        .await
        .unwrap();

        let cli = file::load_config(&paths).unwrap().cli.expect("cli section");
        assert_eq!(cli.token.as_deref(), Some("legacy-user-token"));
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
        assert_eq!(cli.token_for_workspace(Some("beta")), Some("mt_beta"));
    }

    #[test]
    fn default_working_dir_is_named_and_exclusive_per_runner() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let a = default_working_dir(&paths, "WEB", "codex-runner", id_a);
        let b = default_working_dir(&paths, "WEB", "codex-runner", id_b);
        // Both land under <data_dir>/workspaces and carry the project + runner
        // slug, lowercased.
        assert!(a.starts_with(paths.data_dir.join("workspaces")));
        let a_name = a.file_name().unwrap().to_string_lossy();
        assert!(a_name.starts_with("web_codex-runner_"), "name: {a_name}");
        // Two runners with an identical project+name still resolve to distinct
        // directories because of the runner-id suffix.
        assert_ne!(a, b);
    }

    #[test]
    fn default_working_dir_handles_empty_slugs() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let dir = default_working_dir(&paths, "***", "", Uuid::new_v4());
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("project_runner_"), "name: {name}");
    }

    #[tokio::test]
    async fn add_without_working_dir_auto_creates_an_exclusive_dir() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let resp = sample_response("codex");
        let applied = apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            ApplyEnrollOptions {
                working_dir: None,
                agent_kind: AgentKind::Codex,
                model: None,
                reasoning_effort: None,
            },
        )
        .await
        .unwrap();
        let wd = &applied.runner.workspace.working_dir;
        assert!(
            wd.starts_with(paths.data_dir.join("workspaces")),
            "wd: {wd:?}"
        );
        // The persisted config validates (exclusive dir, no collision).
        file::load_config(&paths).unwrap().validate().unwrap();
    }

    #[tokio::test]
    async fn a_repeated_create_for_the_same_runner_keeps_the_block() {
        // The server answers a second create for the same machine and
        // project with the runner it already has.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let resp = sample_response("engine");
        apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            enroll_options(tmp.path().join("wd1"), AgentKind::Engine),
        )
        .await
        .unwrap();
        let again = apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            enroll_options(tmp.path().join("wd2"), AgentKind::Engine),
        )
        .await
        .unwrap();
        assert!(again.already_present);
        assert_eq!(again.runner.workspace.working_dir, tmp.path().join("wd1"));
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.runners.len(), 1);
        assert_eq!(cfg.runners[0].workspace.working_dir, tmp.path().join("wd1"));
    }

    #[tokio::test]
    async fn a_failure_after_a_repeated_create_forbids_rollback() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let resp = sample_response("engine");
        apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            enroll_options(tmp.path().join("wd1"), AgentKind::Engine),
        )
        .await
        .unwrap();
        // A folder where the credentials file goes: the write fails.
        let creds = paths.for_runner(resp.runner_id).credentials_path();
        std::fs::remove_file(&creds).unwrap();
        std::fs::create_dir_all(&creds).unwrap();
        let err = apply_enroll_response(
            &paths,
            &resp,
            "https://example.com",
            enroll_options(tmp.path().join("wd1"), AgentKind::Engine),
        )
        .await
        .unwrap_err();
        assert!(!may_roll_back(&err), "{err:#}");
        // A brand-new runner that fails the same way may be rolled back.
        let fresh = sample_response("fresh");
        let creds = paths.for_runner(fresh.runner_id).credentials_path();
        std::fs::create_dir_all(&creds).unwrap();
        let err = apply_enroll_response(
            &paths,
            &fresh,
            "https://example.com",
            enroll_options(tmp.path().join("wd3"), AgentKind::Codex),
        )
        .await
        .unwrap_err();
        assert!(may_roll_back(&err), "{err:#}");
    }

    #[tokio::test]
    async fn add_rejects_duplicate_working_dir() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let repo = tmp.path().join("repo");

        let r1 = sample_response("r1");
        apply_enroll_response(
            &paths,
            &r1,
            "https://example.com",
            enroll_options(repo.clone(), AgentKind::Codex),
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let err = apply_enroll_response(
            &paths,
            &r2,
            "https://example.com",
            enroll_options(repo, AgentKind::Codex),
        )
        .await
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("share") && msg.contains("working_dir"),
            "unexpected duplicate-working-dir error: {msg}"
        );
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
            enroll_options(tmp.path().join("wd1"), AgentKind::Codex),
        )
        .await
        .unwrap();

        let r2 = sample_response("r2");
        let err = apply_enroll_response(
            &paths,
            &r2,
            "https://cloud-b.example.com",
            enroll_options(tmp.path().join("wd2"), AgentKind::Codex),
        )
        .await
        .unwrap_err();
        assert!(format!("{err:#}").contains("refusing to add a runner pointing at"));
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
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx", false).unwrap();
        let token = load_cli_token(&paths).unwrap();
        assert_eq!(token.as_deref(), Some("pi_dash_api_xxx"));
    }

    #[test]
    fn ensure_dev_machine_id_mints_once_and_persists() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx", false).unwrap();
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
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx", false).unwrap();
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
        write_cli_token(&paths, "https://example.com", "tok-1", false).unwrap();
        write_cli_workspace(&paths, "acme").unwrap();
        write_cli_token(&paths, "https://example.com", "tok-2", false).unwrap();
        let cfg = file::load_config(&paths).unwrap();
        let cli = cfg.cli.expect("cli section");
        assert_eq!(cli.token.as_deref(), Some("tok-2"));
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
    }

    #[test]
    fn write_cli_token_refuses_different_cloud_without_force() {
        // A host already enrolled with one cloud must not be silently
        // rebound to another — `pidash auth login --url <other>` bails
        // unless the operator passes --force.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://one.example.com", "tok-1", false).unwrap();
        let err = write_cli_token(&paths, "https://two.example.com", "tok-2", false)
            .expect_err("rebind to a different cloud without --force must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("already enrolled") && msg.contains("--force"),
            "unexpected error message: {msg}"
        );
        // The original binding and token are left untouched.
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.daemon.cloud_url, "https://one.example.com");
        assert_eq!(cfg.cli.and_then(|c| c.token).as_deref(), Some("tok-1"));
    }

    #[test]
    fn write_cli_token_force_rebinds_to_different_cloud() {
        // `pidash auth login --url <other> --force` re-enrolls the host:
        // both [daemon].cloud_url and [cli].token move to the new cloud.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://one.example.com", "tok-1", false).unwrap();
        write_cli_token(&paths, "https://two.example.com", "tok-2", true)
            .expect("forced rebind to a different cloud should succeed");
        let cfg = file::load_config(&paths).unwrap();
        assert_eq!(cfg.daemon.cloud_url, "https://two.example.com");
        assert_eq!(cfg.cli.and_then(|c| c.token).as_deref(), Some("tok-2"));
    }

    #[test]
    fn write_cli_workspace_rebinds_when_called_with_new_slug() {
        // Workspace rebinding is intentionally allowed (membership
        // changes, user pick a different one on re-login). The login
        // flow gates this; the helper itself does not refuse.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        write_cli_token(&paths, "https://example.com", "tok", false).unwrap();
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
        write_cli_token(&paths, "https://example.com", "pi_dash_api_xxx", false).unwrap();
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
            openclaw: OpenClawSection::default(),
            grok: GrokSection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: ApprovalPolicySection::default(),
        });
        file::write_config(&paths, &cfg).unwrap();

        clear_cli_token(&paths).unwrap();
        let after = file::load_config(&paths).unwrap();
        assert!(after.cli.as_ref().and_then(|c| c.token.as_ref()).is_none());
        assert_eq!(after.runners.len(), 1, "runner block must be preserved");
    }

    #[test]
    fn store_workspace_token_adds_a_workspace_next_to_the_default() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        bind_cloud(&paths, "https://example.com", false).unwrap();
        store_workspace_token(&paths, "https://example.com", "acme", "mt_a", true, false).unwrap();
        // `pidash auth login --workspace beta`
        store_workspace_token(&paths, "https://example.com", "beta", "mt_b", false, false).unwrap();
        assert_eq!(load_cli_token(&paths).unwrap().as_deref(), Some("mt_a"));
        assert_eq!(load_cli_workspace(&paths).unwrap().as_deref(), Some("acme"));
        assert_eq!(
            load_workspace_token(&paths, Some("beta"))
                .unwrap()
                .as_deref(),
            Some("mt_b")
        );
        assert_eq!(
            load_workspace_token(&paths, Some("acme"))
                .unwrap()
                .as_deref(),
            Some("mt_a")
        );

        // Signing out one workspace leaves the other.
        assert!(clear_workspace_token(&paths, "beta").unwrap());
        assert_eq!(load_workspace_token(&paths, Some("beta")).unwrap(), None);
        assert_eq!(load_cli_token(&paths).unwrap().as_deref(), Some("mt_a"));

        // A different cloud is refused before anything is approved.
        assert!(bind_cloud(&paths, "https://other.example.com", false).is_err());
    }

    #[test]
    fn store_account_records_the_signed_in_user() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        bind_cloud(&paths, "https://example.com", false).unwrap();
        store_account(&paths, Some("dev@example.com"), Some("u-1")).unwrap();
        let cli = file::load_config(&paths).unwrap().cli.unwrap();
        assert_eq!(cli.account_email.as_deref(), Some("dev@example.com"));
        assert_eq!(cli.account_user_id.as_deref(), Some("u-1"));
    }
}
