// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash run …` subcommands — the agent's view of its own run.
//!
//! Distinct from the hidden `__run` daemon entry point (`cli::run`). Two
//! verbs: `finish`, the one closing command of an issue run (comment,
//! hand-off, state move and outcome in one request; see
//! `.ai_design/lean_agent_workflow/design.md` §8.1), and `yield`, which
//! scheduled runs use to report their outcome
//! (`.ai_design/ticking_relevance/design.md` §7).

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};

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
    ///
    /// A scheduled run yields the same way: it has no issue and no clock,
    /// so the outcome is recorded on the run itself and the response says
    /// `"run_kind": "scheduler"`.
    Yield(YieldArgs),
    /// End an issue run with one command: post the comment, store the
    /// hand-off (In Progress runs), move the issue and report the result.
    /// Use it as the last action of the run. Pass the current state's name
    /// to `--to` to stay where the issue is.
    Finish(FinishArgs),
}

/// The result a run finishes with. `waiting_on_human` and `blocked` stop
/// the clock until a person acts; the rest keep it running (or stop it
/// because the issue left the stages that run automatically).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum FinishResult {
    /// This stage's work is finished and the issue was moved on.
    Done,
    /// Did work; more remains in this stage. A later run continues.
    Progressed,
    /// Waiting on checks, a merge, or another issue.
    WaitingOnExternal,
    /// A person must answer first.
    WaitingOnHuman,
    /// Cannot proceed until a person acts.
    Blocked,
}

impl FinishResult {
    pub fn as_wire(self) -> &'static str {
        match self {
            FinishResult::Done => "done",
            FinishResult::Progressed => "progressed",
            FinishResult::WaitingOnExternal => "waiting_on_external",
            FinishResult::WaitingOnHuman => "waiting_on_human",
            FinishResult::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Args)]
pub struct FinishArgs {
    /// Target state, by exact name. Pass the current state's name to stay.
    #[arg(long)]
    pub to: String,
    /// What this run achieved.
    #[arg(long, value_enum)]
    pub result: FinishResult,
    /// Markdown file posted as the issue comment: the answer, the findings,
    /// the test results, or the question to a person.
    #[arg(long)]
    pub comment_file: Option<PathBuf>,
    /// In Progress runs only: markdown file with the hand-off review and
    /// test read. Required for a move to In Review or In Test.
    #[arg(long)]
    pub handoff_file: Option<PathBuf>,
    /// One line for the run history.
    #[arg(long)]
    pub note: Option<String>,
    /// Name shown as the comment's speaker (`Codex`, `Claude Code`, …).
    /// Defaults to `AI Agent` on the server.
    #[arg(long)]
    pub as_agent: Option<String>,
    /// Override the run id. Defaults to `PIDASH_RUN_ID`.
    #[arg(long)]
    pub run_id: Option<String>,
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
    /// human. Outcomes alone never stop the clock. A scheduled run has no
    /// clock: the flag is accepted and ignored, and never disables the
    /// schedule.
    #[arg(long)]
    pub stop_ticking: bool,
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
        RunCmdCommand::Finish(f) => cmd_finish(&client, f).await,
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

/// Read one markdown file named on the command line. An empty file is an
/// error, not a silent "no comment": a `waiting_on_human` run would
/// otherwise stop the clock without asking anyone anything, and an empty
/// hand-off would fall back to an older one.
fn read_markdown(flag: &str, path: &Path) -> Result<String, CliError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CliError::new(
            EXIT_INVALID,
            format!("failed reading {flag} {}: {e}", path.display()),
        )
    })?;
    if text.trim().is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            format!(
                "{flag} {} is empty: write it, or leave the flag out",
                path.display()
            ),
        ));
    }
    Ok(text)
}

/// The `finish` request body: the two files read in, blank values left out.
pub fn finish_body(args: &FinishArgs, comment: Option<String>, handoff: Option<String>) -> Value {
    let mut body = json!({ "to": args.to.trim(), "result": args.result.as_wire() });
    let mut put = |key: &str, value: Option<String>| {
        if let Some(text) = value.filter(|t| !t.trim().is_empty()) {
            body[key] = json!(text);
        }
    };
    put("comment", comment);
    put("handoff", handoff);
    put(
        "note",
        args.note.as_deref().map(str::trim).map(str::to_owned),
    );
    put(
        "as_agent",
        args.as_agent.as_deref().map(str::trim).map(str::to_owned),
    );
    body
}

/// One line for the agent: where the issue is now and whether the clock runs.
pub fn finish_summary(resp: &Value) -> String {
    let state = resp.get("state").and_then(Value::as_str).unwrap_or("?");
    let result = resp.get("result").and_then(Value::as_str).unwrap_or("?");
    let clock = if resp.get("ticking").and_then(Value::as_bool) == Some(true) {
        "clock running"
    } else {
        "clock stopped"
    };
    format!("finished: {state} ({result}); {clock}")
}

/// `POST workspaces/{slug}/agent-runs/{run_id}/finish/`.
/// `waiting_on_human` and `blocked` stop the clock until a person acts, so
/// the run must say what that person has to do.
pub fn check_comment_for_result(args: &FinishArgs) -> Result<(), CliError> {
    let stops = matches!(
        args.result,
        FinishResult::WaitingOnHuman | FinishResult::Blocked
    );
    if stops && args.comment_file.is_none() {
        return Err(CliError::new(
            EXIT_INVALID,
            format!(
                "--result {} needs --comment-file saying what the person must do",
                args.result.as_wire()
            ),
        ));
    }
    Ok(())
}

pub async fn cmd_finish(client: &ApiClient, args: FinishArgs) -> Result<(), CliError> {
    check_comment_for_result(&args)?;
    let run_id = resolve_run_id(args.run_id.as_deref(), client.env.run_id.as_deref())?;
    let comment = args
        .comment_file
        .as_deref()
        .map(|p| read_markdown("--comment-file", p))
        .transpose()?;
    let handoff = args
        .handoff_file
        .as_deref()
        .map(|p| read_markdown("--handoff-file", p))
        .transpose()?;
    let path = finish_path(&client.env.workspace_slug, run_id);
    let body = finish_body(&args, comment, handoff);
    let resp = client.post(&path, &body).await?;
    println!("{}", finish_summary(&resp));
    Ok(())
}

pub fn finish_path(workspace_slug: &str, run_id: &str) -> String {
    format!("workspaces/{workspace_slug}/agent-runs/{run_id}/finish/")
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
                "this command needs a run id: pass --run-id or run inside an agent run (PIDASH_RUN_ID)",
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
    fn finish_results_match_the_server_vocabulary() {
        // NEWPIDASH-338 item 10: the runner cannot import the types crate
        // (no pidash-types dep), so the five wire names are pinned here as
        // literals against `rust-api/crates/types/src/orchestration/outcomes.rs`
        // (`OUTCOME_*`, source order below). A rename on either side fails
        // this test.
        let wires = [
            FinishResult::Done.as_wire(),
            FinishResult::Progressed.as_wire(),
            FinishResult::WaitingOnExternal.as_wire(),
            FinishResult::WaitingOnHuman.as_wire(),
            FinishResult::Blocked.as_wire(),
        ];
        assert_eq!(
            wires,
            [
                "done",
                "progressed",
                "waiting_on_external",
                "waiting_on_human",
                "blocked",
            ]
        );
    }

    #[test]
    fn yield_path_shape() {
        assert_eq!(
            yield_path("acme", "123e4567-e89b-12d3-a456-426614174000"),
            "workspaces/acme/agent-runs/123e4567-e89b-12d3-a456-426614174000/yield/"
        );
    }

    fn finish_args(extra: &[&str]) -> FinishArgs {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(subcommand)]
            cmd: RunCmdCommand,
        }
        let mut argv = vec!["pidash", "finish"];
        argv.extend_from_slice(extra);
        match <Cli as clap::Parser>::try_parse_from(argv)
            .expect("parses")
            .cmd
        {
            RunCmdCommand::Finish(f) => f,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn finish_parses_arguments() {
        let args = finish_args(&[
            "--to",
            "In Review",
            "--result",
            "waiting_on_human",
            "--comment-file",
            "c.md",
            "--handoff-file",
            "h.md",
            "--note",
            "n",
        ]);
        assert_eq!(args.to, "In Review");
        assert_eq!(args.result, FinishResult::WaitingOnHuman);
        assert_eq!(args.comment_file.as_deref(), Some(Path::new("c.md")));
        assert_eq!(args.handoff_file.as_deref(), Some(Path::new("h.md")));
        assert_eq!(args.note.as_deref(), Some("n"));
    }

    #[test]
    fn finish_requires_to_and_result() {
        #[derive(clap::Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            cmd: RunCmdCommand,
        }
        assert!(<Cli as clap::Parser>::try_parse_from(["pidash", "finish", "--to", "X"]).is_err());
        assert!(
            <Cli as clap::Parser>::try_parse_from(["pidash", "finish", "--result", "done"])
                .is_err()
        );
        assert!(
            <Cli as clap::Parser>::try_parse_from([
                "pidash", "finish", "--to", "X", "--result", "approved"
            ])
            .is_err()
        );
    }

    #[test]
    fn finish_body_reads_files_and_drops_blanks() {
        let dir = std::env::temp_dir().join(format!("pidash-finish-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let comment = dir.join("c.md");
        std::fs::write(&comment, "## Findings\n\n- one").unwrap();
        let args = finish_args(&[
            "--to",
            " AI Completed ",
            "--result",
            "done",
            "--comment-file",
            comment.to_str().unwrap(),
            "--note",
            "  ",
        ]);
        let text = read_markdown("--comment-file", args.comment_file.as_deref().unwrap()).unwrap();
        let body = finish_body(&args, Some(text), Some("   ".into()));
        assert_eq!(
            body,
            json!({"to": "AI Completed", "result": "done", "comment": "## Findings\n\n- one"})
        );
        let missing = read_markdown("--handoff-file", &dir.join("nope.md")).unwrap_err();
        assert_eq!(missing.exit_code, EXIT_INVALID);
        let empty = dir.join("empty.md");
        std::fs::write(&empty, " \n\n").unwrap();
        let err = read_markdown("--comment-file", &empty).unwrap_err();
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.to_string().contains("is empty"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stop_result_needs_a_comment_file() {
        for result in ["waiting_on_human", "blocked"] {
            let args = finish_args(&["--to", "In Progress", "--result", result]);
            let err = check_comment_for_result(&args).unwrap_err();
            assert_eq!(err.exit_code, EXIT_INVALID, "{result}");
            let args = finish_args(&[
                "--to",
                "In Progress",
                "--result",
                result,
                "--comment-file",
                "q.md",
            ]);
            assert!(check_comment_for_result(&args).is_ok(), "{result}");
        }
        let args = finish_args(&["--to", "In Progress", "--result", "progressed"]);
        assert!(check_comment_for_result(&args).is_ok());
    }

    #[test]
    fn finish_path_and_summary() {
        assert_eq!(
            finish_path("acme", "123e4567-e89b-12d3-a456-426614174000"),
            "workspaces/acme/agent-runs/123e4567-e89b-12d3-a456-426614174000/finish/"
        );
        assert_eq!(
            finish_summary(&json!({"state": "In Review", "result": "done", "ticking": true})),
            "finished: In Review (done); clock running"
        );
        assert_eq!(
            finish_summary(&json!({"state": "In Progress", "result": "blocked", "ticking": false})),
            "finished: In Progress (blocked); clock stopped"
        );
    }
}
