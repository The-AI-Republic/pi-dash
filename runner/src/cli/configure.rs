use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use crate::cloud::register::{RegisterError, RegisterRequest, register};
use crate::config::schema::{AgentKind, Config, Credentials};
use crate::util::paths::Paths;
use crate::util::runner_name;

/// Max attempts when the auto-generated runner name happens to collide. 62³
/// (≈238k) possible suffixes per workspace — five tries is far more than
/// enough absent a truly pathological collision.
const MAX_AUTO_NAME_RETRIES: u32 = 5;

#[derive(Debug, ClapArgs)]
pub struct Args {
    // --- Registration (required together on first setup) ------------------
    /// Pi Dash cloud base URL. Required (with `--token`) on first setup.
    #[arg(long)]
    pub url: Option<String>,

    /// Registration token issued by the cloud UI. Required (with `--url`)
    /// on first setup; consumed to mint the runner's persistent credential.
    #[arg(long)]
    pub token: Option<String>,

    // --- Runner / workspace -----------------------------------------------
    /// Human-friendly runner name.
    #[arg(long)]
    pub name: Option<String>,

    /// Workspace directory the runner clones into.
    #[arg(long)]
    pub working_dir: Option<PathBuf>,

    /// Pi Dash project this runner serves. Required when registering
    /// (via --url + --token). The runner becomes a permanent member of
    /// the project's default pod (or the explicit --pod when set).
    /// To discover available projects: `pidash token list-projects`.
    #[arg(long)]
    pub project: Option<String>,

    /// Pod within the project. Optional; defaults to the project's
    /// default pod. The bare suffix (e.g. `--pod beefy`) is auto-prefixed
    /// with the project identifier so the user doesn't have to repeat it.
    #[arg(long)]
    pub pod: Option<String>,

    // --- Agent selection --------------------------------------------------
    /// Which AI agent CLI the runner drives. Arrow-key picker on a TTY
    /// during first-setup if omitted; a no-op partial edit if omitted here.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    // --- Codex section ----------------------------------------------------
    /// Override `[codex].binary` (path or command name).
    #[arg(long)]
    pub codex_binary: Option<String>,

    /// Override `[codex].model_default`.
    #[arg(long)]
    pub codex_model: Option<String>,

    // --- Claude Code section ----------------------------------------------
    /// Override `[claude_code].binary`.
    #[arg(long)]
    pub claude_binary: Option<String>,

    /// Override `[claude_code].model_default`.
    #[arg(long)]
    pub claude_model: Option<String>,

    // --- Approval policy (scalars only — list fields live in the TUI) -----
    /// Toggle `[approval_policy].auto_approve_readonly_shell`.
    #[arg(long)]
    pub approval_auto_readonly: Option<bool>,

    /// Toggle `[approval_policy].auto_approve_workspace_writes`.
    #[arg(long)]
    pub approval_auto_writes: Option<bool>,

    /// Toggle `[approval_policy].auto_approve_network`.
    #[arg(long)]
    pub approval_auto_network: Option<bool>,

    // --- Logging ----------------------------------------------------------
    /// Override `[logging].level` (trace|debug|info|warn|error).
    #[arg(long)]
    pub log_level: Option<String>,

    /// Override `[logging].retention_days`.
    #[arg(long)]
    pub log_retention_days: Option<u32>,

    // --- Behaviour flags (not persisted) ----------------------------------
    /// Skip on-install doctor checks (not recommended). Also skips the
    /// auth-gate retry loop, since there's nothing to re-check.
    #[arg(long)]
    pub skip_doctor: bool,

    /// Skip installing / starting the OS service at the end. Use from CI or
    /// Ansible playbooks that manage the daemon lifecycle themselves, or when
    /// you only want to write the config files without bouncing a daemon.
    #[arg(long)]
    pub skip_service: bool,

    /// Skip the `sudo loginctl enable-linger` step (Linux only). Without
    /// linger the daemon only starts at login, not at boot. Set this in
    /// CI / unattended installs where a sudo password prompt would hang.
    #[arg(long)]
    pub skip_linger: bool,
}

impl Args {
    /// Returns true if any non-registration flag was set — i.e. the user
    /// intends to *edit* an existing config rather than register a new one.
    /// Used to decide whether to bail out or just mutate specific fields.
    fn has_edit_flags(&self) -> bool {
        self.name.is_some()
            || self.working_dir.is_some()
            || self.agent.is_some()
            || self.codex_binary.is_some()
            || self.codex_model.is_some()
            || self.claude_binary.is_some()
            || self.claude_model.is_some()
            || self.approval_auto_readonly.is_some()
            || self.approval_auto_writes.is_some()
            || self.approval_auto_network.is_some()
            || self.log_level.is_some()
            || self.log_retention_days.is_some()
    }

    /// Apply every `--<flag>` that was set as a mutation on an existing
    /// `Config`. Returns `true` if any field actually changed.
    ///
    /// Targets the primary runner (single-runner mode); a future multi-runner
    /// `--runner <name>` selector will pick a specific entry instead.
    fn apply_to(&self, cfg: &mut Config) -> bool {
        let mut changed = false;
        if let Some(url) = &self.url
            && cfg.daemon.cloud_url != *url
        {
            cfg.daemon.cloud_url = url.clone();
            changed = true;
        }
        let runner = cfg.primary_runner_mut();
        if let Some(name) = &self.name
            && runner.name != *name
        {
            runner.name = name.clone();
            changed = true;
        }
        if let Some(wd) = &self.working_dir
            && runner.workspace.working_dir != *wd
        {
            runner.workspace.working_dir = wd.clone();
            changed = true;
        }
        if let Some(kind) = self.agent
            && runner.agent.kind != kind
        {
            runner.agent.kind = kind;
            changed = true;
        }
        if let Some(b) = &self.codex_binary
            && runner.codex.binary != *b
        {
            runner.codex.binary = b.clone();
            changed = true;
        }
        if let Some(m) = &self.codex_model
            && runner.codex.model_default.as_ref() != Some(m)
        {
            runner.codex.model_default = Some(m.clone());
            changed = true;
        }
        if let Some(b) = &self.claude_binary
            && runner.claude_code.binary != *b
        {
            runner.claude_code.binary = b.clone();
            changed = true;
        }
        if let Some(m) = &self.claude_model
            && runner.claude_code.model_default.as_ref() != Some(m)
        {
            runner.claude_code.model_default = Some(m.clone());
            changed = true;
        }
        if let Some(v) = self.approval_auto_readonly
            && runner.approval_policy.auto_approve_readonly_shell != v
        {
            runner.approval_policy.auto_approve_readonly_shell = v;
            changed = true;
        }
        if let Some(v) = self.approval_auto_writes
            && runner.approval_policy.auto_approve_workspace_writes != v
        {
            runner.approval_policy.auto_approve_workspace_writes = v;
            changed = true;
        }
        if let Some(v) = self.approval_auto_network
            && runner.approval_policy.auto_approve_network != v
        {
            runner.approval_policy.auto_approve_network = v;
            changed = true;
        }
        if let Some(lvl) = &self.log_level
            && cfg.daemon.log_level != *lvl
        {
            cfg.daemon.log_level = lvl.clone();
            changed = true;
        }
        if let Some(n) = self.log_retention_days
            && cfg.daemon.log_retention_days != n
        {
            cfg.daemon.log_retention_days = n;
            changed = true;
        }
        changed
    }
}

/// Inputs for the core registration flow. `cli::install::run` also builds one
/// of these — once via clap, once via interactive prompts.
pub struct RegisterInputs {
    pub url: String,
    pub token: String,
    pub name: Option<String>,
    pub working_dir: Option<PathBuf>,
    /// Explicit agent choice; `None` means "ask on a TTY, else keep the
    /// existing config's kind, else Codex."
    pub agent: Option<AgentKind>,
    /// Project identifier this runner serves. Required at registration —
    /// the runner is bound to one project for its lifetime.
    pub project: String,
    /// Optional pod name within the project. Defaults to the project's
    /// default pod when omitted.
    pub pod: Option<String>,
    pub skip_doctor: bool,
    pub skip_service: bool,
    pub skip_linger: bool,
    /// Full clap Args, if we came from a direct CLI invocation. Lets the
    /// register path also apply any other `--<flag>` the user passed
    /// (e.g. `--codex-model gpt-5 --approval-auto-readonly true`) before
    /// writing the fresh config to disk. `None` when built from the install
    /// wizard's interactive prompts, which don't collect those extras.
    pub extras: Option<Args>,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    // Decision tree for `pidash configure`:
    //
    // 1. Bare invocation (no flags at all) → drop into the TUI's Config tab.
    //    The user wants to browse / edit visually, not script.
    // 2. `--url` + `--token` present → register with the cloud. Any other
    //    flags on this path take effect as part of the initial config file.
    // 3. Edit flags only (no `--token`) against an existing config → apply
    //    a partial mutation and kick the daemon.
    // 4. Edit flags without an existing config → error. We can't edit a
    //    file that doesn't exist, and we won't invent credentials.
    let bare = args.url.is_none()
        && args.token.is_none()
        && !args.has_edit_flags()
        && !args.skip_doctor
        && !args.skip_service
        && !args.skip_linger;
    if bare {
        return crate::tui::run(
            paths.clone(),
            /* no_onboarding = */ false,
            crate::tui::app::Tab::Config,
        )
        .await;
    }

    match (args.url.clone(), args.token.clone()) {
        (Some(url), Some(token)) => {
            let project = args.project.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "missing --project <PROJECT_IDENTIFIER> for registration. \
                     Use `pidash token list-projects` or the cloud's Projects page."
                )
            })?;
            let inputs = RegisterInputs {
                url,
                token,
                name: args.name.clone(),
                working_dir: args.working_dir.clone(),
                agent: args.agent,
                project,
                pod: args.pod.clone(),
                skip_doctor: args.skip_doctor,
                skip_service: args.skip_service,
                skip_linger: args.skip_linger,
                extras: Some(args),
            };
            execute(inputs, paths).await
        }
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!(
                "--url and --token must be used together. Pass both to re-register, \
                 or omit both to edit specific fields of an existing config."
            );
        }
        (None, None) => partial_edit(args, paths).await,
    }
}

/// Partial-edit path: load `config.toml`, apply whatever `--<flag>`s were
/// set, persist, and restart the daemon. Bails if the config doesn't exist
/// yet (fresh machines must register first).
async fn partial_edit(args: Args, paths: &Paths) -> Result<()> {
    // `--url` is intentionally not a partial-edit field: changing the cloud
    // URL without also re-minting credentials leaves stale `credentials.toml`
    // pointing at a URL it was never issued against, and the next restart
    // silently fails cloud auth. More importantly, blindly writing whatever
    // URL the user passes is an SSRF surface (e.g. private metadata IPs) —
    // the validation that guards the register path can't prevent that here
    // because changing cloud_url is never actually what the user wants. Re-
    // registering against the new cloud is the only safe way.
    if args.url.is_some() {
        anyhow::bail!(
            "--url can only be changed by re-registering: run `pidash configure --url <URL> --token <TOKEN>`. \
             Changing cloud_url alone would leave the runner's credentials bound to the old cloud."
        );
    }
    let mut cfg = match crate::config::file::load_config_opt(paths)? {
        Some(c) => c,
        None => {
            anyhow::bail!(
                "no config found at {}. Run `pidash configure --url <URL> --token <TOKEN>` \
                 to register this runner first.",
                paths.config_path().display()
            );
        }
    };
    if let Some(name) = &args.name {
        runner_name::validate(name).with_context(|| format!("invalid --name value {name:?}"))?;
    }
    if let Some(lvl) = &args.log_level {
        validate_log_level(lvl).with_context(|| format!("invalid --log-level value {lvl:?}"))?;
    }
    let changed = args.apply_to(&mut cfg);
    if !changed {
        println!("No config fields were changed.");
        return Ok(());
    }
    crate::config::file::write_config(paths, &cfg)?;
    println!("Wrote {}.", paths.config_path().display());

    if args.skip_service {
        println!("Skipping daemon restart (--skip-service).");
        return Ok(());
    }

    println!("Reloading runner daemon…");
    let outcome = crate::service::reload::restart_and_verify(paths).await;
    if outcome.ok {
        println!("✓ {}", outcome.summary);
        Ok(())
    } else {
        eprintln!("✗ {}", outcome.summary);
        if let Some(detail) = outcome.detail {
            eprintln!("\n{detail}");
        }
        anyhow::bail!("runner failed to come up cleanly after config change");
    }
}

/// End-to-end onboarding: register with the cloud, persist `config.toml` +
/// `credentials.toml`, run the doctor, then (unless `--skip-service`) write
/// the OS service unit and bring the daemon up. One command covers the
/// happy path for an interactive user; `--skip-service` peels off the last
/// step for scripted / CI flows that manage supervision themselves.
pub async fn execute(inputs: RegisterInputs, paths: &Paths) -> Result<()> {
    validate_cloud_url(&inputs.url)?;
    if let Some(extras) = inputs.extras.as_ref()
        && let Some(lvl) = &extras.log_level
    {
        validate_log_level(lvl).with_context(|| format!("invalid --log-level value {lvl:?}"))?;
    }

    // Pre-load any existing config so we can pre-fill the agent prompt with
    // the user's prior choice. Harmless if the file is absent or garbled —
    // `load_config_opt` swallows NotFound and we fall back to Codex.
    let existing_kind = crate::config::file::load_config_opt(paths)
        .ok()
        .flatten()
        .and_then(|c| c.runners.first().map(|r| r.agent.kind));
    let agent_kind = resolve_agent_kind(inputs.agent, existing_kind);

    // User-supplied names are charset-checked up front; an invalid `--name`
    // is a hard error, not something we try to fix by retrying. Auto-generated
    // names are charset-safe by construction.
    let user_supplied_name = inputs.name.is_some();
    if let Some(n) = &inputs.name {
        runner_name::validate(n).with_context(|| format!("invalid --name value {n:?}"))?;
    }

    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let version = crate::RUNNER_VERSION.to_string();

    // On auto-generated names, transparently retry if the cloud says the
    // name is already taken in this workspace. On user-supplied names, a
    // collision is a loud error — we don't silently rename what the user
    // typed. Hoist the user-supplied name outside the loop: it doesn't
    // change between attempts (and for user-supplied input the loop breaks
    // or bails on the first iteration anyway).
    let supplied_name = inputs.name.clone();
    let (resp, final_name) = {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let attempt_name = supplied_name
                .clone()
                .unwrap_or_else(runner_name::generate_default);
            let req = RegisterRequest {
                runner_name: attempt_name.clone(),
                os: os.clone(),
                arch: arch.clone(),
                version: version.clone(),
                protocol_version: crate::PROTOCOL_VERSION,
                project: inputs.project.clone(),
                pod: inputs.pod.clone(),
            };
            match register(&inputs.url, &inputs.token, &req).await {
                Ok(resp) => break (resp, attempt_name),
                Err(RegisterError::NameTaken)
                    if !user_supplied_name && attempts < MAX_AUTO_NAME_RETRIES =>
                {
                    tracing::info!(
                        attempt = attempts,
                        name = %attempt_name,
                        "auto-generated runner name already taken; retrying with a fresh suffix"
                    );
                    continue;
                }
                Err(RegisterError::NameTaken) if user_supplied_name => {
                    anyhow::bail!(
                        "runner name {attempt_name:?} is already taken in this workspace. \
                         Choose a different --name, or omit --name so the client generates a unique one."
                    );
                }
                Err(RegisterError::NameTaken) => {
                    anyhow::bail!(
                        "could not generate a unique runner name after {MAX_AUTO_NAME_RETRIES} attempts. \
                         This is extremely unlikely; check the cloud for stale runners, or pass --name explicitly."
                    );
                }
                Err(RegisterError::Other(e)) => {
                    return Err(e).context("cloud registration failed");
                }
            }
        }
    };

    let working_dir = inputs
        .working_dir
        .clone()
        .unwrap_or_else(|| paths.default_working_dir());

    // A new server always populates this. `None` means we just enrolled
    // against an older server — the daemon still works, but every CRUD
    // subcommand will fail until the user rerun against an updated server.
    // Surface that now instead of letting the first `pidash issue list`
    // produce a confusing error.
    if resp.workspace_slug.is_none() {
        eprintln!(
            "warning: server did not return a workspace_slug. \
             The daemon will run, but `pidash issue` subcommands will fail \
             until you rerun `pidash configure` against an updated server."
        );
    }

    let mut config = Config {
        version: 2,
        daemon: crate::config::schema::DaemonConfig {
            cloud_url: inputs.url.clone(),
            log_level: "info".to_string(),
            log_retention_days: 14,
        },
        runners: vec![crate::config::schema::RunnerConfig {
            name: final_name,
            runner_id: resp.runner_id,
            workspace_slug: resp.workspace_slug.clone(),
            // Prefer the cloud's echoed project identifier; fall back to
            // what the user passed (older servers don't echo). Either way
            // the value is non-empty post-registration.
            project_slug: Some(
                resp.project_identifier
                    .clone()
                    .unwrap_or_else(|| inputs.project.clone()),
            ),
            pod_id: resp.pod_id,
            workspace: crate::config::schema::WorkspaceSection { working_dir },
            agent: crate::config::schema::AgentSection { kind: agent_kind },
            codex: crate::config::schema::CodexSection::default(),
            claude_code: crate::config::schema::ClaudeCodeSection::default(),
            approval_policy: crate::config::schema::ApprovalPolicySection::default(),
        }],
    };
    // Apply any advanced field flags the user passed alongside --url/--token.
    // They're already reflected in `config` for the fields this function
    // populates directly (name, cloud_url, agent, working_dir); `apply_to`
    // covers the rest (approval_policy.*, logging.*).
    if let Some(extras) = inputs.extras.as_ref() {
        extras.apply_to(&mut config);
    }
    crate::config::file::write_config(paths, &config)?;

    let creds = Credentials {
        // No token block yet — `pidash configure` (the v1 enrollment flow)
        // only mints a runner_secret. Token-based auth is set up by
        // `pidash configure token` once cloud ships v2.
        token: None,
        runner_id: resp.runner_id,
        runner_secret: resp.runner_secret,
        api_token: resp.api_token,
        issued_at: chrono::Utc::now(),
    };
    crate::config::file::write_credentials(paths, &creds)?;

    if !inputs.skip_doctor {
        run_doctor_with_auth_gate(paths, agent_kind).await?;
    }

    println!(
        "\nRegistered runner '{}' with id {}.",
        config.primary_runner().name,
        creds.runner_id,
    );

    if inputs.skip_service {
        println!(
            "\nSkipping service setup (--skip-service). Run `pidash install` later to enable the background daemon.\n"
        );
        return Ok(());
    }

    // Happy path: user just ran `pidash configure` on an interactive machine.
    // Write the user-scoped service unit and bring the daemon up so they
    // don't also have to type `pidash install`. `enable_and_start` restarts
    // an already-running daemon so a re-configure picks up fresh creds.
    let svc = crate::service::detect();
    svc.write_unit(paths).await?;
    svc.enable_and_start().await?;
    let boot_outcome = if inputs.skip_linger {
        crate::service::BootStartOutcome::Skipped
    } else {
        svc.ensure_boot_start().await
    };
    print_post_install_hints(&boot_outcome);

    Ok(())
}

fn print_post_install_hints(boot: &crate::service::BootStartOutcome) {
    use crate::service::BootStartOutcome::*;
    println!("Service installed and running.");
    if cfg!(target_os = "linux") {
        println!();
        match boot {
            AlreadyEnabled => {
                println!("Linger is enabled — the service will start automatically at boot.");
            }
            Enabled => {
                println!("Enabled linger — the service will now start automatically at boot.");
            }
            NonInteractive => {
                println!("No TTY available; skipped enabling linger.");
                println!("To start the service at boot, run:");
                println!("  sudo loginctl enable-linger $USER");
            }
            Skipped => {
                println!("Skipped `loginctl enable-linger` (--skip-linger).");
                println!("Without lingering, the service only starts at login.");
                println!("To enable later, run:  sudo loginctl enable-linger $USER");
            }
            CheckFailed(err) => {
                println!("Couldn't check linger state ({err}).");
                println!("To start the service at boot, run:");
                println!("  sudo loginctl enable-linger $USER");
            }
            EnableFailed(err) => {
                println!("Couldn't enable linger ({err}).");
                println!("The service is running now but won't restart at boot until you run:");
                println!("  sudo loginctl enable-linger $USER");
            }
            NotApplicable => {}
        }
    }
    println!();
    println!("Useful next commands:");
    println!("  pidash status         # service + daemon state");
    println!("  pidash tui            # interactive UI");
    println!("  pidash stop           # stop the service");
    println!();
}

/// Picks the agent for this run. Precedence:
/// 1. `--agent` flag (always wins, scriptable).
/// 2. Interactive TTY arrow-key picker, pre-selecting the existing config's
///    kind if one exists (otherwise `codex`). Enter confirms, Esc keeps the
///    default.
/// 3. Non-TTY with no flag: keep the existing config's kind, or Codex.
fn resolve_agent_kind(flag: Option<AgentKind>, existing: Option<AgentKind>) -> AgentKind {
    if let Some(k) = flag {
        return k;
    }
    if std::io::stdin().is_terminal() {
        return prompt_agent_kind(existing.unwrap_or_default());
    }
    existing.unwrap_or_default()
}

/// Interactive agent picker. Renders a small list inline; Up/Down move the
/// cursor, Enter confirms, Esc keeps the pre-filled default. Needs raw mode
/// on stdout — if stdout isn't a TTY or raw mode can't be engaged we
/// silently fall back to `default` (resolve_agent_kind only calls us on a
/// stdin TTY, so this is the only remaining non-TTY path).
fn prompt_agent_kind(default: AgentKind) -> AgentKind {
    use crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{self, ClearType},
    };

    const OPTIONS: [(AgentKind, &str); 2] = [
        (AgentKind::Codex, "codex"),
        (AgentKind::ClaudeCode, "claude-code"),
    ];
    let mut idx = OPTIONS.iter().position(|(k, _)| *k == default).unwrap_or(0);

    if !std::io::stdout().is_terminal() {
        return default;
    }

    // Drop-guard restores cooked mode on every exit path — panic, early
    // return, normal flow. Without this a crash inside the loop leaves the
    // user's shell in raw mode and unusable.
    struct RawGuard;
    impl Drop for RawGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    if terminal::enable_raw_mode().is_err() {
        return default;
    }
    let guard = RawGuard;

    let mut out = std::io::stdout();
    // \r\n everywhere: raw mode turns off LF→CRLF translation.
    let _ = write!(
        out,
        "Choose AI agent (↑/↓ to move, Enter to confirm, Esc for default):\r\n"
    );
    let render = |out: &mut std::io::Stdout, idx: usize| {
        for (i, (_, label)) in OPTIONS.iter().enumerate() {
            let marker = if i == idx { ">" } else { " " };
            let _ = write!(out, "{marker} {label}\r\n");
        }
        let _ = out.flush();
    };
    render(&mut out, idx);

    let selected = loop {
        match event::read() {
            Ok(Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press,
                ..
            })) => match code {
                KeyCode::Up => {
                    idx = if idx == 0 { OPTIONS.len() - 1 } else { idx - 1 };
                }
                KeyCode::Down => {
                    idx = (idx + 1) % OPTIONS.len();
                }
                KeyCode::Enter => break OPTIONS[idx].0,
                KeyCode::Esc => break default,
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    // Raw mode disables the kernel's SIGINT translation, so
                    // Ctrl-C reaches us as a keystroke. Restore cooked mode
                    // before exiting — `process::exit` doesn't run Drops.
                    drop(guard);
                    std::process::exit(130);
                }
                _ => continue,
            },
            Ok(_) => continue,
            Err(_) => break default,
        }
        let _ = execute!(
            out,
            cursor::MoveUp(OPTIONS.len() as u16),
            terminal::Clear(ClearType::FromCursorDown),
        );
        render(&mut out, idx);
    };

    // Collapse the picker's rendered lines into a single summary so the
    // scrollback stays tidy regardless of how many arrow presses happened.
    let selected_label = OPTIONS
        .iter()
        .find(|(k, _)| *k == selected)
        .map(|(_, l)| *l)
        .unwrap_or("codex");
    let _ = execute!(
        out,
        cursor::MoveUp(OPTIONS.len() as u16 + 1),
        terminal::Clear(ClearType::FromCursorDown),
    );
    let _ = write!(out, "Choose AI agent: {selected_label}\r\n");
    let _ = out.flush();

    selected
}

/// Runs the doctor and, on a TTY, loops on a failing agent-auth blocker so
/// the operator can authenticate in another terminal and continue without
/// re-running `pidash configure`. Non-TTY keeps the old warn-and-continue
/// behaviour so scripts don't deadlock.
async fn run_doctor_with_auth_gate(paths: &Paths, agent_kind: AgentKind) -> Result<()> {
    let auth_check_name = match agent_kind {
        AgentKind::Codex => "codex-auth",
        AgentKind::ClaudeCode => "claude-auth",
    };
    let tty = std::io::stdin().is_terminal();

    loop {
        let report = crate::cli::doctor::execute(paths, None).await?;
        report.print_compact();

        // In multi-runner installs, the doctor tags per-runner checks as
        // `<base>@<runner-name>` (e.g. `codex-auth@laptop`). Match the
        // base name as a prefix so we still catch any auth-gated runner.
        let auth_failing = report.checks.iter().any(|c| {
            (c.name == auth_check_name
                || c.name.starts_with(&format!("{auth_check_name}@")))
                && c.blocker
                && !c.ok
        });

        if !auth_failing {
            if report.has_blockers() {
                eprintln!("\nWarning: some preflight checks failed. Resolve them before starting.");
            }
            return Ok(());
        }

        if !tty {
            eprintln!("\nWarning: some preflight checks failed. Resolve them before starting.");
            return Ok(());
        }

        let login_hint = match agent_kind {
            AgentKind::Codex => "codex login",
            AgentKind::ClaudeCode => "claude /login",
        };
        print!(
            "\nAgent auth check failed. Run `{login_hint}` in another terminal, then press Enter to retry (Ctrl-C to finish setup later): "
        );
        std::io::stdout().flush()?;
        let stdin = std::io::stdin();
        let mut line = String::new();
        // EOF (0 bytes) = user closed stdin / piped-in empty input; treat as
        // "give up" rather than a busy-loop. Matches Ctrl-C semantics.
        if stdin.lock().read_line(&mut line)? == 0 {
            eprintln!("\nFinishing setup without auth; resolve it before starting the runner.");
            return Ok(());
        }
    }
}

/// Accepted values for `[logging].level`. The TUI's editable Config tab
/// already constrains this via an enum picker; the CLI mirrors the same set
/// so a script can't silently wedge the daemon with e.g. `--log-level foo`
/// (EnvFilter would reject it on next restart and the daemon wouldn't come
/// back up).
const CLI_LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

pub(crate) fn validate_log_level(level: &str) -> Result<()> {
    if CLI_LOG_LEVELS.iter().any(|v| v.eq_ignore_ascii_case(level)) {
        return Ok(());
    }
    anyhow::bail!("log level must be one of: {}", CLI_LOG_LEVELS.join(", "))
}

/// Refuse `http://` URLs that point at non-localhost hosts. Sending the
/// registration token + receiving the runner secret over cleartext to the
/// internet would silently leak credentials. Localhost is allowed for dev.
pub(crate) fn validate_cloud_url(url: &str) -> Result<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            tracing::warn!("using cleartext http:// to {host} — only suitable for development");
            return Ok(());
        }
        anyhow::bail!(
            "refusing to register over cleartext http:// to non-localhost ({host}); use https://"
        );
    }
    anyhow::bail!("cloud URL must start with https:// (or http:// for localhost), got {url}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_agent_kind_flag_wins_over_existing() {
        // We can't assert the TTY branch from a unit test, but we can assert
        // that an explicit `--agent` flag bypasses both the prompt and the
        // existing-config fallback — which is the important guarantee for
        // non-interactive callers.
        let got = resolve_agent_kind(Some(AgentKind::Codex), Some(AgentKind::ClaudeCode));
        assert_eq!(got, AgentKind::Codex);
    }

    #[test]
    fn validate_log_level_accepts_canonical_values() {
        for lvl in ["trace", "debug", "info", "warn", "error"] {
            assert!(
                validate_log_level(lvl).is_ok(),
                "expected {lvl:?} to validate"
            );
        }
    }

    #[test]
    fn validate_log_level_case_insensitive() {
        assert!(validate_log_level("DEBUG").is_ok());
        assert!(validate_log_level("Info").is_ok());
    }

    #[test]
    fn validate_log_level_rejects_garbage() {
        let err = validate_log_level("chatty").unwrap_err().to_string();
        assert!(
            err.contains("trace") && err.contains("error"),
            "error should list allowed values, got: {err}"
        );
    }

    #[test]
    fn validate_cloud_url_rejects_non_localhost_http() {
        let err = validate_cloud_url("http://evil.example.com")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cleartext"));
    }

    #[test]
    fn validate_cloud_url_allows_https_and_localhost_http() {
        assert!(validate_cloud_url("http://localhost").is_ok());
        assert!(validate_cloud_url("http://localhost:3000").is_ok());
        assert!(validate_cloud_url("http://127.0.0.1:3000").is_ok());
    }
}
