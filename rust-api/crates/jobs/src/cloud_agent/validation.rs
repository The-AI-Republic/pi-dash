//! Provider-error rendering helpers with pydantic-core error shapes.
//!
//! What the chat engine keeps after P3 step A (agent naming
//! convention): the retry-error detail types ([`ValidationDetail`],
//! [`LocSeg`]), and the
//! unknown-tool message ([`unknown_tool_content`]). The retired
//! tool-argument and output validators (built on
//! `CloudAgentOutput`) died with the execute plane; the wire
//! rendering of any remaining detail stays byte-identical.

use serde_json::Value;

/// A single pydantic-core-style error detail (`type`/`loc`/`msg`/`input`,
/// context excluded, as `error.errors(include_url=False,
/// include_context=False)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDetail {
    /// `missing`, `int_type`, `string_too_long`, ...
    pub error_type: &'static str,
    /// Location path (field names / list indices).
    pub loc: Vec<LocSeg>,
    /// The human message (`Field required`, ...).
    pub msg: String,
    /// The offending input value (original, pre-coercion/strip).
    pub input: Value,
}

/// A location segment: object field or list index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocSeg {
    Field(String),
    Index(usize),
}

// `ValidationDetail::new` was consumed only by the retired validators
// (deleted in P3 step A); construction is now by struct literal.

/// The unknown-tool `ModelRetry` content (`_resolve_tool`,
/// `tool_manager.py:356-369`): the available names are byte-sorted and
/// include the output tool. The wire rendering appends the fix suffix
/// (`RetryPromptPart.model_response`).
pub fn unknown_tool_content(name: &str, available_sorted: &[&str]) -> String {
    let quoted: Vec<String> = available_sorted
        .iter()
        .map(|tool| format!("'{tool}'"))
        .collect();
    format!(
        "Unknown tool name: '{name}'. Available tools: {}",
        quoted.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_agent::history::{retry_message, TurnRetryContent as RetryContent};

    fn provider_text_retry(text: &str, tool: Option<&str>) -> String {
        retry_message(&RetryContent::Text(text.to_owned()), tool)
    }

    #[test]
    fn unknown_tool_message_matches_probe() {
        assert_eq!(
            provider_text_retry(
                &unknown_tool_content("nope_tool", &["final_result", "probe_tool"]),
                Some("nope_tool")
            ),
            "Unknown tool name: 'nope_tool'. Available tools: 'final_result', 'probe_tool'\n\nFix the errors and try again."
        );
    }
}
