//! `pidash auth <login|status|logout>` — user-identity flow.
//!
//! Mints, inspects, and revokes the `[cli].token` written to
//! `~/.config/pidash/config.toml`. Strictly the user-identity side: it
//! does not touch `[[runner]]` blocks or `credentials.toml`. Runner
//! credentials are minted separately by `pidash runner add`, which uses
//! the CLI token populated here to authorise itself to the cloud.

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};

pub mod login;
pub mod logout;
pub mod status;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Authenticate this host with Pi Dash cloud via device-code OAuth.
    /// Stores a user-scoped CLI token in `~/.config/pidash/config.toml`.
    Login(login::Args),
    /// Show who this host is logged in as and which runners are
    /// registered here.
    Status(status::Args),
    /// Revoke the CLI token server-side and clear it locally. Leaves
    /// runner registrations untouched.
    Logout(logout::Args),
}

pub async fn run(args: AuthArgs, paths: &Paths) -> Result<()> {
    match args.command {
        AuthCommand::Login(a) => login::run(a, paths).await,
        AuthCommand::Status(a) => status::run(a, paths).await,
        AuthCommand::Logout(a) => logout::run(a, paths).await,
    }
}
