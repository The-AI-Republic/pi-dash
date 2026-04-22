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
    /// Pi Dash cloud base URL (https://cloud.pidash.so).
    #[arg(long)]
    pub url: String,

    /// Registration token issued by the cloud UI.
    #[arg(long)]
    pub token: String,

    /// Optional human-friendly name for this runner.
    #[arg(long)]
    pub name: Option<String>,

    /// Override the workspace directory.
    #[arg(long)]
    pub working_dir: Option<PathBuf>,

    /// Which AI agent CLI the runner should drive. Omit on a TTY to get the
    /// arrow-key picker (pre-selects your previous choice, or `codex` on
    /// first run). Required to run non-interactively with a non-default choice.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Skip on-install doctor checks (not recommended). Also skips the
    /// auth-gate retry loop, since there's nothing to re-check.
    #[arg(long)]
    pub skip_doctor: bool,

    /// Skip installing / starting the OS service at the end. Use from CI or
    /// Ansible playbooks that manage the daemon lifecycle themselves, or when
    /// you only want to re-register credentials without bouncing a running
    /// daemon.
    #[arg(long)]
    pub skip_service: bool,
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
    pub skip_doctor: bool,
    pub skip_service: bool,
}

impl From<Args> for RegisterInputs {
    fn from(a: Args) -> Self {
        Self {
            url: a.url,
            token: a.token,
            name: a.name,
            working_dir: a.working_dir,
            agent: a.agent,
            skip_doctor: a.skip_doctor,
            skip_service: a.skip_service,
        }
    }
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    execute(args.into(), paths).await
}

/// End-to-end onboarding: register with the cloud, persist `config.toml` +
/// `credentials.toml`, run the doctor, then (unless `--skip-service`) write
/// the OS service unit and bring the daemon up. One command covers the
/// happy path for an interactive user; `--skip-service` peels off the last
/// step for scripted / CI flows that manage supervision themselves.
pub async fn execute(inputs: RegisterInputs, paths: &Paths) -> Result<()> {
    validate_cloud_url(&inputs.url)?;

    // Pre-load any existing config so we can pre-fill the agent prompt with
    // the user's prior choice. Harmless if the file is absent or garbled —
    // `load_config_opt` swallows NotFound and we fall back to Codex.
    let existing_kind = crate::config::file::load_config_opt(paths)
        .ok()
        .flatten()
        .map(|c| c.agent.kind);
    let agent_kind = resolve_agent_kind(inputs.agent, existing_kind);

    // User-supplied names are charset-checked up front; an invalid `--name`
    // is a hard error, not something we try to fix by retrying. Auto-generated
    // names are charset-safe by construction.
    let user_supplied_name = inputs.name.is_some();
    if let Some(n) = &inputs.name {
        runner_name::validate(n)
            .with_context(|| format!("invalid --name value {n:?}"))?;
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

    let config = Config {
        version: 1,
        runner: crate::config::schema::RunnerSection {
            name: final_name,
            cloud_url: inputs.url.clone(),
            workspace_slug: resp.workspace_slug.clone(),
        },
        workspace: crate::config::schema::WorkspaceSection { working_dir },
        codex: crate::config::schema::CodexSection::default(),
        claude_code: crate::config::schema::ClaudeCodeSection::default(),
        agent: crate::config::schema::AgentSection { kind: agent_kind },
        approval_policy: crate::config::schema::ApprovalPolicySection::default(),
        logging: crate::config::schema::LoggingSection::default(),
    };
    crate::config::file::write_config(paths, &config)?;

    let creds = Credentials {
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
        config.runner.name, creds.runner_id,
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
    print_post_install_hints();

    Ok(())
}

fn print_post_install_hints() {
    println!("Service installed and running.");
    if cfg!(target_os = "linux") {
        println!();
        println!("For the service to start on OS boot (before you log in), run:");
        println!("  sudo loginctl enable-linger $USER");
        println!(
            "Without lingering, the service still starts at every user login and restarts on crash."
        );
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
    let mut idx = OPTIONS
        .iter()
        .position(|(k, _)| *k == default)
        .unwrap_or(0);

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
        let report = crate::cli::doctor::execute(paths).await?;
        report.print_compact();

        let auth_failing = report
            .checks
            .iter()
            .any(|c| c.name == auth_check_name && c.blocker && !c.ok);

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

/// Refuse `http://` URLs that point at non-localhost hosts. Sending the
/// registration token + receiving the runner secret over cleartext to the
/// internet would silently leak credentials. Localhost is allowed for dev.
fn validate_cloud_url(url: &str) -> Result<()> {
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
}

