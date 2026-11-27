//! CLI guidance for runner chat sessions (NEWPIDASH-402).
//!
//! Dispatched issue runs and scheduler runs receive the `pidash-cli`
//! prompt section through the server-side composer. Chat never goes
//! through the composer (desktop chat drives the daemon's engine
//! directly over local IPC; cloud-relayed chat goes runner to engine),
//! so without this module the chat agent never learns the `pidash` CLI
//! and fumbles with `--help` exploration.
//!
//! The guidance is rendered at runtime from the **same source file**
//! the composer uses
//! (`rust-api/crates/services/src/prompting/sections/pidash-cli.md`,
//! embedded with `include_str!`), with `run.kind == "chat"` so the
//! run-only material (env defaults, workpad, `run finish` / `run yield`,
//! lifecycle) is left out. One source of truth: there is no forked copy
//! to drift. A unit test renders the same source with a minimal context
//! (no issue, no project), so an unguarded `{{ issue.* }}` reference in
//! a chat-taken branch fails here instead of in production.
//!
//! Delivery: every chat lane prefixes the **first** turn of an agent
//! session (or subprocess) with this text and sends later turns
//! verbatim. Prefixing keeps one code path for all seven agent kinds:
//! the pinned Codex app-server schema has no developer-instructions
//! field, and the Claude spawn is shared with one-shot assignment runs
//! that must not receive chat guidance. The prefix never reaches the
//! user-visible history: history frames (`ChatMessageStarted`,
//! `ChatMessageCompleted`) carry no user content, only the engine's
//! own deltas and replies.

use std::sync::OnceLock;

/// The `pidash-cli` section source, shared with the server-side
/// composer. Build-time embedding: editing the section changes chat
/// guidance with no further step, and deleting the file breaks the
/// build instead of silently serving stale text.
const SECTION_SOURCE: &str =
    include_str!("../../rust-api/crates/services/src/prompting/sections/pidash-cli.md");

/// Separator between the injected guidance and the user's own first
/// message. The guidance renders first so the engine treats it as
/// standing instructions for the session.
const SEPARATOR: &str = "\n\n---\n\n";

/// Minimal static fallback when the embedded section fails to render
/// (a broken template — the unit tests below fail on the same
/// source, so this only fires on a build that skipped them). Chat
/// still works; the agent just finds `pidash` through `pidash docs`.
const FALLBACK_GUIDANCE: &str = "## Pi Dash CLI (`pidash`)\n\n`pidash` is your only channel to Pi Dash. It is already authenticated **as this runner's owner**. Run `pidash docs <topic>` (or `pidash <command> --help`) before first use. There is no current issue or run: pass the project and issue identifiers explicitly.";

fn front_matter_end(source: &str) -> Option<usize> {
    let mut fences = 0;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches('\n') == "---" {
            fences += 1;
            if fences == 2 {
                return Some(offset);
            }
        }
    }
    None
}

/// The section body: the source with its `---` front-matter block
/// stripped, mirroring the registry's parsing (the body keeps a
/// single trailing newline there; here the exact tail does not
/// matter because the separator follows).
pub fn section_body() -> &'static str {
    static BODY: OnceLock<&'static str> = OnceLock::new();
    BODY.get_or_init(|| {
        let end =
            front_matter_end(SECTION_SOURCE).expect("pidash-cli section keeps its front-matter");
        &SECTION_SOURCE[end..]
    })
}

/// Render the chat variant of the section: `run.kind == "chat"` with
/// no issue or project, so run-only branches stay out. Pure function
/// over its inputs for testing; production passes
/// [`section_body`] and the current time.
pub fn render_chat_guidance_for(body: &str, rendered_at: &str) -> Result<String, minijinja::Error> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let ctx = serde_json::json!({
        "run": {
            "kind": "chat",
            "rendered_at": rendered_at,
        },
    });
    env.render_str(body, ctx)
}

/// The chat guidance text, rendered once per process. Infallible at
/// runtime: a broken template falls back to [`FALLBACK_GUIDANCE`]
/// (and the unit tests fail on the same source).
pub fn chat_guidance() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(
            || match render_chat_guidance_for(section_body(), &now_rfc3339()) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!(error = %e, "chat guidance render failed; using fallback");
                    FALLBACK_GUIDANCE.to_string()
                }
            },
        )
        .clone()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Prefix the first turn of a chat session with the CLI guidance.
/// Later turns pass through verbatim; the caller tracks first-turn
/// state per agent session (or subprocess).
pub fn with_chat_guidance(content: &str) -> String {
    prefix_with(&chat_guidance(), content)
}

/// Prefix `content` with an already-rendered guidance text (the
/// [`chat_guidance`] result the caller decided to inject).
pub fn prefix_with(guidance: &str, content: &str) -> String {
    format!("{guidance}{SEPARATOR}{content}")
}

/// First-turn flag transition, shared by every chat lane so the rule
/// is stated once and tested once: a dropped bridge (fresh subprocess
/// on revival) re-arms injection; a live bridge that just received
/// the guidance stays quiet afterwards.
pub fn next_guidance_sent(current: bool, injected: bool, bridge_live: bool) -> bool {
    if !bridge_live {
        return false;
    }
    current || injected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_strips_front_matter() {
        let body = section_body();
        assert!(
            body.starts_with("## Pi Dash CLI (`pidash`)"),
            "body starts at the heading: {body:.60}"
        );
        assert!(!body.contains("customizable:"), "no front-matter leaks");
    }

    #[test]
    fn body_tracks_the_section_source() {
        // The module embeds the composer's section file directly; this
        // pins the coupling so a move of either file breaks loudly.
        assert!(SECTION_SOURCE.starts_with("---\nkey: pidash-cli\n"));
    }

    /// The drift guard: the same source the composer ships in run
    /// prompts renders here with a minimal chat context.
    #[test]
    fn chat_render_keeps_guidance_and_drops_run_material() {
        let body = render_chat_guidance_for(section_body(), "2026-11-25T08:05:46Z")
            .expect("chat renders from the embedded section");
        for kept in [
            "## Pi Dash CLI (`pidash`)",
            "already authenticated **as this runner's owner**",
            "Every command prints one JSON document",
            "Run `pidash docs <topic>`",
            "### Other commands",
            "There is no current issue or run",
            "Nothing is pre-loaded",
            "state list unavailable",
            "`pidash issue list --project <PROJ>",
            "`pidash issue create --project <PROJ>",
            "`pidash state list",
        ] {
            assert!(body.contains(kept), "chat carries {kept:?}");
        }
        for dropped in [
            "PIDASH_ISSUE_IDENTIFIER",
            "PIDASH_RUN_ID",
            "plus this run's id",
            "workpad",
            "pidash run finish",
            "pidash run yield",
            "run_kind",
            "Blocking the run",
            "Lifecycle",
            "already shown below",
        ] {
            assert!(!body.contains(dropped), "chat drops {dropped:?}");
        }
        assert!(
            body.len() <= 6_000,
            "chat guidance stays lean ({} bytes)",
            body.len()
        );
    }

    /// The guidance is multi-line; the engine bridges send it inside a
    /// single JSON-RPC line. Pin that the serialized `turn/start`
    /// request carries no literal newline (a fake logging with `echo`
    /// would otherwise split it — a harness artifact, not a wire bug).
    #[test]
    fn wire_request_stays_single_line() {
        let text = with_chat_guidance("list the open issues");
        assert!(text.contains('\n'), "guidance is multi-line");
        let params = serde_json::to_value(crate::codex::schema::TurnStartParams {
            thread_id: "th".into(),
            input: vec![crate::codex::schema::TurnInputItem {
                item_type: "text".into(),
                text,
            }],
            model: None,
            effort: None,
        })
        .unwrap();
        let line = crate::codex::jsonrpc::request(2, "turn/start", &params).unwrap();
        assert!(!line.contains('\n'), "wire line must be single-line");
        assert!(
            line.contains("## Pi Dash CLI"),
            "guidance survives serialization"
        );
    }

    #[test]
    fn flag_transition_rearms_only_on_a_dropped_bridge() {
        // Fresh subprocess (revival): inject again.
        assert!(!next_guidance_sent(false, true, false));
        assert!(!next_guidance_sent(true, true, false));
        // Live bridge, just injected: quiet from now on.
        assert!(next_guidance_sent(false, true, true));
        // Live bridge, already guided: stays guided without injecting.
        assert!(next_guidance_sent(true, false, true));
        // Live bridge, never injected: stays unguided.
        assert!(!next_guidance_sent(false, false, true));
    }

    #[test]
    fn prefix_keeps_user_content_intact_after_the_guidance() {
        let prefixed = with_chat_guidance("list the open issues");
        let guidance = chat_guidance();
        assert!(prefixed.starts_with(guidance.as_str()));
        assert!(prefixed.ends_with("list the open issues"));
        assert!(prefixed.contains(SEPARATOR));
    }
}
