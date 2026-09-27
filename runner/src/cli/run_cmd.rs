// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash run …` subcommands — the agent's view of its own run.
//!
//! Distinct from the hidden `__run` daemon entry point (`cli::run`). Today
//! there is one verb, `yield`, which reports the run's outcome to the
//! ticking clock; see `.ai_design/ticking_relevance/design.md` §7.

use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

#[derive(Debug, Args)]
pub struct RunCmdArgs {
    #[command(subcommand)]
    pub command: RunCmdCommand,
}

#[derive(Debug, Subcommand)]
pub enum RunCmdCommand {
    /// Report this run's outcome to Pi Dash's ticking clock. Call it once,
    /// as the last `pidash` command of the run, after any state move.
    Yield(YieldArgs),
}

/// The §7 outcome vocabulary. Kept in sync with
/// `pi_dash.orchestration.scheduling.RUN_OUTCOMES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Outcome {
    /// Did work; more to do in this stage — tick again.
    Progressed,
    /// Asked the human a question (or told them the budget is spent) —
    /// the clock stops until a human acts.
    WaitingOnHuman,
    /// In Progress is waiting on CI or a merge — keep ticking.
    WaitingOnExternal,
    /// This stage's exit condition is met — the issue was moved on, or it
    /// is approved / verified and stays.
    Done,
    /// Cannot proceed; "Blocking the run" was followed.
    Blocked,
}

impl Outcome {
    pub fn as_wire(self) -> &'static str {
        match self {
            Outcome::Progressed => "progressed",
            Outcome::WaitingOnHuman => "waiting_on_human",
            Outcome::WaitingOnExternal => "waiting_on_external",
            Outcome::Done => "done",
            Outcome::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Args)]
pub struct YieldArgs {
    /// The outcome to report.
    #[arg(long, value_enum)]
    pub outcome: Outcome,
    /// One-line note stored with the outcome (optional).
    #[arg(long)]
    pub note: Option<String>,
    /// Override the run id. Defaults to `PIDASH_RUN_ID`, which the daemon
    /// sets on the agent process; only scripted use should need this.
    #[arg(long)]
    pub run_id: Option<String>,
}

pub async fn run(args: RunCmdArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        RunCmdCommand::Yield(y) => cmd_yield(&client, y).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// `POST workspaces/{slug}/agent-runs/{run_id}/yield/` with the outcome.
pub async fn cmd_yield(client: &ApiClient, args: YieldArgs) -> Result<(), CliError> {
    let run_id = resolve_run_id(args.run_id.as_deref(), client.env.run_id.as_deref())?;
    let path = yield_path(&client.env.workspace_slug, run_id);
    let mut body = json!({ "outcome": args.outcome.as_wire() });
    if let Some(note) = args
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        body["note"] = json!(note);
    }
    let resp = client.post(&path, &body).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// Explicit `--run-id` wins; otherwise the run the daemon put in the
/// environment. Neither → a clear error rather than a 404 from the cloud.
pub fn resolve_run_id<'a>(
    explicit: Option<&'a str>,
    from_env: Option<&'a str>,
) -> Result<&'a str, CliError> {
    explicit
        .or(from_env)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CliError::new(
                EXIT_INVALID,
                "run yield needs a run id: pass --run-id or run inside an agent run (PIDASH_RUN_ID)",
            )
        })
}

pub fn yield_path(workspace_slug: &str, run_id: &str) -> String {
    format!("workspaces/{workspace_slug}/agent-runs/{run_id}/yield/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_run_id_wins_over_env() {
        assert_eq!(resolve_run_id(Some("a"), Some("b")).unwrap(), "a");
        assert_eq!(resolve_run_id(None, Some("b")).unwrap(), "b");
    }

    #[test]
    fn missing_run_id_is_an_invalid_request() {
        let err = resolve_run_id(None, None).unwrap_err();
        assert_eq!(err.exit_code, EXIT_INVALID);
        let err = resolve_run_id(Some("  "), None).unwrap_err();
        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn outcomes_serialize_to_the_cloud_vocabulary() {
        assert_eq!(Outcome::WaitingOnHuman.as_wire(), "waiting_on_human");
        assert_eq!(Outcome::Done.as_wire(), "done");
    }

    #[test]
    fn yield_path_shape() {
        assert_eq!(
            yield_path("acme", "123e4567-e89b-12d3-a456-426614174000"),
            "workspaces/acme/agent-runs/123e4567-e89b-12d3-a456-426614174000/yield/"
        );
    }
}
