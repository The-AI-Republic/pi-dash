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
    // Agent auth depends on the environment the agent is spawned from, which
    // is the daemon's, not this terminal's. Ask the running daemon to run
    // the suite; only check in-process when no daemon answers.
    let daemon = report_from_daemon(
        &paths.ipc_socket_path(),
        args.runner.as_deref(),
        &paths.config_dir,
        DAEMON_DOCTOR_TIMEOUT,
    )
    .await;
    let report = resolve_report(paths, args.runner.as_deref(), daemon).await?;
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

/// Turn the daemon's answer into the report to print, checking in this
/// process when the daemon cannot speak for this request.
async fn resolve_report(
    paths: &Paths,
    runner: Option<&str>,
    daemon: DaemonReport,
) -> Result<Report> {
    match daemon {
        DaemonReport::Report(report) => Ok(report),
        DaemonReport::Error(message) => anyhow::bail!("{message}"),
        // An older daemon has no Claude probe and ignores the config dir.
        // Only Claude runners need what it lacks; the other checks are
        // the same here as before doctor asked the daemon.
        DaemonReport::Outdated(message) => {
            if request_has_claude_runner(paths, runner) {
                anyhow::bail!("{message}");
            }
            execute(paths, runner).await
        }
        DaemonReport::OtherConfig(message) => {
            tracing::debug!("not using the running daemon for doctor: {message}");
            let mut report = execute(paths, runner).await?;
            mark_local_checks(&mut report, OTHER_CONFIG_NOTE);
            Ok(report)
        }
        DaemonReport::Unavailable => {
            let mut report = execute(paths, runner).await?;
            mark_local_checks(&mut report, LOCAL_CHECK_NOTE);
            Ok(report)
        }
    }
}

/// Whether the runners this doctor request covers include a Claude Code
/// runner. An unreadable config or unknown `--runner` counts as no, so the
/// local check reports that error itself.
fn request_has_claude_runner(paths: &Paths, runner: Option<&str>) -> bool {
    let Ok(Some(cfg)) = file::load_config_opt(paths) else {
        return false;
    };
    cfg.runners
        .iter()
        .filter(|r| runner.is_none_or(|name| r.name == name))
        .any(|r| r.agent.kind == crate::config::schema::AgentKind::ClaudeCode)
}

pub async fn execute(paths: &Paths, runner_filter: Option<&str>) -> Result<Report> {
    execute_until(paths, runner_filter, None).await
}

/// [`execute`], with the per-runner checks cut off at `deadline`. A runner
/// still running then keeps the checks it finished and gets a
/// `runner-checks@<name>` entry, as does each runner after it. The
/// daemon-wide checks run either way.
pub async fn execute_until(
    paths: &Paths,
    runner_filter: Option<&str>,
    deadline: Option<tokio::time::Instant>,
) -> Result<Report> {
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
            "agy",
            "muse",
            false,
        )
        .await;
    } else {
        for r in &runners {
            let prefix = if multi { Some(r.name.as_str()) } else { None };
            let mut runner_checks = Vec::new();
            let run = run_agent_checks(
                &mut runner_checks,
                prefix,
                r.agent.kind,
                &r.codex.binary,
                &r.claude_code.binary,
                &r.cursor_agent.binary,
                &r.openclaw.binary,
                &r.grok.binary,
                &r.antigravity.binary,
                &r.muse_code.binary,
                crate::agent::auth::is_managed_codex(&r.codex),
            );
            let finished = match deadline {
                Some(d) => tokio::time::timeout_at(d, run).await.is_ok(),
                None => {
                    run.await;
                    true
                }
            };
            checks.append(&mut runner_checks);
            if !finished {
                checks.push(runner_timeout_check(&r.name, multi));
            }
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

    if let Some(cfg) = cfg.as_ref() {
        // Which credential store holds the daemon and engine credentials
        // (design §4.3: `pidash doctor` reports it).
        let store = crate::config::secrets::describe(paths, cfg);
        checks.push(Check {
            name: "credentials-store".to_string(),
            ok: store.ok,
            detail: store.detail,
            blocker: false,
        });
        if cfg
            .runners
            .iter()
            .any(|r| r.agent.kind == crate::config::schema::AgentKind::PidashAgent)
        {
            match crate::pidash_agent::locate::resolve(&cfg.engine) {
                Ok(engine) => checks.push(Check {
                    name: "engine".to_string(),
                    ok: true,
                    detail: engine.binary.display().to_string(),
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: "engine".to_string(),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            let signed_in = cfg
                .cli
                .as_ref()
                .and_then(|c| c.engine_credential.as_ref())
                .is_some_and(|c| !c.token.is_empty());
            checks.push(Check {
                name: "engine-sign-in".to_string(),
                ok: signed_in,
                detail: if signed_in {
                    "engine credential present".to_string()
                } else {
                    "the engine is not signed in — run `pidash login --engine`".to_string()
                },
                blocker: false,
            });
        }
    }

    Ok(Report {
        checks,
        daemon_probe_version: None,
    })
}

/// A runner whose checks were cut off by the daemon's doctor deadline.
fn runner_timeout_check(name: &str, tagged: bool) -> Check {
    Check {
        name: if tagged {
            format!("runner-checks@{name}")
        } else {
            "runner-checks".to_string()
        },
        ok: false,
        detail: format!(
            "this runner's checks did not finish within the daemon's doctor budget; \
             run `pidash doctor --runner {name}` to check it on its own"
        ),
        blocker: false,
    }
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
    antigravity_binary: &str,
    muse_binary: &str,
    codex_managed: bool,
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
            // Codex presence check: OPENAI_API_KEY or ~/.codex/auth.json
            // under the daemon's home. Shared with the pre-pickup gate.
            // Managed runners carry Pi Dash-managed auth and skip the
            // presence check, like Engine.
            if codex_managed {
                checks.push(Check {
                    name: tag("codex-auth"),
                    ok: true,
                    detail: "Pi Dash-managed Codex credentials (via desktop host)".to_string(),
                    blocker: false,
                });
            } else {
                let has_creds = crate::agent::auth::has_codex_credentials();
                checks.push(Check {
                    name: tag("codex-auth"),
                    ok: has_creds,
                    detail: if has_creds {
                        "Codex credentials present (OPENAI_API_KEY or ~/.codex/auth.json)"
                            .to_string()
                    } else {
                        format!(
                            "missing Codex credentials: {}",
                            crate::agent::auth::CODEX_FIX_HINT
                        )
                    },
                    blocker: false,
                });
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
            let binary_runs = checks.last().is_some_and(|c| c.ok);
            let auth = if binary_runs {
                match gather_claude_auth_probe(claude_binary).await {
                    Some(probe) => classify_claude_auth(&probe),
                    None => AuthStatus::Unknown(format!(
                        "reading Claude's config files took over {}s",
                        CLAUDE_FILE_SCAN_TIMEOUT.as_secs()
                    )),
                }
                .with_fix(claude_binary)
            } else {
                AuthStatus::Unknown("skipped: the claude CLI is not runnable".to_string())
            };
            checks.push(auth.into_check(tag("claude-auth")));
            checks.push(claude_pickup_check(
                tag("claude-pickup"),
                crate::agent::auth::has_claude_credentials(),
            ));
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
            let has_creds = crate::agent::auth::has_cursor_credentials();
            checks.push(Check {
                name: tag("cursor-auth"),
                ok: has_creds,
                detail: if has_creds {
                    "Cursor credentials present (CURSOR_API_KEY or saved login)".to_string()
                } else {
                    format!(
                        "missing Cursor credentials: {}",
                        crate::agent::auth::CURSOR_FIX_HINT
                    )
                },
                blocker: false,
            });
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
            let has_creds = crate::agent::auth::has_grok_credentials();
            checks.push(Check {
                name: tag("grok-auth"),
                ok: has_creds,
                detail: if has_creds {
                    "Grok credentials present (XAI_API_KEY or saved login)".to_string()
                } else {
                    format!(
                        "missing Grok credentials: {}",
                        crate::agent::auth::GROK_FIX_HINT
                    )
                },
                blocker: false,
            });
        }
        crate::config::schema::AgentKind::Antigravity => {
            match check_version(antigravity_binary).await {
                Ok(detail) => checks.push(Check {
                    name: tag("antigravity"),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: tag("antigravity"),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // NEWPIDASH-397 presence check: API key, Google Cloud ADC, or an
            // OS-keychain sign-in item (existence only, never the secret). The
            // run-time safety net (ACP error -32000 -> `agent_auth_missing`)
            // still catches expired or revoked sign-ins this cannot see.
            let detail = crate::agent::auth::antigravity_live_detail();
            checks.push(Check {
                name: tag("antigravity-auth"),
                ok: detail != crate::agent::auth::AntigravityAuthDetail::Missing,
                detail: detail.to_string(),
                blocker: false,
            });
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
            // Muse Code auth is a presence check only: `META_API_KEY` in the
            // environment the daemon spawns `muse` with, or a non-empty
            // `~/.config/muse/auth.json` under the same home. See
            // `agent::auth` (delegated to `muse_code::auth`). No API call is
            // made; presence is enough to accept a run, absence means
            // `muse exec` would fail at `run.lifecycle.started` and burn a
            // tick.
            let has_creds = crate::agent::auth::has_muse_credentials();
            checks.push(Check {
                name: tag("muse-auth"),
                ok: has_creds,
                detail: if has_creds {
                    "Muse credentials present (META_API_KEY or ~/.config/muse/auth.json)"
                        .to_string()
                } else {
                    format!(
                        "missing Muse credentials: {}",
                        crate::agent::auth::MUSE_FIX_HINT
                    )
                },
                blocker: false,
            });
        }
        // Checked once per daemon in `execute` (the binary and sign-in are
        // machine-level, not per runner).
        crate::config::schema::AgentKind::PidashAgent => {}
    }
}

/// How long a `<binary> --version` probe may run before doctor gives up on
/// it (same bound as the other short `claude` probes).
const VERSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Shared `<binary> --version` check used for every supported agent binary
/// (`codex`, `claude`, `cursor-agent`, and `acpx`), each of which prints a
/// short version line on stdout and exits 0 on success.
///
/// Runs through the same platform spawn helper the daemon uses so this check
/// reflects what the daemon will actually see at spawn time.
async fn check_version(binary: &str) -> Result<String> {
    check_version_within(binary, VERSION_PROBE_TIMEOUT).await
}

/// [`check_version`] with an explicit bound. The child is killed when it
/// overruns `limit` or when the caller drops this future (the daemon's
/// doctor budget does), so a hung binary never outlives the probe.
async fn check_version_within(binary: &str, limit: std::time::Duration) -> Result<String> {
    let mut cmd = login_shell_command(binary, &["--version"], None);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(limit, cmd.output())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "{binary} --version did not finish within {}s",
                limit.as_secs()
            )
        })??;
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
    /// Holds a `claudeAiOauth` entry with a token.
    Found(PathBuf),
    /// Exists but is not in a format we recognise.
    Unrecognized(PathBuf),
    Missing,
}

/// Whether a Claude config/settings file sets a given credential key.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfigKey {
    /// The key is set (non-empty string) in this file.
    Found(PathBuf),
    /// The file exists but could not be read or parsed, so we cannot rule
    /// the key out.
    Unreadable(PathBuf),
    /// No file, or the file parsed and does not set the key.
    Absent,
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
    /// First third-party-provider switch that is enabled (truthy).
    third_party_provider: Option<&'static str>,
    credentials: CredentialsFile,
    /// `apiKeyHelper` in user or managed Claude settings.
    api_key_helper: ConfigKey,
    /// An auth variable in the `env` block of user or managed Claude
    /// settings, which Claude Code applies to every session.
    settings_env_var: ConfigKey,
    /// An enabled cloud-provider switch in the `env` block of user or
    /// managed Claude settings.
    settings_provider: ConfigKey,
    /// `primaryApiKey` in Claude's global config (`~/.claude.json`), where
    /// a Console API-key login is stored.
    stored_api_key: ConfigKey,
    is_macos: bool,
}

fn classify_claude_auth(p: &ClaudeAuthProbe) -> AuthStatus {
    // Claude Code's own status command reads every credential source it
    // supports (env, credentials file, macOS Keychain, apiKeyHelper), so its
    // answer wins when we have one.
    //
    // A cloud-provider switch (Bedrock/Vertex/Foundry) routes runs to that
    // provider, whose credentials neither doctor nor `claude auth status`
    // checks (it answers `loggedIn: true, authMethod: "third_party"` with
    // none configured), so provider routing is always "could not verify".
    let provider = provider_routing(p);
    if let Some(status) = &p.status {
        if let Some(why) = provider {
            return AuthStatus::Unknown(why);
        }
        if status.logged_in {
            let method = status.auth_method.as_deref().unwrap_or("unknown method");
            return AuthStatus::LoggedIn(format!("logged in ({method})"));
        }
        // Claude counts an apiKeyHelper only when it runs it; doctor does
        // not, so a helper-only setup is not called logged out.
        if let ConfigKey::Found(path) = &p.api_key_helper {
            return AuthStatus::Unknown(api_key_helper_detail(path));
        }
        return AuthStatus::NotLoggedIn("`claude auth status` reports no login".to_string());
    }
    if let Some(why) = provider {
        return AuthStatus::Unknown(why);
    }
    if let Some(var) = p.env_var {
        return AuthStatus::LoggedIn(format!("{var} is set"));
    }
    if let CredentialsFile::Found(path) = &p.credentials {
        return AuthStatus::LoggedIn(format!("credentials found ({})", path.display()));
    }
    if let ConfigKey::Found(path) = &p.stored_api_key {
        return AuthStatus::LoggedIn(format!("API key stored in {}", path.display()));
    }
    if let ConfigKey::Found(path) = &p.settings_env_var {
        return AuthStatus::LoggedIn(format!("auth env var set in {}", path.display()));
    }
    if let ConfigKey::Found(path) = &p.api_key_helper {
        return AuthStatus::Unknown(api_key_helper_detail(path));
    }
    // From here on nothing positive was found. Only report "not logged in"
    // when every source we know of was actually ruled out.
    if let CredentialsFile::Unrecognized(path) = &p.credentials {
        return AuthStatus::Unknown(format!(
            "unrecognised credentials file ({})",
            path.display()
        ));
    }
    for key in [
        &p.api_key_helper,
        &p.settings_env_var,
        &p.settings_provider,
        &p.stored_api_key,
    ] {
        if let ConfigKey::Unreadable(path) = key {
            return AuthStatus::Unknown(format!("could not read {}", path.display()));
        }
    }
    // On macOS Claude Code keeps its login in the Keychain, so a missing
    // file says nothing.
    if p.is_macos {
        return AuthStatus::Unknown(
            "credentials are kept in the macOS Keychain, which doctor does not read".to_string(),
        );
    }
    AuthStatus::NotLoggedIn(
        "no auth env var, credentials file, apiKeyHelper or stored API key".to_string(),
    )
}

/// Why the login cannot be verified when an enabled provider switch routes
/// Claude to a cloud provider (agent env or settings `env`), else `None`.
fn provider_routing(p: &ClaudeAuthProbe) -> Option<String> {
    if let Some(var) = p.third_party_provider {
        return Some(format!(
            "{var} is enabled; cloud-provider credentials are not checked"
        ));
    }
    if let ConfigKey::Found(path) = &p.settings_provider {
        return Some(format!(
            "a cloud-provider switch is enabled in {}; its credentials are not checked",
            path.display()
        ));
    }
    if p.status
        .as_ref()
        .is_some_and(|s| s.logged_in && s.auth_method.as_deref() == Some("third_party"))
    {
        return Some(
            "`claude auth status` reports a third-party provider; its credentials are not checked"
                .to_string(),
        );
    }
    None
}

fn api_key_helper_detail(path: &Path) -> String {
    format!(
        "apiKeyHelper is set ({}) but doctor does not run it",
        path.display()
    )
}

/// Claude Code's boolean env semantics (`isEnvTruthy`): only these values,
/// case-insensitive and trimmed, switch a feature on. `0`/`false` are off.
fn claude_env_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Parse `claude auth status` JSON. Tolerates login-shell noise around the
/// object. Returns `None` for anything without a boolean `loggedIn`.
fn parse_claude_status(stdout: &str) -> Option<ClaudeStatus> {
    // Try each `{` as the start of a complete object, so shell banners with
    // braces before or after the response don't hide it.
    stdout.match_indices('{').find_map(|(start, _)| {
        let v = serde_json::Deserializer::from_str(&stdout[start..])
            .into_iter::<serde_json::Value>()
            .next()?
            .ok()?;
        Some(ClaudeStatus {
            logged_in: v.get("loggedIn")?.as_bool()?,
            auth_method: v
                .get("authMethod")
                .and_then(|m| m.as_str())
                .map(str::to_string),
        })
    })
}

/// True when `claude auth status --help` printed that subcommand's own usage.
/// An older CLI without it prints the top-level help instead; running
/// `claude auth status` there would treat the words as a prompt.
fn supports_auth_status(help_output: &str) -> bool {
    // Match the usage line itself, so a shell banner that merely mentions
    // the subcommand does not count.
    help_output.lines().any(|line| {
        line.trim_start()
            .strip_prefix("Usage:")
            .is_some_and(|rest| rest.trim_start().starts_with("claude auth status"))
    })
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

/// Claude's settings and global-config locations for a config root
/// (`CLAUDE_CONFIG_DIR`, else `~/.claude`). Returns
/// `(settings files that may set apiKeyHelper, global config file)`.
///
/// Project-level `.claude/settings*.json` are not scanned: they live in the
/// per-run checkout, which doctor does not know.
fn claude_config_files(
    config_dir: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
    managed: &[PathBuf],
) -> (Vec<PathBuf>, Option<PathBuf>) {
    let custom = config_dir.filter(|d| !d.is_empty()).map(PathBuf::from);
    let root = custom
        .clone()
        .or_else(|| home.as_ref().map(|h| h.join(".claude")));
    let mut settings: Vec<PathBuf> = root.into_iter().map(|r| r.join("settings.json")).collect();
    settings.extend(managed.iter().cloned());
    let global = match custom {
        Some(dir) => Some(dir.join(".claude.json")),
        None => home.map(|h| h.join(".claude.json")),
    };
    (settings, global)
}

/// System-wide managed settings, which can also set `apiKeyHelper`.
fn claude_managed_settings_paths() -> Vec<PathBuf> {
    let p = if cfg!(target_os = "macos") {
        "/Library/Application Support/ClaudeCode/managed-settings.json"
    } else if cfg!(target_os = "windows") {
        r"C:\ProgramData\ClaudeCode\managed-settings.json"
    } else {
        "/etc/claude-code/managed-settings.json"
    };
    vec![PathBuf::from(p)]
}

/// Whether the JSON file at `path` sets `key` to a non-empty string. The
/// value itself is never returned.
fn read_config_key(path: &Path, key: &str) -> ConfigKey {
    read_config_with(path, |v| non_empty_str(v.get(key)))
}

/// Whether the settings file at `path` sets a Claude auth variable in its
/// `env` block. Values are never returned.
fn read_settings_env_auth(path: &Path) -> ConfigKey {
    read_config_with(path, |v| {
        let Some(env) = v.get("env").and_then(|e| e.as_object()) else {
            return false;
        };
        CLAUDE_AUTH_ENV_VARS
            .iter()
            .any(|var| non_empty_str(env.get(*var)))
    })
}

/// Whether the effective Claude settings enable a cloud-provider switch.
///
/// `paths` are ordered lowest to highest precedence (user settings, then
/// managed settings). Claude Code merges the settings `env` blocks key by
/// key with the higher file winning, so each switch takes its value from
/// the highest-precedence file that sets it, explicit `"0"`/`false`
/// included. Returns the file whose enabled value is in force; when an
/// unreadable file outranks the deciding one, the switch cannot be ruled
/// out (an enabled lower value stays `Found`, otherwise `Unreadable`).
fn resolve_settings_provider(paths: &[PathBuf]) -> ConfigKey {
    // Highest precedence first.
    let envs: Vec<(&PathBuf, SettingsEnv)> = paths
        .iter()
        .rev()
        .map(|p| (p, read_settings_env(p)))
        .collect();
    let mut unreadable: Option<&PathBuf> = None;
    for var in CLAUDE_THIRD_PARTY_ENV_VARS {
        for (path, env) in &envs {
            match env {
                SettingsEnv::Unreadable => {
                    unreadable.get_or_insert(path);
                }
                SettingsEnv::Env(map) if map.contains_key(*var) => {
                    if setting_truthy(&map[*var]) {
                        return ConfigKey::Found((*path).clone());
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    unreadable.map_or(ConfigKey::Absent, |p| ConfigKey::Unreadable(p.clone()))
}

/// The `env` block of one Claude settings file.
enum SettingsEnv {
    Env(serde_json::Map<String, serde_json::Value>),
    /// Missing file, or no `env` block.
    None,
    /// The file exists but could not be read or parsed.
    Unreadable,
}

fn read_settings_env(path: &Path) -> SettingsEnv {
    let text = match read_regular_file(path) {
        FileText::Text(text) => text,
        FileText::Missing => return SettingsEnv::None,
        FileText::Unreadable => return SettingsEnv::Unreadable,
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut v)) => match v.remove("env") {
            Some(serde_json::Value::Object(env)) => SettingsEnv::Env(env),
            _ => SettingsEnv::None,
        },
        _ => SettingsEnv::Unreadable,
    }
}

/// Claude's boolean semantics for a settings `env` value.
fn setting_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => claude_env_truthy(s),
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => claude_env_truthy(&n.to_string()),
        _ => false,
    }
}

fn non_empty_str(v: Option<&serde_json::Value>) -> bool {
    v.and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty())
}

/// Read the JSON object at `path` and test it with `has`.
fn read_config_with(path: &Path, has: impl Fn(&serde_json::Value) -> bool) -> ConfigKey {
    let text = match read_regular_file(path) {
        FileText::Text(text) => text,
        FileText::Missing => return ConfigKey::Absent,
        FileText::Unreadable => return ConfigKey::Unreadable(path.to_path_buf()),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if v.is_object() => {
            if has(&v) {
                ConfigKey::Found(path.to_path_buf())
            } else {
                ConfigKey::Absent
            }
        }
        _ => ConfigKey::Unreadable(path.to_path_buf()),
    }
}

/// First file that sets `key`; otherwise the first unreadable one; else
/// [`ConfigKey::Absent`].
fn scan_config_key(paths: &[PathBuf], key: &str) -> ConfigKey {
    scan_configs(paths, |p| read_config_key(p, key))
}

/// First `Found` from `read` over `paths`; otherwise the first unreadable
/// one; else [`ConfigKey::Absent`].
fn scan_configs(paths: &[PathBuf], read: impl Fn(&Path) -> ConfigKey) -> ConfigKey {
    let results: Vec<ConfigKey> = paths.iter().map(|p| read(p)).collect();
    results
        .iter()
        .find(|r| matches!(r, ConfigKey::Found(_)))
        .or_else(|| {
            results
                .iter()
                .find(|r| matches!(r, ConfigKey::Unreadable(_)))
        })
        .cloned()
        .unwrap_or(ConfigKey::Absent)
}

/// One of Claude's config files, read only if it is a regular file: a FIFO
/// or device would block the read.
enum FileText {
    Text(String),
    Missing,
    Unreadable,
}

fn read_regular_file(path: &Path) -> FileText {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileText::Missing,
        Ok(m) if m.is_file() => match std::fs::read_to_string(path) {
            Ok(text) => FileText::Text(text),
            Err(_) => FileText::Unreadable,
        },
        _ => FileText::Unreadable,
    }
}

fn read_credentials_file(path: &Path) -> CredentialsFile {
    let text = match read_regular_file(path) {
        FileText::Text(text) => text,
        FileText::Missing => return CredentialsFile::Missing,
        FileText::Unreadable => return CredentialsFile::Unrecognized(path.to_path_buf()),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        // An OAuth record counts only when it holds a token.
        Ok(v)
            if v.get("claudeAiOauth").is_some_and(|o| {
                non_empty_str(o.get("accessToken")) || non_empty_str(o.get("refreshToken"))
            }) =>
        {
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
    /// Names from [`CLAUDE_AUTH_ENV_VARS`] that are set and non-blank.
    set_vars: Vec<String>,
    /// Names from [`CLAUDE_THIRD_PARTY_ENV_VARS`] that are enabled under
    /// Claude's boolean semantics ([`claude_env_truthy`]).
    providers: Vec<String>,
    config_dir: Option<String>,
    home: Option<String>,
}

const ENV_SET_MARK: &str = "__pidash_doctor_set ";
const ENV_PROV_MARK: &str = "__pidash_doctor_prov ";
const ENV_CFG_MARK: &str = "__pidash_doctor_cfg ";
const ENV_HOME_MARK: &str = "__pidash_doctor_home ";

/// POSIX sh script printing [`AgentEnvSnapshot`] as marked lines. Marker
/// prefixes let us ignore whatever the user's rc files print. Credential
/// variables report only their names; provider switches (not secrets)
/// report their value so truthiness is decided in Rust.
fn agent_env_script() -> String {
    format!(
        "for v in {creds}; do eval \"x=\\${{$v-}}\"; case \"$x\" in *[![:space:]]*) \
         printf '{ENV_SET_MARK}%s\\n' \"$v\";; esac; done; \
         for v in {provs}; do eval \"x=\\${{$v-}}\"; [ -n \"$x\" ] && \
         printf '{ENV_PROV_MARK}%s=%s\\n' \"$v\" \"$x\"; done; \
         printf '{ENV_CFG_MARK}%s\\n' \"${{CLAUDE_CONFIG_DIR-}}\"; \
         printf '{ENV_HOME_MARK}%s\\n' \"${{HOME-}}\"",
        creds = CLAUDE_AUTH_ENV_VARS.join(" "),
        provs = CLAUDE_THIRD_PARTY_ENV_VARS.join(" "),
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
        } else if let Some(v) = line.strip_prefix(ENV_PROV_MARK) {
            if let Some((name, value)) = v.split_once('=')
                && CLAUDE_THIRD_PARTY_ENV_VARS.contains(&name)
                && claude_env_truthy(value)
            {
                snap.providers.push(name.to_string());
            }
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
            .filter(|v| set(v))
            .map(|v| v.to_string())
            .collect(),
        providers: CLAUDE_THIRD_PARTY_ENV_VARS
            .iter()
            .filter(|v| std::env::var(v).is_ok_and(|x| claude_env_truthy(&x)))
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
    managed_settings: &[PathBuf],
    is_macos: bool,
) -> ClaudeAuthProbe {
    let find = |list: &[&'static str], set: &[String]| {
        list.iter().copied().find(|v| set.iter().any(|s| s == v))
    };
    let credentials = claude_credentials_path(
        env.config_dir.clone().map(Into::into),
        env.home.clone().map(PathBuf::from),
    )
    .map(|p| read_credentials_file(&p))
    .unwrap_or(CredentialsFile::Missing);
    let (settings, global) = claude_config_files(
        env.config_dir.clone().map(Into::into),
        env.home.clone().map(PathBuf::from),
        managed_settings,
    );
    ClaudeAuthProbe {
        status,
        env_var: find(CLAUDE_AUTH_ENV_VARS, &env.set_vars),
        third_party_provider: find(CLAUDE_THIRD_PARTY_ENV_VARS, &env.providers),
        credentials,
        api_key_helper: scan_config_key(&settings, "apiKeyHelper"),
        settings_env_var: scan_configs(&settings, read_settings_env_auth),
        settings_provider: resolve_settings_provider(&settings),
        stored_api_key: scan_config_key(global.as_slice(), "primaryApiKey"),
        is_macos,
    }
}

/// How long the scan of Claude's config files may block.
const CLAUDE_FILE_SCAN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Run blocking `f` off the async workers, giving up after `limit`. A read
/// stuck on a stalled mount then costs one blocking thread, not the
/// daemon's IPC task.
async fn blocking_within<T: Send + 'static>(
    limit: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    tokio::time::timeout(limit, tokio::task::spawn_blocking(f))
        .await
        .ok()?
        .ok()
}

/// `None` when Claude's config files could not be read in time.
async fn gather_claude_auth_probe(binary: &str) -> Option<ClaudeAuthProbe> {
    let status = claude_auth_status(binary).await;
    let env = agent_env_snapshot().await;
    blocking_within(CLAUDE_FILE_SCAN_TIMEOUT, move || {
        build_claude_probe(
            status,
            &env,
            &claude_managed_settings_paths(),
            cfg!(target_os = "macos"),
        )
    })
    .await
}

/// Appended to agent-auth checks run in this process because no daemon
/// answered: they reflect this terminal's environment, not the daemon's.
const LOCAL_CHECK_NOTE: &str =
    "[checked in this terminal; the daemon is not running, so its environment may differ]";

/// Like [`LOCAL_CHECK_NOTE`], when the running daemon uses another config.
const OTHER_CONFIG_NOTE: &str = "[checked in this terminal; the running daemon uses another \
     config dir, so this config's runs may see a different environment]";

/// Checks whose answer depends on the environment they ran in.
fn is_env_dependent(name: &str) -> bool {
    ["claude-auth", "claude-pickup"]
        .iter()
        .any(|base| name == *base || name.starts_with(&format!("{base}@")))
}

fn mark_local_checks(report: &mut Report, note: &str) {
    for c in &mut report.checks {
        if is_env_dependent(&c.name) {
            c.detail = format!("{} {note}", c.detail);
        }
    }
}

#[derive(Debug)]
enum DaemonReport {
    /// The running daemon ran the suite in its own environment.
    Report(Report),
    /// The daemon was reached but gave no usable report: an error (e.g. an
    /// unknown `--runner`), a timeout, or a report from an older daemon.
    Error(String),
    /// The running daemon uses another config dir than the one asked
    /// about; check in this process instead.
    OtherConfig(String),
    /// No daemon answered; check in this process instead.
    Unavailable,
    /// The daemon predates the Claude auth probe (and ignores the config
    /// dir it was asked about).
    Outdated(String),
}

/// Version of the Claude auth probe a daemon stamps on its `DoctorRun`
/// report. A daemon that predates the probe leaves it unset.
pub const CLAUDE_AUTH_PROBE_VERSION: u32 = 1;
/// `DoctorRun` error code: the daemon uses another config dir.
pub const CONFIG_MISMATCH_CODE: i64 = 409;
/// How long the daemon may spend on `DoctorRun` before it gives up.
pub const DAEMON_DOCTOR_BUDGET: std::time::Duration = std::time::Duration::from_secs(170);
/// How long the CLI waits for the daemon: the budget plus a margin, so the
/// daemon's own "did not finish" error normally arrives first.
const DAEMON_DOCTOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(185);
/// Part of [`DAEMON_DOCTOR_BUDGET`] kept for the daemon-wide checks (git,
/// the 10s cloud probe) after the per-runner checks are cut off.
const DAEMON_RUNNER_RESERVE: std::time::Duration = std::time::Duration::from_secs(20);
const _: () = assert!(
    DAEMON_RUNNER_RESERVE.as_secs() < DAEMON_DOCTOR_BUDGET.as_secs()
        && DAEMON_DOCTOR_BUDGET.as_secs() < DAEMON_DOCTOR_TIMEOUT.as_secs()
);

/// Whether the daemon's pre-pickup gate (`agent::auth::has_claude_credentials`:
/// the daemon process's env plus the legacy login files) lets Claude runs
/// through. Reported apart from `claude-auth`: a key exported only by the
/// login shell passes the auth probe but not this gate.
fn claude_pickup_check(name: String, present: bool) -> Check {
    Check {
        name,
        ok: present,
        detail: if present {
            "the daemon's pre-pickup credential check passes".to_string()
        } else {
            format!(
                "the daemon refuses Claude runs before starting them: its pre-pickup check finds \
                 no ANTHROPIC_API_KEY in the daemon process's own environment and none of \
                 ~/.claude.json, ~/.claude/credentials.json, ~/.config/claude/auth.json \
                 (a key exported only in shell rc files does not count); {}",
                crate::agent::auth::CLAUDE_FIX_HINT
            )
        },
        blocker: false,
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// The daemon's side of `DoctorRun`: refuse a request about another config
/// dir, bound the suite by `budget`, and stamp the probe version.
pub async fn daemon_doctor(
    paths: &Paths,
    runner: Option<&str>,
    config_dir: Option<PathBuf>,
    budget: std::time::Duration,
) -> crate::ipc::protocol::Response {
    use crate::ipc::protocol::{Response, RpcError};
    // A relative path would resolve against the daemon's working
    // directory, not the CLI's; the CLI always sends an absolute one.
    if let Some(wanted) = config_dir
        && (wanted.is_relative() || !same_dir(&wanted, &paths.config_dir))
    {
        return Response::Error(RpcError {
            code: CONFIG_MISMATCH_CODE,
            message: format!(
                "the running daemon uses config dir {}, not {}",
                paths.config_dir.display(),
                wanted.display()
            ),
        });
    }
    // Stop the per-runner checks early enough that the daemon-wide checks
    // still fit, so a slow runner costs only its own results.
    let runner_deadline =
        tokio::time::Instant::now() + budget.saturating_sub(DAEMON_RUNNER_RESERVE);
    match tokio::time::timeout(budget, execute_until(paths, runner, Some(runner_deadline))).await {
        Ok(Ok(mut report)) => {
            report.daemon_probe_version = Some(CLAUDE_AUTH_PROBE_VERSION);
            Response::Doctor(report)
        }
        Ok(Err(e)) => Response::Error(RpcError {
            code: 400,
            message: e.to_string(),
        }),
        Err(_) => Response::Error(RpcError {
            code: 504,
            message: format!(
                "the daemon's doctor checks did not finish within {}s; \
                 retry with `--runner <name>` to check one runner",
                budget.as_secs()
            ),
        }),
    }
}

/// Ask the running daemon to run doctor (`DoctorRun` over IPC) for
/// `config_dir`. Only a failed connect means "no daemon"; once connected,
/// every other outcome is reported rather than silently re-run here.
async fn report_from_daemon(
    socket: &Path,
    runner: Option<&str>,
    config_dir: &Path,
    timeout: std::time::Duration,
) -> DaemonReport {
    use crate::ipc::protocol::{Request, Response};
    let Ok(mut client) = crate::ipc::client::Client::connect(socket).await else {
        return DaemonReport::Unavailable;
    };
    let call = client.call(Request::DoctorRun {
        runner: runner.map(str::to_string),
        config_dir: Some(std::path::absolute(config_dir).unwrap_or_else(|_| config_dir.into())),
    });
    match tokio::time::timeout(timeout, call).await {
        Ok(Ok(Response::Doctor(report))) => match report.daemon_probe_version {
            Some(v) if v >= CLAUDE_AUTH_PROBE_VERSION => DaemonReport::Report(report),
            _ => DaemonReport::Outdated(
                "the running daemon is an older build whose doctor does not check the Claude \
                 login; restart it with `pidash restart` and run doctor again"
                    .to_string(),
            ),
        },
        Ok(Ok(Response::Error(e))) if e.code == CONFIG_MISMATCH_CODE => {
            DaemonReport::OtherConfig(e.message)
        }
        Ok(Ok(Response::Error(e))) => DaemonReport::Error(e.message),
        Ok(Ok(other)) => {
            DaemonReport::Error(format!("unexpected answer from the daemon: {other:?}"))
        }
        Ok(Err(e)) => DaemonReport::Error(format!("the daemon connection failed: {e}")),
        Err(_) => DaemonReport::Error(format!(
            "the daemon did not answer doctor within {}s; it is running, so its checks were \
             not repeated here; retry with `--runner <name>` to check one runner",
            timeout.as_secs()
        )),
    }
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
        AgentKind, AntigravitySection, ClaudeCodeSection, CodexSection, Config, CursorAgentSection,
        DaemonConfig, GrokSection, MuseCodeSection, OpenClawSection, RunnerConfig,
        WorkspaceSection,
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
            antigravity: AntigravitySection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: Default::default(),
        }
    }

    fn cfg_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "http://127.0.0.1:1".into(),
                dev_machine_id: None,
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
                agent_watchdog: Default::default(),
                auto_update: true,
                max_concurrent_agent_runs: 3,
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
            "agy-missing",
            "muse-missing",
            false,
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
            "agy-missing",
            "muse-missing",
            false,
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
            "agy-missing",
            "muse-missing",
            false,
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
            "agy-missing",
            "muse-missing",
            false,
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
            "agy-missing",
            "muse-missing",
            false,
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
            "agy-missing",
            "muse-missing",
            false,
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

        let mut antigravity_checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut antigravity_checks,
            None,
            AgentKind::Antigravity,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "agy-missing",
            "muse-missing",
            false,
        )
        .await;
        assert!(antigravity_checks.iter().any(|c| c.name == "antigravity"));
        // Until NEWPIDASH-397 lands the auth gate never blocks on a guess.
        let auth = antigravity_checks
            .iter()
            .find(|c| c.name == "antigravity-auth")
            .expect("antigravity-auth check present");
        assert!(
            auth.ok,
            "antigravity-auth must be ok (never block on a guess)"
        );
        assert!(!auth.blocker);
    }

    #[tokio::test]
    async fn managed_codex_reports_ok_without_user_credentials() {
        // Managed runners carry Pi Dash-managed auth; doctor must not
        // fail codex-auth when the user's own credentials are missing.
        let mut checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut checks,
            None,
            AgentKind::Codex,
            "codex-missing",
            "claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "agy-missing",
            "muse-missing",
            true,
        )
        .await;
        let codex_auth = checks
            .iter()
            .find(|c| c.name == "codex-auth")
            .expect("codex-auth check present");
        assert!(codex_auth.ok, "managed codex-auth must be ok");
        assert!(codex_auth.detail.contains("managed"));
    }

    // ---- Claude Code auth classification ----

    fn probe() -> ClaudeAuthProbe {
        ClaudeAuthProbe {
            status: None,
            env_var: None,
            third_party_provider: None,
            credentials: CredentialsFile::Missing,
            api_key_helper: ConfigKey::Absent,
            settings_env_var: ConfigKey::Absent,
            settings_provider: ConfigKey::Absent,
            stored_api_key: ConfigKey::Absent,
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
        // A login-shell banner that mentions the subcommand is not its usage.
        assert!(!supports_auth_status(
            "tip: run `claude auth status` to check your login\n\
             Usage: claude [options] [command] [prompt]"
        ));
        assert!(supports_auth_status(
            "welcome\n  Usage:   claude auth status [options]"
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
        // An OAuth record without a token is not a login.
        for body in [
            r#"{"claudeAiOauth":{}}"#,
            r#"{"claudeAiOauth":{"accessToken":"  "}}"#,
        ] {
            std::fs::write(&path, body).unwrap();
            assert_eq!(
                read_credentials_file(&path),
                CredentialsFile::Unrecognized(path.clone()),
                "{body}"
            );
        }
        std::fs::write(&path, r#"{"claudeAiOauth":{"refreshToken":"r"}}"#).unwrap();
        assert_eq!(
            read_credentials_file(&path),
            CredentialsFile::Found(path.clone())
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
    fn parse_agent_env_reads_markers_and_ignores_rc_noise() {
        let out = "welcome from .bashrc\n__pidash_doctor_set ANTHROPIC_API_KEY\n\
                   __pidash_doctor_cfg /cfg\n__pidash_doctor_home /home/u\n";
        assert_eq!(
            parse_agent_env(out),
            Some(AgentEnvSnapshot {
                set_vars: vec!["ANTHROPIC_API_KEY".into()],
                providers: vec![],
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
            .env("CLAUDE_CODE_USE_BEDROCK", " TRUE ")
            .env("CLAUDE_CODE_USE_VERTEX", "0")
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains("secret-value"), "{text}");
        assert_eq!(
            parse_agent_env(&text),
            Some(AgentEnvSnapshot {
                set_vars: vec!["ANTHROPIC_AUTH_TOKEN".into()],
                providers: vec!["CLAUDE_CODE_USE_BEDROCK".into()],
                config_dir: Some("/cfg dir".into()),
                home: Some("/home/u".into()),
            })
        );
    }

    #[test]
    fn build_claude_probe_uses_agent_env_for_fallback() {
        // Old CLI (no status): a key exported only by the login shell counts.
        let env = AgentEnvSnapshot {
            set_vars: vec!["ANTHROPIC_API_KEY".into()],
            providers: vec!["CLAUDE_CODE_USE_VERTEX".into()],
            config_dir: None,
            home: None,
        };
        let p = build_claude_probe(None, &env, &[], false);
        assert_eq!(p.env_var, Some("ANTHROPIC_API_KEY"));
        assert_eq!(p.third_party_provider, Some("CLAUDE_CODE_USE_VERTEX"));
        // Vertex routing means the Anthropic key is not what runs use.
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
        let env = AgentEnvSnapshot {
            set_vars: vec!["ANTHROPIC_API_KEY".into()],
            ..AgentEnvSnapshot::default()
        };
        let p = build_claude_probe(None, &env, &[], false);
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
        let p = build_claude_probe(None, &env, &[], false);
        assert!(matches!(p.credentials, CredentialsFile::Found(_)));
    }

    #[test]
    fn claude_auth_api_key_helper_is_unverified_and_stored_key_is_logged_in() {
        // Doctor never runs the helper, so its presence alone is not a
        // verified login: neutral, with the limitation in the detail.
        let p = ClaudeAuthProbe {
            api_key_helper: ConfigKey::Found(PathBuf::from("/h/.claude/settings.json")),
            ..probe()
        };
        match classify_claude_auth(&p) {
            AuthStatus::Unknown(d) => {
                assert!(d.contains("apiKeyHelper") && d.contains("not run"), "{d}")
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
        let p = ClaudeAuthProbe {
            stored_api_key: ConfigKey::Found(PathBuf::from("/h/.claude.json")),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::LoggedIn(_)));
    }

    #[test]
    fn claude_auth_unreadable_settings_could_not_verify() {
        // A settings file we cannot parse might hold an apiKeyHelper, so we
        // cannot claim "not logged in".
        for p in [
            ClaudeAuthProbe {
                api_key_helper: ConfigKey::Unreadable(PathBuf::from("/h/.claude/settings.json")),
                ..probe()
            },
            ClaudeAuthProbe {
                stored_api_key: ConfigKey::Unreadable(PathBuf::from("/h/.claude.json")),
                ..probe()
            },
        ] {
            assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
        }
    }

    #[test]
    fn read_config_key_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(read_config_key(&path, "apiKeyHelper"), ConfigKey::Absent);
        std::fs::write(&path, r#"{"apiKeyHelper":"/bin/get-key.sh"}"#).unwrap();
        assert_eq!(
            read_config_key(&path, "apiKeyHelper"),
            ConfigKey::Found(path.clone())
        );
        std::fs::write(&path, r#"{"model":"opus","apiKeyHelper":""}"#).unwrap();
        assert_eq!(read_config_key(&path, "apiKeyHelper"), ConfigKey::Absent);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(
            read_config_key(&path, "apiKeyHelper"),
            ConfigKey::Unreadable(path.clone())
        );
    }

    #[test]
    fn build_claude_probe_finds_api_key_helper_in_settings() {
        // Old CLI (status unavailable), no env var, no credentials file, but
        // `apiKeyHelper` in the user's settings: logged in, not a warning.
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".claude")).unwrap();
        std::fs::write(
            home.path().join(".claude").join("settings.json"),
            r#"{"apiKeyHelper":"~/bin/anthropic-key.sh"}"#,
        )
        .unwrap();
        let env = AgentEnvSnapshot {
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let p = build_claude_probe(None, &env, &[], false);
        assert!(matches!(p.api_key_helper, ConfigKey::Found(_)));
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));

        // Managed settings count too, and CLAUDE_CONFIG_DIR moves the user
        // settings and global config.
        let cfg = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let managed_file = managed.path().join("managed-settings.json");
        std::fs::write(&managed_file, r#"{"apiKeyHelper":"/opt/key.sh"}"#).unwrap();
        let env = AgentEnvSnapshot {
            config_dir: Some(cfg.path().display().to_string()),
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let p = build_claude_probe(None, &env, std::slice::from_ref(&managed_file), false);
        assert_eq!(p.api_key_helper, ConfigKey::Found(managed_file));

        // A Console API key stored in the global config counts.
        std::fs::write(cfg.path().join(".claude.json"), r#"{"primaryApiKey":"sk"}"#).unwrap();
        let p = build_claude_probe(None, &env, &[], false);
        assert!(matches!(p.stored_api_key, ConfigKey::Found(_)));

        // With all sources ruled out it is still "not logged in" on Linux.
        let empty = tempfile::tempdir().unwrap();
        let env = AgentEnvSnapshot {
            home: Some(empty.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let p = build_claude_probe(None, &env, &[], false);
        assert!(matches!(
            classify_claude_auth(&p),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    #[test]
    fn build_claude_probe_finds_auth_var_in_settings_env() {
        // Old CLI, nothing in the process env, but the key lives in the
        // settings `env` block that Claude Code applies to every session.
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".claude")).unwrap();
        let settings = home.path().join(".claude").join("settings.json");
        let env = AgentEnvSnapshot {
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        std::fs::write(&settings, r#"{"env":{"ANTHROPIC_API_KEY":"sk-x"}}"#).unwrap();
        let p = build_claude_probe(None, &env, &[], false);
        assert_eq!(p.settings_env_var, ConfigKey::Found(settings.clone()));
        match classify_claude_auth(&p) {
            AuthStatus::LoggedIn(d) => assert!(!d.contains("sk-x"), "{d}"),
            other => panic!("expected LoggedIn, got {other:?}"),
        }
        // An unrelated or empty env entry does not count.
        std::fs::write(&settings, r#"{"env":{"ANTHROPIC_API_KEY":"","FOO":"1"}}"#).unwrap();
        let p = build_claude_probe(None, &env, &[], false);
        assert_eq!(p.settings_env_var, ConfigKey::Absent);
        assert!(matches!(
            classify_claude_auth(&p),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    // ---- Review follow-ups: provider routing, daemon env, parsing ----

    #[test]
    fn claude_auth_status_third_party_login_is_unverified() {
        // Claude reports `loggedIn: true, authMethod: "third_party"` for
        // Bedrock/Vertex/Foundry without checking the cloud credentials.
        let p = ClaudeAuthProbe {
            status: Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("third_party".into()),
            }),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
        // Same when the switch is on in the agent env and status says yes.
        let p = ClaudeAuthProbe {
            status: Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("api_key".into()),
            }),
            third_party_provider: Some("CLAUDE_CODE_USE_BEDROCK"),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
    }

    #[test]
    fn claude_settings_provider_switch_is_unverified_not_logged_in() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".claude")).unwrap();
        let settings = home.path().join(".claude").join("settings.json");
        let env = AgentEnvSnapshot {
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        std::fs::write(&settings, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#).unwrap();
        let p = build_claude_probe(None, &env, &[], false);
        assert_eq!(p.settings_env_var, ConfigKey::Absent);
        assert_eq!(p.settings_provider, ConfigKey::Found(settings.clone()));
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
        // A disabled switch in settings is not provider routing.
        std::fs::write(&settings, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"false"}}"#).unwrap();
        let p = build_claude_probe(None, &env, &[], false);
        assert_eq!(p.settings_provider, ConfigKey::Absent);
        assert!(matches!(
            classify_claude_auth(&p),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    #[test]
    fn disabled_provider_switch_does_not_hide_logged_out() {
        assert!(claude_env_truthy("1"));
        assert!(claude_env_truthy(" Yes "));
        assert!(claude_env_truthy("ON"));
        assert!(!claude_env_truthy("0"));
        assert!(!claude_env_truthy("false"));
        assert!(!claude_env_truthy(""));
        let out = "__pidash_doctor_prov CLAUDE_CODE_USE_BEDROCK=0\n\
                   __pidash_doctor_prov CLAUDE_CODE_USE_VERTEX=false\n\
                   __pidash_doctor_home /home/u\n";
        let env = parse_agent_env(out).unwrap();
        assert!(env.providers.is_empty(), "{env:?}");
        let p = build_claude_probe(
            Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            }),
            &env,
            &[],
            false,
        );
        assert!(matches!(
            classify_claude_auth(&p),
            AuthStatus::NotLoggedIn(_)
        ));
    }

    #[test]
    fn status_logged_out_with_api_key_helper_is_unverified() {
        let p = ClaudeAuthProbe {
            status: Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            }),
            api_key_helper: ConfigKey::Found(PathBuf::from("/h/.claude/settings.json")),
            ..probe()
        };
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
    }

    #[test]
    fn parse_claude_status_skips_brace_banners() {
        let out = "{ welcome to box } motd {x}\n{\"loggedIn\": false, \"authMethod\": \"none\"}\nbye {}\n";
        assert_eq!(
            parse_claude_status(out),
            Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            })
        );
    }

    #[test]
    fn local_report_labels_claude_auth_as_local() {
        let mut report = Report {
            checks: vec![
                AuthStatus::LoggedIn("logged in (api_key)".into())
                    .into_check("claude-auth@a".into()),
                Check {
                    name: "git".into(),
                    ok: true,
                    detail: "git 2".into(),
                    blocker: true,
                },
            ],
            daemon_probe_version: None,
        };
        mark_local_checks(&mut report, LOCAL_CHECK_NOTE);
        assert!(report.checks[0].detail.contains(LOCAL_CHECK_NOTE));
        assert_eq!(report.checks[1].detail, "git 2");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_uses_running_daemon_report() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("pidash.sock");
        // No daemon: caller falls back to a local check.
        assert!(matches!(
            report_from_daemon(&sock, None, dir.path(), DAEMON_DOCTOR_TIMEOUT).await,
            DaemonReport::Unavailable
        ));
        let server = fake_daemon(
            &sock,
            Some(crate::ipc::protocol::Response::Doctor(Report {
                checks: vec![
                    AuthStatus::LoggedIn("daemon env".into()).into_check("claude-auth".into()),
                ],
                daemon_probe_version: Some(CLAUDE_AUTH_PROBE_VERSION),
            })),
        );
        match report_from_daemon(&sock, Some("alpha"), dir.path(), DAEMON_DOCTOR_TIMEOUT).await {
            DaemonReport::Report(r) => assert_eq!(r.checks[0].detail, "daemon env"),
            other => panic!("expected daemon report, got {other:?}"),
        }
        let req = server.await.unwrap();
        assert!(matches!(
            req,
            crate::ipc::protocol::Request::DoctorRun { runner: Some(ref r), .. } if r == "alpha"
        ));
    }

    #[tokio::test]
    async fn missing_claude_binary_skips_auth_probe() {
        let mut checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut checks,
            None,
            AgentKind::ClaudeCode,
            "codex-missing",
            "/nonexistent/claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "agy-missing",
            "muse-missing",
            false,
        )
        .await;
        let auth = checks.iter().find(|c| c.name == "claude-auth").unwrap();
        assert!(auth.ok && !auth.blocker, "{auth:?}");
        assert!(auth.detail.contains("skipped"), "{auth:?}");
    }

    /// A fake `claude` that records its argv, to prove an old CLI never gets
    /// `auth status` (which it would read as a prompt).
    #[cfg(unix)]
    fn fake_claude(dir: &Path, help: &str, status: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("argv.log");
        let bin = dir.join("claude");
        let script = format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\n\
             if [ \"$*\" = \"auth status --help\" ]; then printf '%s\\n' '{help}'; exit 0; fi\n\
             if [ \"$*\" = \"auth status\" ]; then printf '%s\\n' '{status}'; exit 1; fi\n\
             echo 'prompt mode'\n",
            log = log.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin, log)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_auth_status_calls_only_safe_subcommands() {
        let old = tempfile::tempdir().unwrap();
        let (bin, log) = fake_claude(old.path(), "Usage: claude [options] [prompt]", "");
        assert_eq!(claude_auth_status(&bin.display().to_string()).await, None);
        let argv = std::fs::read_to_string(&log).unwrap();
        assert_eq!(argv.trim(), "auth status --help", "{argv}");

        let new = tempfile::tempdir().unwrap();
        let (bin, log) = fake_claude(
            new.path(),
            "Usage: claude auth status [options]",
            r#"{"loggedIn": false, "authMethod": "none"}"#,
        );
        assert_eq!(
            claude_auth_status(&bin.display().to_string()).await,
            Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            })
        );
        let argv = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            argv.lines().collect::<Vec<_>>(),
            vec!["auth status --help", "auth status"]
        );
    }

    #[test]
    fn claude_pickup_check_reports_the_daemon_gate() {
        let ok = claude_pickup_check("claude-pickup".into(), true);
        assert!(ok.ok && !ok.blocker, "{ok:?}");
        let refused = claude_pickup_check("claude-pickup".into(), false);
        assert!(!refused.ok && !refused.blocker, "{refused:?}");
        assert!(
            refused.detail.contains("refuses Claude runs"),
            "{refused:?}"
        );
        assert!(refused.detail.contains("ANTHROPIC_API_KEY"), "{refused:?}");
    }

    #[tokio::test]
    async fn claude_runner_reports_pickup_gate_and_local_label() {
        let mut checks: Vec<Check> = Vec::new();
        run_agent_checks(
            &mut checks,
            Some("a"),
            AgentKind::ClaudeCode,
            "codex-missing",
            "/nonexistent/claude-missing",
            "cursor-missing",
            "acpx-missing",
            "grok-missing",
            "agy-missing",
            "muse-missing",
            false,
        )
        .await;
        assert!(
            checks.iter().any(|c| c.name == "claude-pickup@a"),
            "{checks:?}"
        );
        let mut report = Report {
            checks,
            daemon_probe_version: None,
        };
        mark_local_checks(&mut report, LOCAL_CHECK_NOTE);
        let pickup = report
            .checks
            .iter()
            .find(|c| c.name == "claude-pickup@a")
            .unwrap();
        assert!(pickup.detail.contains(LOCAL_CHECK_NOTE), "{pickup:?}");
    }

    /// A fake daemon that answers one request with `resp` (or never, when
    /// `None`), returning the request it received.
    #[cfg(unix)]
    fn fake_daemon(
        sock: &Path,
        resp: Option<crate::ipc::protocol::Response>,
    ) -> tokio::task::JoinHandle<crate::ipc::protocol::Request> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::UnixListener::bind(sock).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let req = serde_json::from_str(line.trim()).unwrap();
            match resp {
                Some(resp) => {
                    let mut out = serde_json::to_vec(&resp).unwrap();
                    out.push(b'\n');
                    stream.get_mut().write_all(&out).await.unwrap();
                }
                None => tokio::time::sleep(std::time::Duration::from_secs(30)).await,
            }
            req
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_daemon_report_asks_for_restart() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("pidash.sock");
        let server = fake_daemon(
            &sock,
            Some(crate::ipc::protocol::Response::Doctor(Report {
                checks: vec![],
                daemon_probe_version: None,
            })),
        );
        match report_from_daemon(&sock, None, dir.path(), DAEMON_DOCTOR_TIMEOUT).await {
            DaemonReport::Outdated(m) => assert!(m.contains("pidash restart"), "{m}"),
            other => panic!("expected outdated daemon, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn slow_daemon_is_a_timeout_not_a_missing_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("pidash.sock");
        let _server = fake_daemon(&sock, None);
        match report_from_daemon(
            &sock,
            None,
            dir.path(),
            std::time::Duration::from_millis(200),
        )
        .await
        {
            DaemonReport::Error(m) => assert!(m.contains("did not answer"), "{m}"),
            other => panic!("expected timeout error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn daemon_on_another_config_dir_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("pidash.sock");
        let server = fake_daemon(
            &sock,
            Some(crate::ipc::protocol::Response::Error(
                crate::ipc::protocol::RpcError {
                    code: CONFIG_MISMATCH_CODE,
                    message: "daemon uses /elsewhere".into(),
                },
            )),
        );
        let wanted = dir.path().join("other");
        match report_from_daemon(&sock, Some("beta"), &wanted, DAEMON_DOCTOR_TIMEOUT).await {
            DaemonReport::OtherConfig(m) => assert!(m.contains("/elsewhere"), "{m}"),
            other => panic!("expected other-config, got {other:?}"),
        }
        let req = server.await.unwrap();
        assert!(
            matches!(
                req,
                crate::ipc::protocol::Request::DoctorRun {
                    runner: Some(ref r),
                    config_dir: Some(ref c),
                } if r == "beta" && c == &wanted
            ),
            "{req:?}"
        );
    }

    #[tokio::test]
    async fn daemon_doctor_checks_config_dir_and_stamps_probe_version() {
        let (_dir, paths) = temp_paths();
        let other = paths.config_dir.join("elsewhere");
        match daemon_doctor(&paths, None, Some(other), DAEMON_DOCTOR_BUDGET).await {
            crate::ipc::protocol::Response::Error(e) => {
                assert_eq!(e.code, CONFIG_MISMATCH_CODE, "{e:?}")
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
        match daemon_doctor(
            &paths,
            None,
            Some(paths.config_dir.clone()),
            DAEMON_DOCTOR_BUDGET,
        )
        .await
        {
            crate::ipc::protocol::Response::Doctor(r) => {
                assert_eq!(r.daemon_probe_version, Some(CLAUDE_AUTH_PROBE_VERSION))
            }
            other => panic!("expected report, got {other:?}"),
        }
        match daemon_doctor(&paths, None, None, std::time::Duration::ZERO).await {
            crate::ipc::protocol::Response::Error(e) => {
                assert!(e.message.contains("did not finish"), "{e:?}")
            }
            other => panic!("expected budget error, got {other:?}"),
        }
    }

    #[test]
    fn managed_settings_override_user_provider_switch() {
        // Managed settings take precedence over user settings, key by key,
        // so an explicit "0" in managed settings switches Bedrock off even
        // when the user's settings turn it on.
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".claude")).unwrap();
        let user = home.path().join(".claude").join("settings.json");
        let managed_dir = tempfile::tempdir().unwrap();
        let managed = managed_dir.path().join("managed-settings.json");
        let env = AgentEnvSnapshot {
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let logged_out = || {
            Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".into()),
            })
        };
        std::fs::write(&user, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#).unwrap();
        std::fs::write(&managed, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"0"}}"#).unwrap();
        let p = build_claude_probe(logged_out(), &env, std::slice::from_ref(&managed), false);
        assert_eq!(p.settings_provider, ConfigKey::Absent);
        assert!(matches!(
            classify_claude_auth(&p),
            AuthStatus::NotLoggedIn(_)
        ));

        // Boolean false in managed settings is an explicit "off" too.
        std::fs::write(&managed, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":false}}"#).unwrap();
        let p = build_claude_probe(logged_out(), &env, std::slice::from_ref(&managed), false);
        assert_eq!(p.settings_provider, ConfigKey::Absent);

        // The reverse: user off, managed on, routes to Bedrock.
        std::fs::write(&user, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"0"}}"#).unwrap();
        std::fs::write(&managed, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#).unwrap();
        let p = build_claude_probe(logged_out(), &env, std::slice::from_ref(&managed), false);
        assert_eq!(p.settings_provider, ConfigKey::Found(managed.clone()));
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));

        // Managed settings that do not mention the switch leave the user's
        // value in force.
        std::fs::write(&user, r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1"}}"#).unwrap();
        std::fs::write(&managed, r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"0"}}"#).unwrap();
        let p = build_claude_probe(logged_out(), &env, std::slice::from_ref(&managed), false);
        assert_eq!(p.settings_provider, ConfigKey::Found(user.clone()));

        // An unreadable managed file could override the user's switch, so
        // the switch stays "could not verify", never "not logged in".
        std::fs::write(&managed, "{not json").unwrap();
        let p = build_claude_probe(logged_out(), &env, std::slice::from_ref(&managed), false);
        assert!(matches!(classify_claude_auth(&p), AuthStatus::Unknown(_)));
    }

    /// A fake agent binary that records its pid and hangs on `--version`.
    #[cfg(unix)]
    fn hanging_binary(dir: &Path) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let pid = dir.join("pid");
        let bin = dir.join("agent");
        let script = format!(
            "#!/bin/sh\necho $$ > '{pid}'\nexec sleep 300\n",
            pid = pid.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin, pid)
    }

    #[cfg(unix)]
    async fn wait_for_pid(file: &Path) -> i32 {
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(file)
                && let Ok(pid) = s.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("fake binary never started");
    }

    #[cfg(unix)]
    async fn is_gone(pid: i32) -> bool {
        // Reaped or a zombie: either way the probe no longer runs.
        for _ in 0..100 {
            let out = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&out.stdout);
            if stat.trim().is_empty() || stat.trim().starts_with('Z') {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        false
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn version_probe_is_killed_when_doctor_is_cancelled() {
        // The daemon's doctor budget drops `execute`; a hung `--version`
        // must not outlive it.
        let dir = tempfile::tempdir().unwrap();
        let (bin, pid_file) = hanging_binary(dir.path());
        let bin = bin.display().to_string();
        let probe = tokio::spawn(async move { check_version(&bin).await });
        let pid = wait_for_pid(&pid_file).await;
        probe.abort();
        let _ = probe.await;
        let gone = is_gone(pid).await;
        if !gone {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        assert!(gone, "version probe {pid} survived cancellation");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn version_probe_times_out_and_kills_a_hung_binary() {
        let dir = tempfile::tempdir().unwrap();
        let (bin, pid_file) = hanging_binary(dir.path());
        let started = std::time::Instant::now();
        let err = check_version_within(
            &bin.display().to_string(),
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
        assert!(err.to_string().contains("did not finish"), "{err}");
        let pid = wait_for_pid(&pid_file).await;
        assert!(is_gone(pid).await, "hung version probe {pid} still running");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_sends_an_absolute_config_dir() {
        // A relative `--config-dir` must not be resolved against the
        // daemon's working directory.
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("pidash.sock");
        let server = fake_daemon(
            &sock,
            Some(crate::ipc::protocol::Response::Doctor(Report {
                checks: vec![],
                daemon_probe_version: Some(CLAUDE_AUTH_PROBE_VERSION),
            })),
        );
        let relative = Path::new("relative-config");
        let _ = report_from_daemon(&sock, None, relative, DAEMON_DOCTOR_TIMEOUT).await;
        let req = server.await.unwrap();
        let expected = std::env::current_dir().unwrap().join(relative);
        assert!(
            matches!(
                req,
                crate::ipc::protocol::Request::DoctorRun { config_dir: Some(ref c), .. }
                    if c.is_absolute() && c == &expected
            ),
            "{req:?}"
        );
    }

    #[tokio::test]
    async fn daemon_doctor_refuses_a_relative_config_dir() {
        let (_dir, paths) = temp_paths();
        match daemon_doctor(
            &paths,
            None,
            Some(PathBuf::from("config")),
            DAEMON_DOCTOR_BUDGET,
        )
        .await
        {
            crate::ipc::protocol::Response::Error(e) => {
                assert_eq!(e.code, CONFIG_MISMATCH_CODE, "{e:?}")
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn claude_probe_does_not_block_on_a_fifo_credentials_file() {
        // Opening a FIFO for reading blocks until a writer appears; the
        // probe must treat it as unreadable instead of hanging.
        let home = tempfile::tempdir().unwrap();
        let claude_dir = home.path().join(".claude");
        std::fs::create_dir(&claude_dir).unwrap();
        for name in [".credentials.json", "settings.json"] {
            nix::unistd::mkfifo(
                &claude_dir.join(name),
                nix::sys::stat::Mode::from_bits_truncate(0o600),
            )
            .unwrap();
        }
        let env = AgentEnvSnapshot {
            home: Some(home.path().display().to_string()),
            ..AgentEnvSnapshot::default()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(build_claude_probe(None, &env, &[], false));
        });
        let probe = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("probe blocked on a FIFO");
        assert!(
            matches!(probe.credentials, CredentialsFile::Unrecognized(_)),
            "{probe:?}"
        );
        assert!(
            matches!(probe.settings_provider, ConfigKey::Unreadable(_)),
            "{probe:?}"
        );
    }

    #[tokio::test]
    async fn blocking_file_scan_is_bounded() {
        let started = std::time::Instant::now();
        let out = blocking_within(std::time::Duration::from_millis(100), || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            1
        })
        .await;
        assert_eq!(out, None);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            blocking_within(std::time::Duration::from_secs(5), || 7).await,
            Some(7)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn older_daemon_falls_back_locally_for_non_claude_runners() {
        let (_dir, paths) = temp_paths();
        let mut claude = runner("cc", "codex-missing");
        claude.agent.kind = AgentKind::ClaudeCode;
        claude.claude_code.binary = "/nonexistent/claude-missing".into();
        let cfg = cfg_with(vec![runner("beta", "codex-not-installed-beta"), claude]);
        crate::config::file::write_config(&paths, &cfg).unwrap();
        let outdated = || DaemonReport::Outdated("restart with `pidash restart`".into());

        // Codex-only request: the older daemon's missing Claude probe does
        // not matter, so the checks still run here.
        let report = resolve_report(&paths, Some("beta"), outdated())
            .await
            .unwrap();
        assert!(
            report.checks.iter().any(|c| c.name == "codex@beta"),
            "{report:?}"
        );
        // A request that includes a Claude runner still needs the daemon.
        let err = resolve_report(&paths, Some("cc"), outdated())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("pidash restart"), "{err}");
        let err = resolve_report(&paths, None, outdated()).await.unwrap_err();
        assert!(err.to_string().contains("pidash restart"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runner_deadline_keeps_completed_checks() {
        // One runner finishes, one hangs past the deadline: the report keeps
        // the finished runner's checks, names the runner that timed out, and
        // still runs the daemon-wide checks.
        let (dir, paths) = temp_paths();
        let (bin, _pid) = hanging_binary(dir.path());
        let mut slow = runner("slow", "codex-missing");
        slow.agent.kind = AgentKind::ClaudeCode;
        slow.claude_code.binary = bin.display().to_string();
        let cfg = cfg_with(vec![runner("fast", "codex-not-installed"), slow]);
        crate::config::file::write_config(&paths, &cfg).unwrap();

        let started = std::time::Instant::now();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let report = execute_until(&paths, None, Some(deadline)).await.unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(12));
        let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"codex@fast"), "{names:?}");
        assert!(names.contains(&"git"), "{names:?}");
        let timed_out = report
            .checks
            .iter()
            .find(|c| c.name == "runner-checks@slow")
            .unwrap_or_else(|| panic!("no timeout check in {names:?}"));
        assert!(!timed_out.ok && !timed_out.blocker, "{timed_out:?}");
        assert!(timed_out.detail.contains("--runner slow"), "{timed_out:?}");
    }
}
