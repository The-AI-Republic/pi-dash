use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::cloud::register::{RegisterRequest, register};
use crate::config::schema::{Config, Credentials};
use crate::util::paths::Paths;

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

    /// Skip on-install doctor checks (not recommended).
    #[arg(long)]
    pub skip_doctor: bool,
}

/// Inputs for the core registration flow. `cli::install::run` also builds one
/// of these — once via clap, once via interactive prompts.
pub struct RegisterInputs {
    pub url: String,
    pub token: String,
    pub name: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub skip_doctor: bool,
}

impl From<Args> for RegisterInputs {
    fn from(a: Args) -> Self {
        Self {
            url: a.url,
            token: a.token,
            name: a.name,
            working_dir: a.working_dir,
            skip_doctor: a.skip_doctor,
        }
    }
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    execute(args.into(), paths, /* print_next_hint = */ true).await
}

/// Core registration flow — hits the cloud register endpoint, persists
/// `config.toml` + `credentials.toml`, and optionally runs the doctor.
///
/// `print_next_hint` controls whether the trailing "Next: …" banner appears.
/// `pidash configure` sets it true; when `pidash install` chains into this,
/// it sets false because install itself is doing the "next" step.
pub async fn execute(inputs: RegisterInputs, paths: &Paths, print_next_hint: bool) -> Result<()> {
    validate_cloud_url(&inputs.url)?;
    let name = inputs
        .name
        .clone()
        .unwrap_or_else(|| hostname_default().unwrap_or_else(|| "runner".to_string()));
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let version = crate::RUNNER_VERSION.to_string();

    let resp = register(
        &inputs.url,
        &inputs.token,
        &RegisterRequest {
            runner_name: name.clone(),
            os: os.clone(),
            arch: arch.clone(),
            version: version.clone(),
            protocol_version: crate::PROTOCOL_VERSION,
        },
    )
    .await
    .context("cloud registration failed")?;

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
            name,
            cloud_url: inputs.url.clone(),
            workspace_slug: resp.workspace_slug.clone(),
        },
        workspace: crate::config::schema::WorkspaceSection { working_dir },
        codex: crate::config::schema::CodexSection::default(),
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
        let report = crate::cli::doctor::execute(paths).await?;
        report.print_compact();
        if report.has_blockers() {
            eprintln!("\nWarning: some preflight checks failed. Resolve them before starting.");
        }
    }

    if print_next_hint {
        println!(
            "\nRegistered runner '{}' with id {}.\nNext: `pidash install` to run as a background service.\n",
            config.runner.name, creds.runner_id,
        );
    } else {
        println!(
            "\nRegistered runner '{}' with id {}.",
            config.runner.name, creds.runner_id,
        );
    }
    Ok(())
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

fn hostname_default() -> Option<String> {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return Some(h);
        }
    }
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
}
