// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash run …` subcommands — the agent's view of its own run.
//!
//! Distinct from the hidden `__run` daemon entry point (`cli::run`). Two
//! verbs: `yield`, which reports the run's outcome to the ticking clock
//! (`.ai_design/ticking_relevance/design.md` §7), and `release-pin`, which
//! unsticks a queued run whose pinned runner will not free up
//! (PDASHOSS01-272).

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
    /// Clear a QUEUED run's runner pin so any eligible runner in its pod can
    /// take it. The recovery path for a run stuck behind a runner that will
    /// not free up — the pod's pin wait budget releases such a pin
    /// automatically, but only while some runner in the pod is idle.
    ReleasePin(ReleasePinArgs),
}

/// The §7 outcome vocabulary. Kept in sync with
/// `pi_dash.orchestration.scheduling.RUN_OUTCOMES`.
///
/// Outcomes are informational: they describe what the run did and never
/// control the ticking clock. Only `--stop-ticking` stops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Outcome {
    /// Did work; more to do in this stage.
    Progressed,
    /// Asked the human a question (or told them the budget is spent).
    WaitingOnHuman,
    /// Waiting on CI, a merge, or another external dependency.
    WaitingOnExternal,
    /// This run's turn is done — the stage's exit condition is met: the
    /// issue was moved on, or it is approved / verified and stays.
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
    /// The outcome to report. Informational only — no outcome stops or
    /// keeps the ticking clock.
    #[arg(long, value_enum)]
    pub outcome: Outcome,
    /// One-line note stored with the outcome (optional).
    #[arg(long)]
    pub note: Option<String>,
    /// Stop this issue's ticking clock: no further agent run should be
    /// scheduled on it. Use when the issue is finished or parked for a
    /// human. Outcomes alone never stop the clock.
    #[arg(long)]
    pub stop_ticking: bool,
    /// Override the run id. Defaults to `PIDASH_RUN_ID`, which the daemon
    /// sets on the agent process; only scripted use should need this.
    #[arg(long)]
    pub run_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ReleasePinArgs {
    /// The QUEUED run whose pin to clear. This is somebody else's stuck run,
    /// not the caller's own, so there is no environment default.
    pub run_id: String,
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
        RunCmdCommand::ReleasePin(r) => cmd_release_pin(&client, r).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// `POST workspaces/{slug}/agent-runs/{run_id}/yield/` with the outcome
/// and, when requested, the explicit stop-ticking signal. Without
/// `--stop-ticking` the key is absent entirely, so older servers that do
/// not know it see the same body as before.
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
    if args.stop_ticking {
        body["stop_ticking"] = json!(true);
    }
    let resp = client.post(&path, &body).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// `POST workspaces/{slug}/agent-runs/{run_id}/release-pin/`.
///
/// 409 when the run is not QUEUED or is not pinned, 404 when the caller may
/// not act on it — both surface through the usual error envelope, so a
/// coordinator that guessed wrong gets a machine-readable answer rather than
/// a silent no-op.
pub async fn cmd_release_pin(client: &ApiClient, args: ReleasePinArgs) -> Result<(), CliError> {
    let run_id = args.run_id.trim();
    if run_id.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "run release-pin needs a run id",
        ));
    }
    let path = release_pin_path(&client.env.workspace_slug, run_id);
    let resp = client.post(&path, &json!({})).await?;
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

pub fn release_pin_path(workspace_slug: &str, run_id: &str) -> String {
    format!("workspaces/{workspace_slug}/agent-runs/{run_id}/release-pin/")
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

    #[test]
    fn release_pin_path_shape() {
        assert_eq!(
            release_pin_path("acme", "123e4567-e89b-12d3-a456-426614174000"),
            "workspaces/acme/agent-runs/123e4567-e89b-12d3-a456-426614174000/release-pin/"
        );
    }
}
