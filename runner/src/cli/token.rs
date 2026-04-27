//! `pidash token …` — manage the machine token (a.k.a. "connection")
//! that authenticates this daemon's WebSocket.
//!
//! See `.ai_design/n_runners_in_same_machine/design.md` §5. The token is
//! created in the Pi Dash UI; this command pastes the resulting
//! `token_id` + `token_secret` + title into `credentials.toml` so the
//! daemon picks them up on next start.
//!
//! `pidash install` (systemd unit) is unrelated — that's for OS service
//! lifecycle. `pidash token install` is for credential lifecycle.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use uuid::Uuid;

use crate::config::schema::TokenCredentials;
use crate::util::paths::Paths;

#[derive(Debug, Args)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub command: TokenCommand,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Install a token on this machine. Replaces any existing `[token]`
    /// block in credentials.toml; the daemon picks up the new values on
    /// next start.
    Install(InstallArgs),

    /// Print the configured token's id and title (no secret). Useful as
    /// a sanity check before/after `install`.
    Show,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Token id, as displayed in the Pi Dash UI.
    #[arg(long, env = "PIDASH_TOKEN_ID")]
    pub token_id: Uuid,

    /// Token secret. Shown once at creation time in the UI; this is
    /// the only chance to copy it. Subsequent `pidash token show`
    /// commands print only the id, never the secret.
    #[arg(long, env = "PIDASH_TOKEN_SECRET")]
    pub token_secret: String,

    /// Human-readable label for this connection. The Pi Dash UI shows
    /// it in the connections list; locally it's stamped on
    /// credentials.toml so `pidash token show` can echo it back.
    #[arg(long)]
    pub title: String,
}

pub async fn run(args: TokenArgs, paths: &Paths) -> Result<()> {
    match args.command {
        TokenCommand::Install(install) => run_install(install, paths).await,
        TokenCommand::Show => run_show(paths).await,
    }
}

async fn run_install(args: InstallArgs, paths: &Paths) -> Result<()> {
    let title = args.title.trim();
    if title.is_empty() {
        anyhow::bail!("--title cannot be empty");
    }
    if title.len() > 128 {
        anyhow::bail!("--title cannot exceed 128 characters");
    }
    if args.token_secret.trim().is_empty() {
        anyhow::bail!("--token-secret cannot be empty");
    }

    let mut creds = crate::config::file::load_credentials(paths)
        .context("loading credentials.toml — run `pidash configure --url ... --token ...` first")?;
    creds.token = Some(TokenCredentials {
        token_id: args.token_id,
        token_secret: args.token_secret.trim().to_string(),
        title: title.to_string(),
    });
    crate::config::file::write_credentials(paths, &creds)?;
    println!(
        "Installed token {} (\"{}\"). Restart the daemon to use it.",
        args.token_id, title,
    );
    Ok(())
}

async fn run_show(paths: &Paths) -> Result<()> {
    let creds = crate::config::file::load_credentials(paths)?;
    match creds.token {
        Some(token) => {
            println!("token_id: {}", token.token_id);
            println!("title:    {}", token.title);
        }
        None => {
            println!("no token configured. Run `pidash token install` to install one.");
        }
    }
    Ok(())
}
