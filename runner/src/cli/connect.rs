//! ``pidash connect`` — enroll this dev machine with the cloud.
//!
//! Replaces the registration half of the legacy ``configure``. After
//! enrollment the daemon has a long-lived connection secret and zero
//! runners. ``pidash runner add`` adds runners under it.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args as ClapArgs;
use std::io::{BufRead, IsTerminal, Write};

use crate::cloud::http::{
    RunnerCredentials, SharedHttpTransport, enroll_runner, write_runner_credentials,
};
use crate::config::file;
use crate::config::schema::{Config, Credentials, DaemonConfig, RunnerConfig, WorkspaceSection};
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Pi Dash cloud base URL (e.g. ``https://pidash.example.com``).
    #[arg(long)]
    pub url: String,

    /// One-time enrollment token shown by the cloud's "Add connection"
    /// button. Consumed on first use; if you lose it, delete the pending
    /// connection in the cloud UI and create a new one.
    #[arg(long)]
    pub token: String,

    /// Free-form host label. Defaults to the machine's hostname.
    #[arg(long)]
    pub host_label: Option<String>,

    /// Skip the post-enroll doctor + service install. Useful in CI.
    #[arg(long)]
    pub skip_service: bool,

    /// Skip ``loginctl enable-linger`` on Linux (avoids a sudo prompt
    /// in unattended installs).
    #[arg(long)]
    pub skip_linger: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    validate_cloud_url(&args.url)?;

    if paths.config_path().exists() {
        anyhow::bail!(
            "this machine is already enrolled. Run `pidash remove` first if you want to re-enroll."
        );
    }

    let host_label = args
        .host_label
        .clone()
        .unwrap_or_else(|| hostname().unwrap_or_else(|| "unknown-host".to_string()));

    let transport = SharedHttpTransport::new(args.url.clone())?;
    let resp = enroll_runner(&transport, &args.token, &host_label, None)
        .await
        .context("cloud enrollment failed")?;

    let runner_paths = paths.for_runner(resp.runner_id);
    runner_paths.ensure()?;
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
    .context("writing runner credentials failed")?;

    let config = Config {
        version: 2,
        daemon: DaemonConfig {
            cloud_url: args.url.clone(),
            log_level: "info".to_string(),
            log_retention_days: 14,
        },
        runners: vec![RunnerConfig {
            name: resp.runner_name.clone(),
            runner_id: resp.runner_id,
            workspace_slug: Some(resp.workspace_slug.clone()),
            project_slug: Some(resp.project_identifier.clone()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: paths.runner_dir(resp.runner_id).join("workspace"),
            },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            approval_policy: Default::default(),
        }],
    };
    file::write_config(paths, &config)?;

    // Legacy machine-scoped credentials are no longer used by the
    // HTTP transport, but a minimal file keeps older CLI surfaces from
    // crashing while the rest of the migration lands.
    let creds = Credentials {
        connection_id: resp.runner_id,
        connection_secret: String::new(),
        connection_name: Some(resp.runner_name.clone()),
        api_token: None,
        issued_at: Utc::now(),
    };
    file::write_credentials(paths, &creds)?;

    println!(
        "Enrolled runner {} ({}) (host_label={host_label}).",
        resp.runner_name, resp.runner_id
    );
    println!(
        "Workspace: {}; protocol v{}.",
        resp.workspace_slug, resp.protocol_version
    );

    if args.skip_service {
        println!("\nSkipping service install (--skip-service).");
        return Ok(());
    }

    let svc = crate::service::detect();
    svc.write_unit(paths).await?;
    svc.enable_and_start().await?;
    let boot_outcome = if args.skip_linger {
        crate::service::BootStartOutcome::Skipped
    } else {
        svc.ensure_boot_start().await
    };
    print_post_install_hints(&boot_outcome);

    Ok(())
}

fn print_post_install_hints(boot: &crate::service::BootStartOutcome) {
    use crate::service::BootStartOutcome::*;
    println!("\nService installed and running.");
    if cfg!(target_os = "linux") {
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
            }
            CheckFailed(err) => {
                println!("Couldn't check linger state ({err}).");
                println!("To start at boot, run: sudo loginctl enable-linger $USER");
            }
            EnableFailed(err) => {
                println!("Couldn't enable linger ({err}).");
                println!("To start at boot, run: sudo loginctl enable-linger $USER");
            }
            NotApplicable => {}
        }
    }
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Refuse cleartext http:// to non-localhost hosts (token leak surface).
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
            "refusing to enroll over cleartext http:// to non-localhost ({host}); use https://"
        );
    }
    anyhow::bail!("cloud URL must start with https:// (or http:// for localhost), got {url}")
}

/// Hook left for symmetry with future ``pidash connect --read-stdin`` flows.
#[allow(dead_code)]
fn read_line() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    print!("> ");
    std::io::stdout().flush().ok()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok()?;
    Some(line.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(validate_cloud_url("https://pi-dash.example").is_ok());
    }
}
