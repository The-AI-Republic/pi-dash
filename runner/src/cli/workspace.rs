// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash workspace …` subcommands.

use clap::{Args, Subcommand};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_UNKNOWN, report_error};

#[derive(Debug, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Verify credentials and return the authenticated user. Used by `doctor`
    /// as the single end-to-end probe: token presence + validity + cloud
    /// reachability + TLS + active user are all proven on success.
    Me,
}

pub async fn run(args: WorkspaceArgs) -> i32 {
    let env = match CliEnv::from_env() {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        WorkspaceCommand::Me => cmd_me(&client).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_me(client: &ApiClient) -> Result<(), CliError> {
    // `/api/v1/users/me/` returns the authenticated user profile. No
    // workspace-scoped /members/me/ route exists today; the workspace
    // binding is implicit in the api_token itself (scoped at mint time).
    let resp = client.get("users/me/").await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}
