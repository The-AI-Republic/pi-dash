//! Provider-error rendering helpers with pydantic-core error shapes.
//!
//! What the chat engine keeps after P3 step A (agent naming
//! convention): the retry-error detail types ([`ValidationDetail`],
//! [`LocSeg`]), the JSON re-escaper ([`ensure_ascii`]), and the
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

/// Re-escape non-ASCII text in an already-serialized JSON document the way
/// CPython `json.dumps` does with `ensure_ascii=True`: every `c >= 0x7F`
/// inside a string becomes `\uXXXX` (lowercase hex; surrogate pairs for
/// astral planes). String boundaries are tracked with escape awareness;
/// structural characters are never touched.
pub(crate) fn ensure_ascii(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    let mut chars = pretty.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    for c in chars.by_ref() {
        if !in_string {
            out.push(c);
            if c == '"' {
                in_string = true;
            }
            continue;
        }
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                escaped = true;
            }
            '"' => {
                out.push(c);
                in_string = false;
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let shifted = code - 0x1_0000;
                    let high = 0xd800 + (shifted >> 10);
                    let low = 0xdc00 + (shifted & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out
}

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
    use crate::cloud_agent::provider::{retry_message, RetryContent};

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

    #[test]
    fn ensure_ascii_matches_cpython() {
        assert_eq!(ensure_ascii(r#""caf\u00e9""#), r#""caf\u00e9""#);
        assert_eq!(ensure_ascii("\"caf\u{e9}\""), r#""caf\u00e9""#);
        assert_eq!(ensure_ascii("\"\u{1f600}\""), r#""\ud83d\ude00""#);
        assert_eq!(ensure_ascii("\"a\x7fb\""), r#""a\u007fb""#);
        // Structural characters and existing escapes pass through.
        assert_eq!(ensure_ascii("{\n  \"a\": 1\n}"), "{\n  \"a\": 1\n}");
        assert_eq!(ensure_ascii(r#""a\nb""#), r#""a\nb""#);
    }
}
