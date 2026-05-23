// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash config …` subcommands.

use clap::{Args, Subcommand};

use crate::cli::runner_ops;

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Set local CLI defaults.
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct SetArgs {
    #[command(subcommand)]
    pub command: SetCommand,
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// Set the local default project used by non-interactive CLI calls.
    #[command(name = "default-project")]
    DefaultProject {
        /// Project identifier or UUID.
        project: String,
    },
}

pub async fn run(args: ConfigArgs, paths: &crate::util::paths::Paths) -> anyhow::Result<()> {
    match args.command {
        ConfigCommand::Set(s) => match s.command {
            SetCommand::DefaultProject { project } => {
                runner_ops::write_cli_default_project(paths, &project)?;
                println!("Set default project to {project}.");
            }
        },
    }
    Ok(())
}
