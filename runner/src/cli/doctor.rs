use anyhow::Result;
use clap::Args as ClapArgs;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub blocker: bool,
}

impl Report {
    pub fn has_blockers(&self) -> bool {
        self.checks.iter().any(|c| c.blocker && !c.ok)
    }

    pub fn print_compact(&self) {
        for c in &self.checks {
            let mark = if c.ok { "✓" } else { "✗" };
            println!(
                "  {mark} {name:<14} {detail}",
                mark = mark,
                name = c.name,
                detail = c.detail
            );
        }
    }
}

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
async fn run_agent_checks(
    checks: &mut Vec<Check>,
    prefix: Option<&str>,
    agent_kind: crate::config::schema::AgentKind,
    codex_binary: &str,
    claude_binary: &str,
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
            checks.push(Check {
                name: tag("claude-auth"),
                ok: true,
                detail: format!(
                    "assumed ok (run `{} /login` if runs fail with auth errors)",
                    claude_binary
                ),
                blocker: false,
            });
        }
    }
}

/// Shared `<binary> --version` check. Works for both `codex` and `claude`
/// since both print a short version line on stdout and exit 0 on success.
///
/// Runs through the same login-shell wrapper the daemon uses so this check
/// reflects what the daemon will actually see at spawn time — not just
/// whether the binary is on the caller's interactive `PATH`.
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
        AgentKind, ClaudeCodeSection, CodexSection, Config, DaemonConfig, RunnerConfig,
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
            approval_policy: Default::default(),
        }
    }

    fn cfg_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "http://127.0.0.1:1".into(),
                log_level: "info".into(),
                log_retention_days: 14,
            },
            runners,
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

    #[test]
    fn agent_kind_drives_check_set() {
        // Sanity: switching an agent flips which check tags appear.
        // We assert via the helper function's tag construction so we
        // don't have to spawn binaries to see the difference.
        let mut codex_checks: Vec<Check> = Vec::new();
        let mut claude_checks: Vec<Check> = Vec::new();
        // Run synchronously inside a tokio rt for the async helper.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(run_agent_checks(
            &mut codex_checks,
            None,
            AgentKind::Codex,
            "codex-missing",
            "claude-missing",
        ));
        rt.block_on(run_agent_checks(
            &mut claude_checks,
            None,
            AgentKind::ClaudeCode,
            "codex-missing",
            "claude-missing",
        ));
        assert!(codex_checks.iter().any(|c| c.name == "codex"));
        assert!(codex_checks.iter().any(|c| c.name == "codex-auth"));
        assert!(claude_checks.iter().any(|c| c.name == "claude"));
        assert!(claude_checks.iter().any(|c| c.name == "claude-auth"));
    }
}
