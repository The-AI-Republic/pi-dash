use anyhow::Result;
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

use crate::config::file;
use crate::util::paths::Paths;
use crate::util::shell::login_shell_command;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Emit a machine-readable JSON report.
    #[arg(long)]
    pub json: bool,
    /// Restrict per-runner checks (agent binary, agent auth) to the named
    /// runner. Without it, doctor walks every `[[runner]]` block and tags
    /// each result with the runner name. Daemon-wide checks (git, cloud
    /// reachability) always run once.
    #[arg(long)]
    pub runner: Option<String>,
}

// The doctor `Report`/`Check` DTOs moved to the shared `pidash-ipc` crate
// (PDASHOSS01-158) because the IPC `Response::Doctor` variant carries a
// `Report`. Re-exported here so `cli::doctor::{Report, Check}` call sites are
// unchanged.
pub use pidash_ipc::dto::{Check, Report};

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let report = execute(paths, args.runner.as_deref()).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.print_compact();
    }
    if report.has_blockers() {
        std::process::exit(2);
    }
    Ok(())
}

pub async fn execute(paths: &Paths, runner_filter: Option<&str>) -> Result<Report> {
    let mut checks = Vec::new();

    // Per-runner checks (agent binary, agent auth) walk every configured
    // runner so multi-runner installs get every agent path verified. With
    // `--runner <name>`, only that runner is checked. Pre-`configure`
    // installs (no config on disk) fall through to a single Codex probe so
    // `pidash doctor` keeps working immediately after install.
    let cfg = file::load_config_opt(paths)?;
    let runners: Vec<&crate::config::schema::RunnerConfig> = match cfg.as_ref() {
        Some(c) => match runner_filter {
            Some(name) => match c.runners.iter().find(|r| r.name == name) {
                Some(r) => vec![r],
                None => {
                    let known: Vec<&str> = c.runners.iter().map(|r| r.name.as_str()).collect();
                    anyhow::bail!(
                        "no runner named {name:?} in config; configured: [{}]",
                        known.join(", ")
                    );
                }
            },
            None => c.runners.iter().collect(),
        },
        None => Vec::new(),
    };

    // Tag check names with `@<runner>` whenever there's any ambiguity:
    // multiple runners walked, *or* an explicit filter narrowed the walk
    // (so `pidash doctor --runner beta` clearly shows `codex@beta` rather
    // than a bare `codex` that hides which runner ran).
    let multi = runners.len() > 1 || runner_filter.is_some();
    if runners.is_empty() {
        // No config — historical default. Probe Codex unattended so
        // operators get useful feedback before `pidash configure` runs.
        run_agent_checks(
            &mut checks,
            None,
            crate::config::schema::AgentKind::Codex,
            "codex",
            "claude",
            "cursor-agent",
            "acpx",
            "grok",
            "muse",
        )
        .await;
    } else {
        for r in &runners {
            let prefix = if multi { Some(r.name.as_str()) } else { None };
            run_agent_checks(
                &mut checks,
                prefix,
                r.agent.kind,
                &r.codex.binary,
                &r.claude_code.binary,
                &r.cursor_agent.binary,
                &r.openclaw.binary,
                &r.grok.binary,
                &r.muse_code.binary,
            )
            .await;
        }
    }

    // git.
    match check_git().await {
        Ok(detail) => checks.push(Check {
            name: "git".to_string(),
            ok: true,
            detail,
            blocker: true,
        }),
        Err(e) => checks.push(Check {
            name: "git".to_string(),
            ok: false,
            detail: e.to_string(),
            blocker: true,
        }),
    }

    // Cloud reachability (if we have config).
    if let Some(cfg) = cfg.as_ref() {
        match check_cloud(&cfg.daemon.cloud_url).await {
            Ok(detail) => checks.push(Check {
                name: "network".to_string(),
                ok: true,
                detail,
                blocker: false,
            }),
            Err(e) => checks.push(Check {
                name: "network".to_string(),
                ok: false,
                detail: e.to_string(),
                blocker: false,
            }),
        }
    }

    Ok(Report { checks })
}

/// Run the agent binary + auth checks for one runner. `prefix` is `Some(name)`
/// in multi-runner installs so the report distinguishes which runner failed
/// — `codex@laptop` vs `codex@build-box` — and `None` in the single-runner
/// case to keep output identical to the pre-multi-runner format.
// One `&str` binary name per supported agent, so the arity grows with the
// agent list; a struct wrapper would add ceremony without clarifying the call
// sites, which pass the runner's per-agent binaries positionally.
#[allow(clippy::too_many_arguments)]
async fn run_agent_checks(
    checks: &mut Vec<Check>,
    prefix: Option<&str>,
    agent_kind: crate::config::schema::AgentKind,
    codex_binary: &str,
    claude_binary: &str,
    cursor_binary: &str,
    openclaw_binary: &str,
    grok_binary: &str,
    muse_binary: &str,
) {
    let tag = |base: &str| match prefix {
        Some(p) => format!("{base}@{p}"),
        None => base.to_string(),
    };
    match agent_kind {
        crate::config::schema::AgentKind::Codex => {
            match check_version(codex_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("codex"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("codex"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            match check_codex_auth(codex_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("codex-auth"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("codex-auth"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
        }
        crate::config::schema::AgentKind::ClaudeCode => {
            match check_version(claude_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("claude"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("claude"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            let probe = gather_claude_auth_probe(claude_binary).await;
            checks.push(
                classify_claude_auth(&probe)
                    .with_fix(claude_binary)
                    .into_check(tag("claude-auth")),
            );
        }
        crate::config::schema::AgentKind::CursorAgent => {
            match check_version(cursor_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("cursor-agent"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("cursor-agent"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // cursor-agent auth is interactive (`cursor-agent login`) or via the
            // CURSOR_API_KEY env var. The env var is a cheap local check; an
            // interactive login has no probe we trust, so without the key we
            // keep the neutral hint rather than warn.
            checks.push(
                env_key_auth(
                    "CURSOR_API_KEY",
                    std::env::var("CURSOR_API_KEY").ok(),
                    format!(
                        "assumed ok (run `{} login` or set CURSOR_API_KEY if runs fail \
                         with auth errors)",
                        cursor_binary
                    ),
                )
                .into_check(tag("cursor-auth")),
            );
        }
        crate::config::schema::AgentKind::OpenClaw => {
            match check_version(openclaw_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("acpx"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("acpx"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // OpenClaw needs two binaries: `acpx` (probed above, the ACP client
            // the runner spawns) and `openclaw` itself, which acpx launches for
            // `openclaw exec`. Presence of `openclaw` is cheaply probeable, so
            // check it directly — a machine with acpx but no openclaw would
            // otherwise pass doctor yet fail every run. Keep it non-blocking:
            // an older openclaw that doesn't print a `--version` line shouldn't
            // hard-fail doctor, and auth + a reachable OpenClaw Gateway still
            // have no cheap non-interactive probe (so they stay a hint).
            let openclaw_runnable = crate::util::shell::binary_runs_version("openclaw").await;
            checks.push(Check {
                name: tag("openclaw"),
                ok: openclaw_runnable,
                detail: if openclaw_runnable {
                    "openclaw CLI runnable (ensure it's authenticated and an \
                     OpenClaw Gateway is reachable if runs fail)"
                        .to_string()
                } else {
                    "openclaw CLI not runnable on PATH — acpx launches it for \
                     `openclaw exec`, so install OpenClaw on this machine and \
                     ensure a Gateway is reachable"
                        .to_string()
                },
                blocker: false,
            });
        }
        crate::config::schema::AgentKind::Grok => {
            match check_version(grok_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("grok"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("grok"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // grok auth is via the `XAI_API_KEY` env var or an interactive
            // `grok` login. Report the env var when set; an interactive login
            // has no cheap probe, so otherwise keep the neutral hint.
            checks.push(
                env_key_auth(
                    "XAI_API_KEY",
                    std::env::var("XAI_API_KEY").ok(),
                    format!(
                        "assumed ok (set XAI_API_KEY or run `{grok_binary}` login if runs \
                         fail with auth errors)"
                    ),
                )
                .into_check(tag("grok-auth")),
            );
        }
        crate::config::schema::AgentKind::MuseCode => {
            match check_version(muse_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("muse"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("muse"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // Muse Code auth is via the `META_API_KEY` env var. Report it when
            // set. When unset here it may still come from the login shell the
            // daemon spawns the agent through, so stay neutral rather than warn.
            checks.push(
                env_key_auth(
                    "META_API_KEY",
                    std::env::var("META_API_KEY").ok(),
                    "assumed ok (set META_API_KEY if runs fail with auth errors)".to_string(),
                )
                .into_check(tag("muse-auth")),
            );
        }
    }
}

/// Shared `<binary> --version` check used for every supported agent binary
/// (`codex`, `claude`, `cursor-agent`, and `acpx`), each of which prints a
/// short version line on stdout and exits 0 on success.
///
/// Runs through the same platform spawn helper the daemon uses so this check
/// reflects what the daemon will actually see at spawn time.
async fn check_version(binary: &str) -> Result<String> {
    let mut cmd = login_shell_command(binary, &["--version"], None);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = cmd.output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "{binary} --version exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(format!(
        "{} ({})",
        String::from_utf8_lossy(&out.stdout).trim(),
        binary
    ))
}

async fn check_codex_auth(binary: &str) -> Result<String> {
    // codex has `account` as a subcommand in newer releases; fall back to `whoami`.
    let mut cmd = login_shell_command(binary, &["account", "status"], None);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = cmd.output().await.ok();
    if let Some(o) = out
        && o.status.success()
    {
        return Ok(String::from_utf8_lossy(&o.stdout).trim().to_string());
    }
    anyhow::bail!(
        "unable to confirm Codex auth; run `{} login` before starting the runner",
        binary
    )
}

/// Outcome of a cheap, local agent-auth check. Never derived from a model
/// call: doctor must not spend tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthStatus {
    /// Credentials were found. Reported as a pass.
    LoggedIn(String),
    /// We looked and found nothing. A warning (`ok: false`) but never a
    /// blocker: the operator gets the fix, doctor still exits 0.
    NotLoggedIn(String),
    /// We could not tell (e.g. macOS Keychain, an unrecognised credentials
    /// file, a third-party provider). Neutral: reported as a pass with a hint.
    Unknown(String),
}

impl AuthStatus {
    /// Append the Claude-specific fix to non-passing outcomes.
    fn with_fix(self, claude_binary: &str) -> Self {
        match self {
            AuthStatus::LoggedIn(d) => AuthStatus::LoggedIn(d),
            AuthStatus::NotLoggedIn(d) => AuthStatus::NotLoggedIn(format!(
                "not logged in: {d}; run `{claude_binary} /login` (or set ANTHROPIC_API_KEY)"
            )),
            AuthStatus::Unknown(d) => AuthStatus::Unknown(format!(
                "could not verify: {d}; run `{claude_binary} /login` if runs fail with auth errors"
            )),
        }
    }

    fn into_check(self, name: String) -> Check {
        let (ok, detail) = match self {
            AuthStatus::LoggedIn(d) => (true, d),
            AuthStatus::NotLoggedIn(d) => (false, d),
            AuthStatus::Unknown(d) => (true, d),
        };
        Check {
            name,
            ok,
            detail,
            blocker: false,
        }
    }
}

/// Env-var-only auth check for agents whose sole cheap local signal is an API
/// key variable (Cursor, Grok, Muse). A set key is reported; an unset key keeps
/// `fallback` as a neutral hint because those agents may also be logged in
/// interactively or get the key from the login shell.
fn env_key_auth(var: &str, value: Option<String>, fallback: String) -> AuthStatus {
    match value {
        Some(v) if !v.trim().is_empty() => AuthStatus::LoggedIn(format!("{var} is set")),
        _ => AuthStatus::Unknown(fallback),
    }
}

/// Env vars Claude Code accepts as credentials, in the order it documents them.
const CLAUDE_AUTH_ENV_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

/// Env vars that route Claude Code to a cloud provider whose credentials we
/// cannot cheaply verify.
const CLAUDE_THIRD_PARTY_ENV_VARS: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// What `claude auth status` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeStatus {
    logged_in: bool,
    auth_method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialsFile {
    /// Holds a `claudeAiOauth` entry.
    Found(PathBuf),
    /// Exists but is not in a format we recognise.
    Unrecognized(PathBuf),
    Missing,
}

/// Everything the Claude auth decision depends on, gathered up front so the
/// decision itself ([`classify_claude_auth`]) is pure and unit-testable.
#[derive(Debug, Clone)]
struct ClaudeAuthProbe {
    /// `None` when the CLI has no `auth status` subcommand or its output
    /// could not be parsed.
    status: Option<ClaudeStatus>,
    /// First credential env var that is set and non-empty.
    env_var: Option<&'static str>,
    /// First third-party-provider switch that is set.
    third_party_provider: Option<&'static str>,
    credentials: CredentialsFile,
    is_macos: bool,
}

fn classify_claude_auth(p: &ClaudeAuthProbe) -> AuthStatus {
    // Claude Code's own status command reads every credential source it
    // supports (env, credentials file, macOS Keychain, apiKeyHelper), so its
    // answer wins when we have one.
    if let Some(status) = &p.status {
        if status.logged_in {
            let method = status.auth_method.as_deref().unwrap_or("unknown method");
            return AuthStatus::LoggedIn(format!("logged in ({method})"));
        }
        if let Some(var) = p.third_party_provider {
            return AuthStatus::Unknown(format!(
                "{var} is set; cloud-provider credentials are not checked"
            ));
        }
        return AuthStatus::NotLoggedIn("`claude auth status` reports no login".to_string());
    }
    if let Some(var) = p.env_var {
        return AuthStatus::LoggedIn(format!("{var} is set"));
    }
    if let Some(var) = p.third_party_provider {
        return AuthStatus::Unknown(format!(
            "{var} is set; cloud-provider credentials are not checked"
        ));
    }
    match &p.credentials {
        CredentialsFile::Found(path) => {
            AuthStatus::LoggedIn(format!("credentials found ({})", path.display()))
        }
        CredentialsFile::Unrecognized(path) => AuthStatus::Unknown(format!(
            "unrecognised credentials file ({})",
            path.display()
        )),
        // On macOS Claude Code keeps its login in the Keychain, so a missing
        // file says nothing.
        CredentialsFile::Missing if p.is_macos => AuthStatus::Unknown(
            "credentials are kept in the macOS Keychain, which doctor does not read".to_string(),
        ),
        CredentialsFile::Missing => {
            AuthStatus::NotLoggedIn("no auth env var and no credentials file".to_string())
        }
    }
}

/// Parse `claude auth status` JSON. Tolerates login-shell noise around the
/// object. Returns `None` for anything without a boolean `loggedIn`.
fn parse_claude_status(stdout: &str) -> Option<ClaudeStatus> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    if end < start {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&stdout[start..=end]).ok()?;
    Some(ClaudeStatus {
        logged_in: v.get("loggedIn")?.as_bool()?,
        auth_method: v
            .get("authMethod")
            .and_then(|m| m.as_str())
            .map(str::to_string),
    })
}

/// True when `claude auth status --help` printed that subcommand's own usage.
/// An older CLI without it prints the top-level help instead; running
/// `claude auth status` there would treat the words as a prompt.
fn supports_auth_status(help_output: &str) -> bool {
    help_output.contains("claude auth status")
}

fn claude_credentials_path(
    config_dir: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    match config_dir {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir).join(".credentials.json")),
        _ => home.map(|h| h.join(".claude").join(".credentials.json")),
    }
}

fn read_credentials_file(path: &Path) -> CredentialsFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return if path.exists() {
            CredentialsFile::Unrecognized(path.to_path_buf())
        } else {
            CredentialsFile::Missing
        };
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if v.get("claudeAiOauth").is_some_and(|o| o.is_object()) => {
            CredentialsFile::Found(path.to_path_buf())
        }
        _ => CredentialsFile::Unrecognized(path.to_path_buf()),
    }
}

/// Run a short, non-model `claude` subcommand through the daemon's spawn
/// helper with a timeout and no stdin. Returns `(stdout, stderr)`, or `None`
/// if it could not be spawned or did not finish in time.
async fn run_claude_quietly(binary: &str, args: &[&str]) -> Option<(String, String)> {
    let mut cmd = login_shell_command(binary, args, None);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .ok()?
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

async fn claude_auth_status(binary: &str) -> Option<ClaudeStatus> {
    let (help_out, help_err) = run_claude_quietly(binary, &["auth", "status", "--help"]).await?;
    if !supports_auth_status(&format!("{help_out}\n{help_err}")) {
        return None;
    }
    // `auth status` exits 1 when logged out, so read the JSON, not the code.
    let (out, _) = run_claude_quietly(binary, &["auth", "status"]).await?;
    parse_claude_status(&out)
}

/// What the agent's environment holds, as far as the Claude auth fallback
/// cares. Read from the same login shell the daemon spawns the agent
/// through, so keys exported by shell rc files count. Values of credential
/// variables are never read back, only whether they are set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AgentEnvSnapshot {
    /// Names from [`CLAUDE_AUTH_ENV_VARS`] / [`CLAUDE_THIRD_PARTY_ENV_VARS`]
    /// that are set and non-blank.
    set_vars: Vec<String>,
    config_dir: Option<String>,
    home: Option<String>,
}

const ENV_SET_MARK: &str = "__pidash_doctor_set ";
const ENV_CFG_MARK: &str = "__pidash_doctor_cfg ";
const ENV_HOME_MARK: &str = "__pidash_doctor_home ";

/// POSIX sh script printing [`AgentEnvSnapshot`] as marked lines. Marker
/// prefixes let us ignore whatever the user's rc files print.
fn agent_env_script() -> String {
    let vars: Vec<&str> = CLAUDE_AUTH_ENV_VARS
        .iter()
        .chain(CLAUDE_THIRD_PARTY_ENV_VARS)
        .copied()
        .collect();
    format!(
        "for v in {vars}; do eval \"x=\\${{$v-}}\"; case \"$x\" in *[![:space:]]*) \
         printf '{ENV_SET_MARK}%s\\n' \"$v\";; esac; done; \
         printf '{ENV_CFG_MARK}%s\\n' \"${{CLAUDE_CONFIG_DIR-}}\"; \
         printf '{ENV_HOME_MARK}%s\\n' \"${{HOME-}}\"",
        vars = vars.join(" ")
    )
}

/// Parse [`agent_env_script`] output. `None` unless the script clearly ran
/// (the HOME marker is present), so a broken shell falls back rather than
/// reading as "nothing set".
fn parse_agent_env(stdout: &str) -> Option<AgentEnvSnapshot> {
    let mut snap = AgentEnvSnapshot::default();
    let mut saw_home = false;
    let non_empty = |v: &str| (!v.is_empty()).then(|| v.to_string());
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix(ENV_SET_MARK) {
            snap.set_vars.push(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix(ENV_CFG_MARK) {
            snap.config_dir = non_empty(v);
        } else if let Some(v) = line.strip_prefix(ENV_HOME_MARK) {
            snap.home = non_empty(v);
            saw_home = true;
        }
    }
    saw_home.then_some(snap)
}

/// The doctor process's own environment, used when the login shell cannot
/// be read (and on Windows, where agents are spawned without a shell).
fn process_env_snapshot() -> AgentEnvSnapshot {
    let set = |var: &&str| std::env::var(var).is_ok_and(|v| !v.trim().is_empty());
    AgentEnvSnapshot {
        set_vars: CLAUDE_AUTH_ENV_VARS
            .iter()
            .chain(CLAUDE_THIRD_PARTY_ENV_VARS)
            .filter(|v| set(v))
            .map(|v| v.to_string())
            .collect(),
        config_dir: std::env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .filter(|v| !v.is_empty()),
        home: directories::BaseDirs::new().map(|b| b.home_dir().display().to_string()),
    }
}

async fn agent_env_snapshot() -> AgentEnvSnapshot {
    if cfg!(target_os = "windows") {
        return process_env_snapshot();
    }
    let script = agent_env_script();
    match run_claude_quietly("sh", &["-c", &script]).await {
        Some((out, _)) => parse_agent_env(&out).unwrap_or_else(process_env_snapshot),
        None => process_env_snapshot(),
    }
}

/// Build the probe from Claude's status answer (if any) and the agent's
/// environment.
fn build_claude_probe(
    status: Option<ClaudeStatus>,
    env: &AgentEnvSnapshot,
    is_macos: bool,
) -> ClaudeAuthProbe {
    let find = |list: &[&'static str]| {
        list.iter()
            .copied()
            .find(|v| env.set_vars.iter().any(|s| s == v))
    };
    let credentials = claude_credentials_path(
        env.config_dir.clone().map(Into::into),
        env.home.clone().map(PathBuf::from),
    )
    .map(|p| read_credentials_file(&p))
    .unwrap_or(CredentialsFile::Missing);
    ClaudeAuthProbe {
        status,
        env_var: find(CLAUDE_AUTH_ENV_VARS),
        third_party_provider: find(CLAUDE_THIRD_PARTY_ENV_VARS),
        credentials,
        is_macos,
    }
}

async fn gather_claude_auth_probe(binary: &str) -> ClaudeAuthProbe {
    let status = claude_auth_status(binary).await;
    let env = agent_env_snapshot().await;
    build_claude_probe(status, &env, cfg!(target_os = "macos"))
}

async fn check_git() -> Result<String> {
    let out = Command::new("git").arg("--version").output().await?;
    if !out.status.success() {
        anyhow::bail!("git not available");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn check_cloud(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let probe = format!("{}/api/v1/runner/health/", url.trim_end_matches('/'));
    let resp = client.get(&probe).send().await?;
    Ok(format!("{} ({})", resp.status(), probe))
}

#[cfg(test)]
mod tests {
    //! These tests exercise the *check-tagging* logic in `execute`, not
    //! the underlying binary probes. They use deliberately bogus binary
    //! names so `check_version` always errs; we then assert that the
    //! resulting `Check` entries are tagged with the right runner name.
    //! The actual `git --version` / cloud probe still runs and may pass or
    //! fail depending on the test environment — we only check that the
    //! per-runner tags are correct.
    use super::*;
    use crate::config::schema::{
        AgentKind, ClaudeCodeSection, CodexSection, Config, CursorAgentSection, DaemonConfig,
        GrokSection, MuseCodeSection, OpenClawSection, RunnerConfig, WorkspaceSection,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("config"),
            data_dir: dir.path().join("data"),
            runtime_dir: dir.path().join("runtime"),
            machine_context_dir: dir.path().join("config"),
        };
        paths.ensure().unwrap();
        (dir, paths)
    }

    fn runner(name: &str, codex_binary: &str) -> RunnerConfig {
        RunnerConfig {
            name: name.to_string(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("WS".into()),
            project_slug: Some("PRJ".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: PathBuf::from("/tmp/pi-dash-doctor-test"),
            },
            agent: Default::default(),
            codex: CodexSection {
                binary: codex_binary.to_string(),
                ..Default::default()
            },
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            openclaw: OpenClawSection::default(),
            grok: GrokSection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: Default::default(),
        }
    }

    fn cfg_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "http://127.0.0.1:1".into(),
                dev_machine_id: None,
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
                agent_watchdog: Default::default(),
                auto_update: true,
            },
            runners,
            cli: None,
        }
    }

    #[tokio::test]
    async fn execute_with_two_runners_tags_each_runner_in_check_names() {
        let (_dir, paths) = temp_paths();
        let cfg = cfg_with(vec![
            // Use deliberately missing binaries so the codex check fails
            // and we don't depend on whatever's on $PATH in CI.
            runner("alpha", "codex-not-installed-alpha"),
            runner("beta", "codex-not-installed-beta"),
        ]);
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let report = execute(&paths, None).await.unwrap();
        let names: Vec<String> = report.checks.iter().map(|c| c.name.clone()).collect();
        // Both per-runner agent checks must appear, tagged.
        assert!(
            names.iter().any(|n| n == "codex@alpha"),
            "expected codex@alpha in {names:?}",
        );
        assert!(
            names.iter().any(|n| n == "codex@beta"),
            "expected codex@beta in {names:?}",
        );
    }

    #[tokio::test]
    async fn execute_with_single_runner_keeps_untagged_check_names() {
        let (_dir, paths) = temp_paths();
        let cfg = cfg_with(vec![runner("solo", "codex-not-installed")]);
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let report = execute(&paths, None).await.unwrap();
        let names: Vec<String> = report.checks.iter().map(|c| c.name.clone()).collect();
        // Single-runner installs keep the bare name so existing scripts
        // and snapshots don't break.
        assert!(
            names.iter().any(|n| n == "codex"),
            "expected bare `codex` in {names:?}",
        );
        assert!(
            !names.iter().any(|n| n.starts_with("codex@")),
            "did not expect tagged names in single-runner: {names:?}",
        );
    }

    #[tokio::test]
    async fn execute_with_runner_filter_restricts_checks_to_that_runner() {
        let (_dir, paths) = temp_paths();
        let cfg = cfg_with(vec![
            runner("alpha", "codex-not-installed-alpha"),
            runner("beta", "codex-not-installed-beta"),
        ]);
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let report = execute(&paths, Some("beta")).await.unwrap();
        let names: Vec<String> = report.checks.iter().map(|c| c.name.clone()).collect();
        assert!(
            names.iter().any(|n| n == "codex@beta"),
            "expected codex@beta in {names:?}",
        );
        // Filter should suppress the other runner entirely. With one
        // runner remaining, we still tag it (`codex@beta`) because the
        // filter is the user's explicit selection — disambiguation
        // matters more than terseness here.
        assert!(
            !names.iter().any(|n| n == "codex@alpha"),
            "did not expect codex@alpha in {names:?}",
        );
    }

    #[tokio::test]
    async fn execute_with_unknown_runner_filter_errors_with_known_names() {
        let (_dir, paths) = temp_paths();
        let cfg = cfg_with(vec![
            runner("alpha", "codex-not-installed-alpha"),
            runner("beta", "codex-not-installed-beta"),
        ]);
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let err = execute(&paths, Some("ghost"))
            .await
            .expect_err("expected error for unknown runner");
        let msg = format!("{err}");
        assert!(msg.contains("ghost"), "missing requested name: {msg}");
        assert!(msg.contains("alpha"), "missing known runner alpha: {msg}");
        assert!(msg.contains("beta"), "missing known runner beta: {msg}");
    }

    #[tokio::test]
    async fn agent_kind_drives_check_set() {
        // Sanity: switching an agent flips which check tags appear.
        // We assert via the helper function's tag construction so we
        // don't have to spawn binaries to see the difference.
        let mut codex_checks: Vec<Check> = Vec::new();
        let mut claude_checks: Vec<Check> = Vec::new();
        let mut cursor_checks: Vec<Check> = Vec::new();
        let mut openclaw_checks: Vec<Check> = Vec::new();
        let mut grok_checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut codex_checks,
            None,
            AgentKind::Codex,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        run_agent_checks(
            &mut claude_checks,
            None,
            AgentKind::ClaudeCode,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        run_agent_checks(
            &mut cursor_checks,
            None,
            AgentKind::CursorAgent,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        run_agent_checks(
            &mut openclaw_checks,
            None,
            AgentKind::OpenClaw,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        run_agent_checks(
            &mut grok_checks,
            None,
            AgentKind::Grok,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        let mut muse_checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut muse_checks,
            None,
            AgentKind::MuseCode,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "muse-missing",
        )
        .await;
        assert!(codex_checks.iter().any(|c| c.name == "codex"));
        assert!(codex_checks.iter().any(|c| c.name == "codex-auth"));
        assert!(claude_checks.iter().any(|c| c.name == "claude"));
        assert!(claude_checks.iter().any(|c| c.name == "claude-auth"));
        assert!(cursor_checks.iter().any(|c| c.name == "cursor-agent"));
        assert!(cursor_checks.iter().any(|c| c.name == "cursor-auth"));
        assert!(openclaw_checks.iter().any(|c| c.name == "acpx"));
        assert!(openclaw_checks.iter().any(|c| c.name == "openclaw"));
        assert!(grok_checks.iter().any(|c| c.name == "grok"));
        assert!(grok_checks.iter().any(|c| c.name == "grok-auth"));
        assert!(muse_checks.iter().any(|c| c.name == "muse"));
        assert!(muse_checks.iter().any(|c| c.name == "muse-auth"));
    }

    // ---- Claude Code auth classification (PIDASH-7) ----

    fn probe() -> ClaudeAuthProbe {
        ClaudeAuthProbe {
            status: None,
            env_var: None,
            third_party_provider: None,
            credentials: CredentialsFile::Missing,
            is_macos: false,
        }
    }

    #[test]
    fn claude_auth_env_key_set_is_logged_in() {
        let p = ClaudeAuthProbe {
            env_var: Some("ANTHROPIC_API_KEY"),
            ..probe()
        };
        match classify_claude_auth(&p) {
            AuthStatus::LoggedIn(d) => assert!(d.contains("ANTHROPIC_API_KEY"), "{d}"),
            other => panic!("expected LoggedIn, got {other:?}"),
        }
    }

    #[test]
    fn claude_auth_credentials_file_found_is_logged_in() {
        let p = ClaudeAuthProbe {
            credentials: CredentialsFile::Found(PathBuf::from("/h/.claude/.credentials.json")),
            ..probe()
        };
        match classify_claude_auth(&p) {
            AuthStatus::LoggedIn(d) => assert!(d.contains(".credentials.json"), "{d}"),
            other => panic!("expected LoggedIn, got {other:?}"),
        }
    }

    #[test]
    fn claude_auth_nothing_found_on_linux_is_not_logged_in() {
        assert!(matches!(
            classify_claude_auth(&probe()),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    #[test]
    fn claude_auth_macos_without_file_could_not_verify() {
        let p = ClaudeAuthProbe {
            is_macos: true,
            ..probe()
        };
        match classify_claude_auth(&p) {
            AuthStatus::Unknown(d) => assert!(d.contains("Keychain"), "{d}"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn claude_auth_unrecognized_credentials_file_could_not_verify() {
        let p = ClaudeAuthProbe {
            credentials: CredentialsFile::Unrecognized(PathBuf::from("/h/.credentials.json")),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
    }

    #[test]
    fn claude_auth_status_command_is_authoritative() {
        let yes = ClaudeAuthProbe {
            status: Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("claude.ai".into()),
            }),
            ..probe()
        };
        match classify_claude_auth(&yes) {
            AuthStatus::LoggedIn(d) => assert!(d.contains("claude.ai"), "{d}"),
            other => panic!("expected LoggedIn, got {other:?}"),
        }
        // Even on macOS, Claude's own "not logged in" answer wins — it reads
        // the Keychain itself.
        let no = ClaudeAuthProbe {
            status: Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            }),
            credentials: CredentialsFile::Found(PathBuf::from("/stale")),
            is_macos: true,
            ..probe()
        };
        assert!(matches!(
            classify_claude_auth(&no),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    #[test]
    fn claude_auth_third_party_provider_could_not_verify() {
        let p = ClaudeAuthProbe {
            third_party_provider: Some("CLAUDE_CODE_USE_BEDROCK"),
            status: Some(ClaudeStatus {
                logged_in: false,
                auth_method: None,
            }),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
    }

    #[test]
    fn claude_auth_status_to_check_mapping() {
        let ok = AuthStatus::LoggedIn("x".into()).into_check("claude-auth".into());
        assert!(ok.ok && !ok.blocker);
        let warn =
            AuthStatus::NotLoggedIn("run `claude /login`".into()).into_check("claude-auth".into());
        assert!(
            !warn.ok && !warn.blocker,
            "not logged in must warn, not block"
        );
        let unknown = AuthStatus::Unknown("could not verify".into()).into_check("a".into());
        assert!(unknown.ok && !unknown.blocker, "unknown must stay neutral");
    }

    #[test]
    fn parse_claude_status_reads_logged_in_json() {
        let out = "bash: no job control in this shell\n{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\"\n}\n";
        assert_eq!(
            parse_claude_status(out),
            Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("claude.ai".into()),
            })
        );
        let out = r#"{"loggedIn": false, "authMethod": "none"}"#;
        assert_eq!(parse_claude_status(out).map(|s| s.logged_in), Some(false));
        assert_eq!(parse_claude_status("Usage: claude [options]"), None);
        assert_eq!(parse_claude_status(r#"{"foo": 1}"#), None);
    }

    #[test]
    fn auth_status_subcommand_detection() {
        assert!(supports_auth_status(
            "Usage: claude auth status [options]\n\nShow authentication status"
        ));
        // An old CLI without the subcommand prints its top-level help for
        // `--help`; we must not then run `claude auth status`, which it would
        // treat as a prompt.
        assert!(!supports_auth_status(
            "Usage: claude [options] [command] [prompt]\n\nClaude Code"
        ));
    }

    #[test]
    fn read_credentials_file_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        assert_eq!(read_credentials_file(&path), CredentialsFile::Missing);
        std::fs::write(&path, r#"{"claudeAiOauth":{"accessToken":"t"}}"#).unwrap();
        assert_eq!(
            read_credentials_file(&path),
            CredentialsFile::Found(path.clone())
        );
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(
            read_credentials_file(&path),
            CredentialsFile::Unrecognized(path.clone())
        );
    }

    #[test]
    fn claude_credentials_path_honours_config_dir() {
        assert_eq!(
            claude_credentials_path(Some("/cfg".into()), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/cfg/.credentials.json"))
        );
        assert_eq!(
            claude_credentials_path(None, Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.claude/.credentials.json"))
        );
        assert_eq!(
            claude_credentials_path(Some("".into()), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.claude/.credentials.json"))
        );
    }

    #[test]
    fn env_key_auth_reports_set_key_or_keeps_hint() {
        let set = env_key_auth("XAI_API_KEY", Some("k".into()), "hint".into());
        assert!(matches!(set, AuthStatus::LoggedIn(ref d) if d.contains("XAI_API_KEY")));
        let unset = env_key_auth("XAI_API_KEY", None, "hint".into());
        assert!(matches!(unset, AuthStatus::Unknown(ref d) if d == "hint"));
        let empty = env_key_auth("XAI_API_KEY", Some("  ".into()), "hint".into());
        assert!(matches!(empty, AuthStatus::Unknown(_)));
    }

    #[test]
    fn parse_agent_env_reads_markers_and_ignores_rc_noise() {
        let out = "welcome from .bashrc\n__pidash_doctor_set ANTHROPIC_API_KEY\n\
                   __pidash_doctor_cfg /cfg\n__pidash_doctor_home /home/u\n";
        assert_eq!(
            parse_agent_env(out),
            Some(AgentEnvSnapshot {
                set_vars: vec!["ANTHROPIC_API_KEY".into()],
                config_dir: Some("/cfg".into()),
                home: Some("/home/u".into()),
            })
        );
        let empty = "__pidash_doctor_cfg \n__pidash_doctor_home /home/u\n";
        assert_eq!(parse_agent_env(empty).unwrap().config_dir, None);
        // The script did not run (no HOME marker): fall back, don't read as unset.
        assert_eq!(parse_agent_env("bash: sh: not found"), None);
    }

    #[cfg(unix)]
    #[test]
    fn agent_env_script_reports_set_names_only() {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(agent_env_script())
            .env_clear()
            .env("HOME", "/home/u")
            .env("ANTHROPIC_AUTH_TOKEN", "secret-value")
            .env("ANTHROPIC_API_KEY", "   ")
            .env("CLAUDE_CONFIG_DIR", "/cfg dir")
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains("secret-value"), "{text}");
        assert_eq!(
            parse_agent_env(&text),
            Some(AgentEnvSnapshot {
                set_vars: vec!["ANTHROPIC_AUTH_TOKEN".into()],
                config_dir: Some("/cfg dir".into()),
                home: Some("/home/u".into()),
            })
        );
    }

    #[test]
    fn build_claude_probe_uses_agent_env_for_fallback() {
        // Old CLI (no status): a key exported only by the login shell counts.
        let env = AgentEnvSnapshot {
            set_vars: vec!["CLAUDE_CODE_USE_VERTEX".into(), "ANTHROPIC_API_KEY".into()],
            config_dir: None,
            home: None,
        };
        let p = build_claude_probe(None, &env, false);
        assert_eq!(p.env_var, Some("ANTHROPIC_API_KEY"));
        assert_eq!(p.third_party_provider, Some("CLAUDE_CODE_USE_VERTEX"));
        assert!(matches!(classify_claude_auth(&p), AuthStatus::LoggedIn(_)));

        // CLAUDE_CONFIG_DIR from the agent env locates the credentials file.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"t"}}"#,
        )
        .unwrap();
        let env = AgentEnvSnapshot {
            config_dir: Some(dir.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let p = build_claude_probe(None, &env, false);
        assert!(matches!(p.credentials, CredentialsFile::Found(_)));
    }
}
