//! Hidden `__run` subcommand: entry point that systemd / launchd exec.
//!
//! Not a user-facing verb. Users drive the daemon through service-lifecycle
//! verbs (`pidash install`, `start`, `stop`, `restart`, `status`). This handler
//! is what the generated unit files call via `ExecStart={exe} __run` (systemd)
//! and `<array><string>{exe}</string><string>__run</string></array>` (launchd).
//!
//! The body is the old `pidash start` foreground flow: load config, load any
//! legacy connection creds, run the supervisor loop, block until shutdown.

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
             Run `pidash auth login --url <URL>`, then `pidash runner add --project <PROJECT>`."
        );
    }

    let config = crate::config::file::load_config(paths)
        .context("failed to load config.toml; re-run enrollment if the file is corrupt")?;
    config
        .validate()
        .context("config.toml failed validation; refusing to start the daemon")?;

    if config.runners.is_empty() {
        anyhow::bail!(
            "no runners configured in config.toml — \
             register one with `pidash runner add --project <SLUG>`."
        );
    }

    // Legacy per-runner-token configs still need each runner's
    // credentials.toml. New shared-dev-machine-token configs keep the token in
    // [cli].token and intentionally do not create per-runner credentials files.
    if requires_runner_credentials(&args, &config) {
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
    }

    // The legacy top-level credentials.toml (connection-secret model)
    // is only populated by the legacy `pidash connect` flow. The
    // supervisor doesn't actually consume it (see Supervisor::run,
    // which destructures `creds: _creds`) — runner runtime auth comes
    // from the shared [cli].token or legacy per-runner refresh-token
    // files. We pass a placeholder when the legacy file is absent so
    // the new `pidash runner add` flow can start the daemon without
    // the legacy enrollment step.
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

fn has_shared_machine_token(config: &crate::config::schema::Config) -> bool {
    config
        .cli
        .as_ref()
        .and_then(|cli| cli.token.as_deref())
        .map(|token| token.starts_with("mt_"))
        .unwrap_or(false)
}

fn requires_runner_credentials(args: &Args, config: &crate::config::schema::Config) -> bool {
    !args.offline && !has_shared_machine_token(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Config, DaemonConfig};

    fn config_with_cli_token(token: Option<&str>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://example.com".into(),
                ..DaemonConfig::default()
            },
            runners: Vec::new(),
            cli: token.map(|token| crate::config::schema::CliSection {
                token: Some(token.into()),
                workspace_slug: Some("acme".into()),
                default_project: None,
            }),
        }
    }

    #[test]
    fn shared_machine_token_skips_legacy_runner_credentials_preflight() {
        let args = Args { offline: false };
        assert!(!requires_runner_credentials(
            &args,
            &config_with_cli_token(Some("mt_shared"))
        ));
    }

    #[test]
    fn non_machine_token_keeps_legacy_runner_credentials_preflight() {
        let args = Args { offline: false };
        assert!(requires_runner_credentials(
            &args,
            &config_with_cli_token(Some("pi_dash_api_user"))
        ));
        assert!(requires_runner_credentials(
            &args,
            &config_with_cli_token(None)
        ));
    }
}
