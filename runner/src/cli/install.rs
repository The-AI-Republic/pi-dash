//! `pidash install` — write the OS service unit (systemd user unit or launchd
//! agent) and enable it.
//!
//! PR 1 scope: moves the old `pidash service install` behavior up to a
//! top-level verb with no behavior change. The fresh-install gate + interactive
//! `configure` chaining lands in PR 2.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    crate::service::detect().install(paths).await
}
