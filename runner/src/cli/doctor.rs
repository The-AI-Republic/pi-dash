use anyhow::Result;
use clap::Args as ClapArgs;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::process::Command;

use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Emit a machine-readable JSON report.
    #[arg(long)]
    pub json: bool,
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
    let report = execute(paths).await?;
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

pub async fn execute(paths: &Paths) -> Result<Report> {
    let mut checks = Vec::new();

    // Agent binary check — which CLI we verify depends on `agent.kind`.
    // Runs without a config file fall back to Codex (the historical default)
    // so `pidash doctor` keeps working pre-`pidash configure`.
    let cfg = file::load_config_opt(paths)?;
    let agent_kind = cfg
        .as_ref()
        .map(|c| c.agent.kind)
        .unwrap_or(crate::config::schema::AgentKind::Codex);
    match agent_kind {
        crate::config::schema::AgentKind::Codex => {
            let codex_binary = cfg
                .as_ref()
                .map(|c| c.codex.binary.clone())
                .unwrap_or_else(|| "codex".to_string());
            match check_version(&codex_binary).await {
                Ok(detail) => checks.push(Check {
                    name: "codex".to_string(),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: "codex".to_string(),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            match check_codex_auth(&codex_binary).await {
                Ok(detail) => checks.push(Check {
                    name: "codex-auth".to_string(),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: "codex-auth".to_string(),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
        }
        crate::config::schema::AgentKind::ClaudeCode => {
            let claude_binary = cfg
                .as_ref()
                .map(|c| c.claude_code.binary.clone())
                .unwrap_or_else(|| "claude".to_string());
            match check_version(&claude_binary).await {
                Ok(detail) => checks.push(Check {
                    name: "claude".to_string(),
                    ok: true,
                    detail,
                    blocker: true,
                }),
                Err(e) => checks.push(Check {
                    name: "claude".to_string(),
                    ok: false,
                    detail: e.to_string(),
                    blocker: true,
                }),
            }
            // There's no unattended `claude whoami`; Claude Code assumes the
            // user is already signed in via the CLI's own onboarding. Surface
            // a non-blocking note so the operator knows where to look.
            checks.push(Check {
                name: "claude-auth".to_string(),
                ok: true,
                detail: format!(
                    "assumed ok (run `{} /login` if runs fail with auth errors)",
                    claude_binary
                ),
                blocker: false,
            });
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
        match check_cloud(&cfg.runner.cloud_url).await {
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

/// Shared `<binary> --version` check. Works for both `codex` and `claude`
/// since both print a short version line on stdout and exit 0 on success.
async fn check_version(binary: &str) -> Result<String> {
    let out = Command::new(binary)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
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
    let out = Command::new(binary)
        .args(["account", "status"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok();
    if let Some(o) = out {
        if o.status.success() {
            return Ok(String::from_utf8_lossy(&o.stdout).trim().to_string());
        }
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
