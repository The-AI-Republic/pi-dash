#![forbid(unsafe_code)]

use anyhow::Result;
use pi_dash_runner::cli;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(cli::run(cli))
}
