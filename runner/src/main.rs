#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use pidash::cli;

fn main() -> Result<()> {
    // Warn before clap parsing so even an unknown-subcommand error (a wrong
    // build lacking the verb) carries the explanation (NEWPIDASH-363).
    pidash::util::shell::warn_if_runner_exe_mismatch();
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(cli::run(cli))
}
