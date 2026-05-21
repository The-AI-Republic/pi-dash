//! Hidden `__run` subcommand: entry point that systemd / launchd exec.
//!
//! Not a user-facing verb. Users drive the daemon through service-lifecycle
//! verbs (`pidash install`, `start`, `stop`, `restart`, `status`). This handler
//! is what the generated unit files call via `ExecStart={exe} __run` (systemd)
//! and `<array><string>{exe}</string><string>__run</string></array>` (launchd).
//!
//! The body is the old `pidash start` foreground flow: load config + creds,
//! run the supervisor loop, block until shutdown.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Run without opening the cloud WS (local-only mode for debugging).
    #[arg(long)]
    pub offline: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let config_path = paths.config_path();
    if !config_path.exists() {
        anyhow::bail!(
            "no config.toml at {config_path:?}. \
             Run `pidash auth login --url <URL>` (then `pidash runner add`) \
             or `pidash connect --url <URL> --token <ONE_TIME_TOKEN>` to enroll."
        );
    }

    let config = crate::config::file::load_config(paths).context(
        "failed to load config.toml; re-run enrollment if the file is corrupt",
    )?;
    config
        .validate()
        .context("config.toml failed validation; refusing to start the daemon")?;

    if config.runners.is_empty() {
        anyhow::bail!(
            "no runners configured in config.toml — \
             register one with `pidash runner add --project <SLUG>`."
        );
    }

    // Multi-runner: each [[runner]] has its own credentials.toml under
    // data_dir/runners/<id>/. The supervisor loads them per-runner; we
    // fail fast here so a missing file produces a clear startup error
    // instead of dying mid-spawn.
    for r in &config.runners {
        let p = paths.for_runner(r.runner_id).credentials_path();
        if !p.exists() {
            anyhow::bail!(
                "no credentials.toml at {p:?} for runner {} ({}). \
                 Re-add it with `pidash runner add`, or remove the [[runner]] block from config.toml.",
                r.name,
                r.runner_id
            );
        }
    }

    // The legacy top-level credentials.toml (connection-secret model)
    // is only populated by the legacy `pidash connect` flow. The
    // supervisor doesn't actually consume it (see Supervisor::run,
    // which destructures `creds: _creds`) — each runner uses its own
    // per-runner refresh-token credentials. We pass a placeholder when
    // the legacy file is absent so the new `pidash runner add` flow
    // can start the daemon without the legacy enrollment step.
    let creds = crate::config::file::load_credentials(paths).unwrap_or_else(|_| {
        use chrono::Utc;
        crate::config::schema::Credentials {
            connection_id: uuid::Uuid::nil(),
            connection_secret: String::new(),
            connection_name: None,
            api_token: None,
            issued_at: Utc::now(),
        }
    });

    let primary_name = config
        .primary_runner()
        .map(|r| r.name.as_str())
        .unwrap_or("(no runners)");
    tracing::info!(
        runner = %primary_name,
        runner_count = config.runners.len(),
        "starting daemon"
    );
    let opts = crate::daemon::Options {
        offline: args.offline,
    };
    crate::daemon::run(config, creds, paths.clone(), opts).await
}
