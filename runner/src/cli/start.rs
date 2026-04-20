//! `pidash start` — start the installed service.
//!
//! Delegates to systemd (`systemctl --user start pidash.service`) on Linux and
//! launchd (`launchctl kickstart -k gui/<uid>/so.pidash.daemon`) on macOS. The
//! foreground daemon entry point moved to the hidden `pidash __run`
//! subcommand (`cli/run.rs`), which is what the generated unit files exec.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, _paths: &Paths) -> Result<()> {
    crate::service::detect().start().await
}
