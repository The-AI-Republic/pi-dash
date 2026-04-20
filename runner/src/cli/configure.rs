use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use crate::cloud::register::{RegisterError, RegisterRequest, register};
use crate::config::schema::{Config, Credentials};
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
    // typed.
    let (resp, final_name) = {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let attempt_name = inputs
                .name
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

