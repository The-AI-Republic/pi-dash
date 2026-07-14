// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash ai <command>` — talk to the connected Pi Dash cloud AI assistant.
//!
//! Unlike the `issue`/`comment`/`state` CRUD verbs (thin one-shot REST calls
//! that emit JSON via `run_crud`), this command drives the assistant's
//! *asynchronous* turn lifecycle and is meant to be read by a human at a
//! terminal. It therefore:
//!   1. opens a fresh assistant thread,
//!   2. posts the user's command as a message (HTTP 202 → a queued turn),
//!   3. polls the thread until the turn finishes, streaming tool activity to
//!      stderr, and finally
//!   4. prints the assistant's reply on stdout.
//!
//! The assistant is BYOK-only: if the user has not configured an LLM provider
//! and key in Pi Dash, the message POST comes back `422 llm_config_missing`.
//! We detect that specific case and print a clear, actionable reason rather
//! than a raw HTTP error — that requirement is the whole point of PDASHOSS01-35.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Args;
use serde_json::Value;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

#[derive(Debug, Args)]
pub struct AiArgs {
    /// The command / question to send to the cloud AI assistant. Everything
    /// after `ai` is joined into one message, so quoting is optional:
    /// `pidash ai "create an issue for the login bug"` and
    /// `pidash ai create an issue for the login bug` are equivalent.
    #[arg(trailing_var_arg = true, required = true, value_name = "COMMAND")]
    pub message: Vec<String>,

    /// Emit the raw turn result as a single JSON document instead of the
    /// human-readable transcript.
    #[arg(long)]
    pub json: bool,

    /// Give up waiting for the assistant after this many seconds. The turn may
    /// still complete in the cloud; only the local wait is bounded.
    #[arg(long, default_value_t = 180)]
    pub timeout: u64,
}

/// Poll cadence while a turn is in flight. Kept short enough to feel live but
/// long enough not to hammer the API for a multi-tool turn.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

pub async fn run(args: AiArgs, paths: &crate::util::paths::Paths) -> Result<()> {
    let message = args.message.join(" ");
    let message = message.trim();
    if message.is_empty() {
        eprintln!("{}", err_json("the AI command must not be empty"));
        std::process::exit(EXIT_INVALID);
    }

    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => std::process::exit(report_error(&e)),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => std::process::exit(report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}")))),
    };

    match drive(
        &client,
        message,
        args.json,
        Duration::from_secs(args.timeout),
    )
    .await
    {
        Ok(code) => {
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => std::process::exit(report_error(&e)),
    }
}

/// Run the full open-thread → post → poll → render flow. Returns the process
/// exit code (0 on a completed turn, non-zero if the assistant reported an
/// error), or a `CliError` for transport/setup failures.
async fn drive(
    client: &ApiClient,
    message: &str,
    json: bool,
    timeout: Duration,
) -> Result<i32, CliError> {
    let slug = &client.env.workspace_slug;

    // 1. Open a fresh thread for this one-shot command.
    let thread = client
        .post(
            &format!("workspaces/{slug}/assistant/threads/"),
            &Value::Object(Default::default()),
        )
        .await?;
    let thread_id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(EXIT_UNKNOWN, "assistant returned a thread without an id"))?
        .to_string();

    // 2. Post the message. This is where BYOK-not-configured surfaces.
    let post_body = serde_json::json!({ "content": message });
    let posted = match client
        .post(
            &format!("workspaces/{slug}/assistant/threads/{thread_id}/messages/"),
            &post_body,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => return Err(explain_post_error(e)),
    };
    let user_seq = posted
        .get("message")
        .and_then(|m| m.get("seq"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let turn_id = posted
        .get("turn")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if !json {
        eprintln!("Assistant is working…");
    }

    // 3. Poll until the turn finishes (or we time out), streaming tool activity.
    let deadline = Instant::now() + timeout;
    let mut printed: HashSet<i64> = HashSet::new();
    loop {
        // Progress: surface any new tool activity / errors as they land.
        // Only fetch progress when we will actually render it — in `--json`
        // mode the transcript is printed once at the end, so skip the extra
        // per-poll request. `!json` must come first so it short-circuits.
        if !json && let Ok(msgs) = fetch_messages(client, slug, &thread_id, user_seq).await {
            for m in &msgs {
                stream_progress(m, &mut printed);
            }
        }

        let active = thread_has_active_turn(client, slug, &thread_id).await?;
        if !active {
            break;
        }
        if Instant::now() >= deadline {
            eprintln!(
                "{}",
                err_json(&format!(
                    "timed out after {}s waiting for the assistant; the turn may still be \
                     running — check Pi Dash to see the result",
                    timeout.as_secs()
                ))
            );
            return Ok(EXIT_UNKNOWN);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    // 4. Turn finished — fetch the authoritative final transcript and render.
    let final_msgs = fetch_messages(client, slug, &thread_id, user_seq).await?;
    render(&thread_id, &turn_id, &final_msgs, json)
}

/// Translate a failed message POST into a clear, human-facing reason. The
/// headline case is BYOK-not-configured (`422 llm_config_missing`), which the
/// issue calls out specifically: the assistant exists but has no LLM key.
fn explain_post_error(err: CliError) -> CliError {
    let code = err.detail.as_deref().and_then(error_code);
    match code.as_deref() {
        Some("llm_config_missing") => CliError::new(
            EXIT_INVALID,
            "the Pi Dash AI assistant is not set up yet. It is BYOK (bring your own key): \
             add your LLM provider and API key under Settings → AI assistant in Pi Dash, \
             then run `pidash ai` again",
        ),
        Some("turn_active") => CliError::new(
            EXIT_INVALID,
            "the assistant is already handling another turn on this thread — try again shortly",
        ),
        Some("thread_full") => CliError::new(
            EXIT_INVALID,
            "this assistant thread is full; start a new one and retry",
        ),
        Some("message_too_long") => CliError::new(
            EXIT_INVALID,
            "the AI command is too long; shorten it and retry",
        ),
        _ => err,
    }
}

/// Pull the `error` code out of an upstream JSON error body, if present.
fn error_code(detail: &str) -> Option<String> {
    serde_json::from_str::<Value>(detail)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
}

async fn thread_has_active_turn(
    client: &ApiClient,
    slug: &str,
    thread_id: &str,
) -> Result<bool, CliError> {
    let detail = client
        .get(&format!("workspaces/{slug}/assistant/threads/{thread_id}/"))
        .await?;
    Ok(detail
        .get("has_active_turn")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

async fn fetch_messages(
    client: &ApiClient,
    slug: &str,
    thread_id: &str,
    after_seq: i64,
) -> Result<Vec<Value>, CliError> {
    let resp = client
        .get(&format!(
            "workspaces/{slug}/assistant/threads/{thread_id}/messages/?after={after_seq}&limit=200"
        ))
        .await?;
    Ok(match resp {
        Value::Array(a) => a,
        _ => Vec::new(),
    })
}

/// Print a one-line note to stderr for each tool step / error as it first
/// appears, so a long turn shows progress instead of hanging silently. The
/// assistant's own reply is intentionally *not* streamed here — it is printed
/// once, authoritatively, in `render`.
fn stream_progress(msg: &Value, printed: &mut HashSet<i64>) {
    let seq = msg.get("seq").and_then(Value::as_i64).unwrap_or(-1);
    let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
    let content = msg
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    match role {
        "tool_call" | "tool_result" => {
            if printed.insert(seq) && !content.is_empty() {
                eprintln!("  · {content}");
            }
        }
        "error" => {
            if printed.insert(seq) && !content.is_empty() {
                eprintln!("  ✗ {content}");
            }
        }
        _ => {}
    }
}

/// Render the finished turn. Returns the process exit code: non-zero when the
/// assistant reported an error so scripts can branch on it.
fn render(thread_id: &str, turn_id: &str, msgs: &[Value], json: bool) -> Result<i32, CliError> {
    if json {
        let out = serde_json::json!({
            "thread_id": thread_id,
            "turn_id": turn_id,
            "messages": msgs,
        });
        println!(
            "{}",
            serde_json::to_string(&out).expect("serialize JSON value")
        );
        let errored = msgs
            .iter()
            .any(|m| m.get("role").and_then(Value::as_str) == Some("error"));
        return Ok(if errored { EXIT_UNKNOWN } else { 0 });
    }

    let mut answers: Vec<&str> = Vec::new();
    let mut errors: Vec<&str> = Vec::new();
    for m in msgs {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        let content = m
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if content.is_empty() {
            continue;
        }
        match role {
            "assistant" => answers.push(content),
            "error" => errors.push(content),
            _ => {}
        }
    }

    if !errors.is_empty() {
        eprintln!("\nThe assistant could not complete the request:");
        for e in &errors {
            eprintln!("  {e}");
        }
        return Ok(EXIT_UNKNOWN);
    }

    if answers.is_empty() {
        // Turn finished but produced no assistant text — unusual, but don't
        // pretend success with empty output.
        eprintln!("The assistant finished without a reply.");
        return Ok(EXIT_UNKNOWN);
    }

    println!("{}", answers.join("\n\n"));
    Ok(0)
}

/// Uniform `{"error": "..."}` line for the stderr contract shared with the
/// other subcommands.
fn err_json(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_code_extracts_known_field() {
        assert_eq!(
            error_code(r#"{"error":"llm_config_missing","detail":"x"}"#).as_deref(),
            Some("llm_config_missing")
        );
    }

    #[test]
    fn error_code_none_for_non_json_or_missing() {
        assert!(error_code("not json").is_none());
        assert!(error_code(r#"{"detail":"x"}"#).is_none());
    }

    #[test]
    fn explain_post_error_maps_byok_missing_to_setup_guidance() {
        let raw = CliError::new(EXIT_INVALID, "invalid request").with_detail(
            r#"{"error":"llm_config_missing","detail":"Configure your AI provider."}"#,
        );
        let mapped = explain_post_error(raw);
        assert_eq!(mapped.exit_code, EXIT_INVALID);
        assert!(
            mapped.message.contains("BYOK"),
            "message: {}",
            mapped.message
        );
        assert!(mapped.message.to_lowercase().contains("api key"));
    }

    #[test]
    fn explain_post_error_passes_through_unknown_codes() {
        let raw = CliError::new(EXIT_UNKNOWN, "server error").with_detail("boom");
        let mapped = explain_post_error(raw);
        // Unknown/unparseable detail keeps the original error verbatim.
        assert_eq!(mapped.exit_code, EXIT_UNKNOWN);
        assert_eq!(mapped.message, "server error");
    }

    #[test]
    fn render_prints_assistant_and_returns_ok() {
        let msgs = vec![
            json!({"role": "tool_result", "content": "created ENG-9", "seq": 2}),
            json!({"role": "assistant", "content": "Done — I filed ENG-9.", "seq": 3}),
        ];
        let code = render("t", "u", &msgs, false).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn render_error_message_yields_nonzero() {
        let msgs = vec![json!({"role": "error", "content": "provider timeout", "seq": 2})];
        let code = render("t", "u", &msgs, false).unwrap();
        assert_eq!(code, EXIT_UNKNOWN);
    }

    #[test]
    fn render_empty_turn_is_nonzero() {
        let msgs: Vec<Value> = vec![];
        assert_eq!(render("t", "u", &msgs, false).unwrap(), EXIT_UNKNOWN);
    }

    #[test]
    fn stream_progress_prints_each_seq_once() {
        let mut printed = HashSet::new();
        let m = json!({"role": "tool_result", "content": "x", "seq": 5});
        stream_progress(&m, &mut printed);
        stream_progress(&m, &mut printed);
        assert!(printed.contains(&5));
        assert_eq!(printed.len(), 1);
    }
}
