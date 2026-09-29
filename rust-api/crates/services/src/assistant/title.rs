//! Single-prompt title generation for assistant-created work items (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/runtime/title.py:1-147`: the prompt
//! constants, the provider branch (`generate_byok_title_for_user`), the
//! OpenAI-compatible path with its DeepSeek extra options
//! (`_openai_compatible_extra_options`, `_uses_deepseek_v4`), the Anthropic
//! path, and the pure response pipeline (`_content_to_text`,
//! `_clean_title`, `_strip_reasoning_blocks`). Fixture id F-A6-08
//! (`rust-api/fixtures/assistant/runtime.json`).
//!
//! Shape notes:
//!
//! * HTTP transport (the `openai` / `anthropic` SDK clients,
//!   `title.py:48-62,80-91`) stays with the handler layer. This module ports
//!   everything around it byte for byte: the exact request bodies
//!   ([`OpenAiTitleRequest::body_json`], [`AnthropicTitleRequest::body_json`])
//!   and the exact parsing of the SDK responses ([`content_to_text`]).
//! * The entry gates (no config/key, no model name, SSRF-blocked base URL,
//!   `title.py:32-38`) are the same three `llm_config_missing` errors as
//!   [`super::llm::resolve_byok_model`]; the branch choice itself is
//!   [`select_provider`].
//! * Hostname parsing reuses [`super::ssrf::extract_hostname`] (lowercased
//!   `urlparse(url).hostname` semantics); the DeepSeek rule only adds the
//!   model-name substring check on top.

use serde_json::{json, Value};

/// Work-item title length cap (`title.py:19`).
pub const TITLE_MAX_LEN: usize = 255;
/// Title completion token budget (`title.py:20`).
pub const TITLE_MAX_OUTPUT_TOKENS: u32 = 256;
/// System prompt for title generation (`title.py:21-27`).
pub const TITLE_SYSTEM_PROMPT: &str = "You write concise, specific titles for project work items. Given a work item's description, reply with a single short title (at most 80 characters) that captures what it is about. Return only the title text: no surrounding quotes, no trailing punctuation, no reasoning or analysis, and no preamble such as 'Title:'.";
/// Request timeout, seconds (`title.py:51,83`).
pub const TITLE_TIMEOUT_SECS: f64 = 20.0;
/// Sampling temperature (`title.py:59,89`).
pub const TITLE_TEMPERATURE: f64 = 0.2;

/// Title provider branch (`generate_byok_title_for_user`, `title.py:41-44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleProvider {
    Anthropic,
    OpenAICompatible,
}

/// Branch choice: the Anthropic kind takes the Anthropic path, everything
/// else takes the OpenAI-compatible path (`title.py:41-44`).
pub fn select_provider(provider_kind: &str) -> TitleProvider {
    if provider_kind == super::llm::PROVIDER_ANTHROPIC {
        TitleProvider::Anthropic
    } else {
        TitleProvider::OpenAICompatible
    }
}

/// DeepSeek v4 detection (`_uses_deepseek_v4`, `title.py:71-77`): the lowered
/// model name contains `deepseek-v4`, or the base URL's host is
/// `api.deepseek.com` (or a subdomain of it).
pub fn uses_deepseek_v4(model_name: &str, base_url: &str) -> bool {
    if model_name.to_lowercase().contains("deepseek-v4") {
        return true;
    }
    match super::ssrf::extract_hostname(base_url) {
        Some(host) => host == "api.deepseek.com" || host.ends_with(".deepseek.com"),
        None => false,
    }
}

/// Extra chat-completions options (`_openai_compatible_extra_options`,
/// `title.py:65-68`): DeepSeek v4 gets reasoning disabled, everything else
/// gets no extra options.
pub fn openai_compatible_extra_body(model_name: &str, base_url: &str) -> Option<Value> {
    if uses_deepseek_v4(model_name, base_url) {
        Some(json!({"thinking": {"type": "disabled"}}))
    } else {
        None
    }
}

/// OpenAI-compatible title request (`_generate_title_openai_compatible`,
/// `title.py:48-62`): exact chat-completions body.
pub struct OpenAiTitleRequest<'a> {
    pub model: &'a str,
    pub base_url: &'a str,
    pub description: &'a str,
}

impl<'a> OpenAiTitleRequest<'a> {
    /// Exact request body: system + user messages, token budget,
    /// temperature, and the DeepSeek `extra_body` only on that branch.
    pub fn body_json(&self) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": TITLE_SYSTEM_PROMPT},
                {"role": "user", "content": self.description},
            ],
            "max_tokens": TITLE_MAX_OUTPUT_TOKENS,
            "temperature": TITLE_TEMPERATURE,
        });
        if let Some(extra) = openai_compatible_extra_body(self.model, self.base_url) {
            body["extra_body"] = extra;
        }
        body
    }
}

/// Anthropic title request (`_generate_title_anthropic`, `title.py:80-91`):
/// exact messages body.
pub struct AnthropicTitleRequest<'a> {
    pub model: &'a str,
    pub description: &'a str,
}

impl<'a> AnthropicTitleRequest<'a> {
    /// Exact request body: top-level system prompt, one user message, token
    /// budget, temperature.
    pub fn body_json(&self) -> Value {
        json!({
            "model": self.model,
            "system": TITLE_SYSTEM_PROMPT,
            "messages": [{"role": "user", "content": self.description}],
            "max_tokens": TITLE_MAX_OUTPUT_TOKENS,
            "temperature": TITLE_TEMPERATURE,
        })
    }
}

/// Python truthiness over a JSON value (`_block_value(...) or ...`,
/// `title.py:108`): `None`/`false`/`0`/`""`/empty containers are falsy.
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            n.as_i64().is_some_and(|i| i != 0)
                || n.as_u64().is_some_and(|u| u != 0)
                || n.as_f64().is_some_and(|f| f != 0.0)
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `str()` over a scalar content value (`title.py:100`).
///
/// Numbers render as-is in both languages; booleans differ (`True` vs
/// `true`), so they are spelled the Python way.
fn scalar_to_text(value: &Value) -> String {
    match value {
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(s) => s.clone(),
        Value::Null => "None".to_owned(),
        _ => value.to_string(),
    }
}

/// Response content to plain text (`_content_to_text` + `_block_value`,
/// `title.py:94-121`).
///
/// `None` → `""`, strings pass through, lists concatenate their text parts
/// while skipping `reasoning`/`thinking` blocks (case-insensitive), and
/// anything else renders via `str()`. Dict attribute access (`block.get`)
/// and object attribute access (`getattr`) both read as object lookups here.
pub fn content_to_text(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            let mut parts = String::new();
            for block in blocks {
                let Value::Object(map) = block else {
                    continue;
                };
                let block_type = map.get("type").and_then(Value::as_str).unwrap_or("");
                if block_type.eq_ignore_ascii_case("reasoning")
                    || block_type.eq_ignore_ascii_case("thinking")
                {
                    continue;
                }
                let text = map.get("text").unwrap_or(&Value::Null);
                let chosen = if py_truthy(text) {
                    text
                } else {
                    map.get("content").unwrap_or(&Value::Null)
                };
                match chosen {
                    Value::String(s) => parts.push_str(s),
                    Value::Array(_) => {
                        let nested = content_to_text(chosen);
                        if !nested.is_empty() {
                            parts.push_str(&nested);
                        }
                    }
                    _ => {}
                }
            }
            parts
        }
        scalar => scalar_to_text(scalar),
    }
}

/// Python `str.strip()` edge (same as the markdown port): `strip()` also
/// trims `\x1c`-`\x1f`, which Rust's Unicode `trim` leaves.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_whitespace() || ('\x1c'..='\x1f').contains(&ch))
}

/// First visual line (`title.splitlines()[0]`, `title.py:133`).
///
/// Splits at the first Python `splitlines` boundary (`\r\n`, `\r`, `\n`,
/// vertical tab, form feed, `\x1c`-`\x1e`, `\x85`, ` `, ` `).
fn first_line(text: &str) -> &str {
    let mut end = text.len();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        let ch = rest.chars().next().expect("char boundary");
        let is_boundary = matches!(
            ch,
            '\r' | '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | ' ' | ' '
        );
        if is_boundary {
            end = i;
            break;
        }
        i += ch.len_utf8();
    }
    &text[..end]
}

/// Normalize the model's reply into a single-line title within the length
/// cap (`_clean_title`, `title.py:124-139`).
pub fn clean_title(raw: &str) -> String {
    let title = py_strip(raw);
    if title.is_empty() {
        return String::new();
    }
    let stripped = strip_reasoning_blocks(title);
    if stripped.trim().is_empty() && py_strip(&stripped).is_empty() {
        return String::new();
    }
    // The model occasionally wraps the title in quotes or spreads it over lines.
    let mut title = py_strip(first_line(&stripped));
    title = title.trim_matches(|c| c == '"' || c == '\'');
    title = py_strip(title);
    if title.to_lowercase().starts_with("title:") {
        title = py_strip(title.split_once(':').map(|(_, after)| after).unwrap_or(""));
    }
    title = title.trim_end_matches(['.', ',', ':', ';', '!', '?']);
    title = py_strip(title);
    // `title[:255]` counts code points and can split a UTF-8 sequence, so
    // take whole chars; the trailing `rstrip()` applies after truncation.
    let mut out: String = title.chars().take(TITLE_MAX_LEN).collect();
    let trimmed = py_strip(&out).to_owned();
    out = trimmed;
    out
}

/// Match one `<think|thinking|reasoning>…</same>` block case-insensitively
/// (`_REASONING_BLOCK_RE`, `title.py:28`): the tag name carries no
/// attributes, the body is non-greedy (first matching close tag wins), and a
/// missing close tag matches nothing.
fn match_reasoning_block(text: &str, at: usize) -> Option<usize> {
    debug_assert!(text.is_char_boundary(at));
    let rest = &text[at..];
    if !rest.starts_with('<') {
        return None;
    }
    let after_open = &rest[1..];
    let name_len = after_open
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .map(|c| c.len_utf8())
        .sum::<usize>();
    let name = after_open[..name_len].to_ascii_lowercase();
    if !matches!(name.as_str(), "think" | "thinking" | "reasoning") {
        return None;
    }
    if after_open[name_len..].starts_with('>') {
        let body_start = at + 1 + name_len + 1;
        let close = format!("</{name}>");
        let body = &text[body_start..];
        let lower = body.to_ascii_lowercase();
        lower.find(&close).map(|rel| body_start + rel + close.len())
    } else {
        None
    }
}

/// Strip reasoning blocks to a fixpoint (`_strip_reasoning_blocks`,
/// `title.py:142-147`): `re.sub` to `""` plus `strip()` per pass, repeating
/// while the text still changes (handles nesting).
pub fn strip_reasoning_blocks(text: &str) -> String {
    let mut previous = text.to_owned();
    loop {
        let mut out = String::with_capacity(previous.len());
        let mut i = 0;
        while i < previous.len() {
            if previous.as_bytes()[i] == b'<' {
                if let Some(end) = match_reasoning_block(&previous, i) {
                    i = end;
                    continue;
                }
            }
            let ch = previous[i..].chars().next().expect("char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        let next = py_strip(&out).to_owned();
        if next == previous {
            return next;
        }
        previous = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_consts_match_python() {
        assert_eq!(TITLE_MAX_LEN, 255);
        assert_eq!(TITLE_MAX_OUTPUT_TOKENS, 256);
        assert!(TITLE_SYSTEM_PROMPT.starts_with("You write concise, specific titles"));
        assert!(TITLE_SYSTEM_PROMPT.ends_with("such as 'Title:'."));
        assert_eq!(TITLE_TIMEOUT_SECS, 20.0);
        assert_eq!(TITLE_TEMPERATURE, 0.2);
    }

    #[test]
    fn provider_branch_selection() {
        assert_eq!(select_provider("anthropic"), TitleProvider::Anthropic);
        assert_eq!(
            select_provider("openai_compatible"),
            TitleProvider::OpenAICompatible
        );
        assert_eq!(select_provider("other"), TitleProvider::OpenAICompatible);
    }

    #[test]
    fn deepseek_detected_by_model_name_case_insensitively() {
        assert!(uses_deepseek_v4(
            "DeepSeek-V4-Chat",
            "https://other.example/v1"
        ));
        assert!(uses_deepseek_v4("org/deepseek-v4-turbo", ""));
        assert!(!uses_deepseek_v4("gpt-4o", "https://api.openai.com/v1"));
        assert!(!uses_deepseek_v4(
            "deepseek-chat",
            "https://api.openai.com/v1"
        ));
    }

    #[test]
    fn deepseek_detected_by_hostname() {
        assert!(uses_deepseek_v4("m", "https://api.deepseek.com/v1"));
        assert!(uses_deepseek_v4("m", "https://EU.DeepSeek.COM/chat"));
        assert!(uses_deepseek_v4("m", "https://proxy.deepseek.com/v1"));
        assert!(!uses_deepseek_v4(
            "m",
            "https://deepseek.com.evil.example/v1"
        ));
        assert!(!uses_deepseek_v4("m", ""));
        assert!(!uses_deepseek_v4("m", "not-a-url"));
    }

    #[test]
    fn extra_options_only_on_deepseek_branch() {
        assert_eq!(
            openai_compatible_extra_body("deepseek-v4", "https://x.example"),
            Some(json!({"thinking": {"type": "disabled"}}))
        );
        assert_eq!(
            openai_compatible_extra_body("gpt-4o", "https://api.deepseek.com"),
            Some(json!({"thinking": {"type": "disabled"}}))
        );
        assert_eq!(
            openai_compatible_extra_body("gpt-4o", "https://x.example"),
            None
        );
    }

    #[test]
    fn openai_request_body_shape() {
        let body = OpenAiTitleRequest {
            model: "gpt-4o",
            base_url: "https://x.example/v1",
            description: "Fix the thing",
        }
        .body_json();
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], TITLE_SYSTEM_PROMPT);
        assert_eq!(
            body["messages"][1],
            json!({"role": "user", "content": "Fix the thing"})
        );
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["temperature"], 0.2);
        assert!(body.get("extra_body").is_none());
    }

    #[test]
    fn openai_request_body_carries_deepseek_extra_body() {
        let body = OpenAiTitleRequest {
            model: "deepseek-v4",
            base_url: "https://x.example/v1",
            description: "d",
        }
        .body_json();
        assert_eq!(
            body["extra_body"],
            json!({"thinking": {"type": "disabled"}})
        );
    }

    #[test]
    fn anthropic_request_body_shape() {
        let body = AnthropicTitleRequest {
            model: "claude-x",
            description: "Fix the thing",
        }
        .body_json();
        assert_eq!(body["model"], "claude-x");
        assert_eq!(body["system"], TITLE_SYSTEM_PROMPT);
        assert_eq!(
            body["messages"],
            json!([{"role": "user", "content": "Fix the thing"}])
        );
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["temperature"], 0.2);
    }

    #[test]
    fn content_to_text_vectors() {
        assert_eq!(content_to_text(&Value::Null), "");
        assert_eq!(content_to_text(&json!("plain")), "plain");
        // Dict-style blocks with a reasoning block skipped (title.py:105-106).
        let mixed = json!([
            {"type": "reasoning", "text": "hidden"},
            {"type": "text", "text": "hi"},
        ]);
        assert_eq!(content_to_text(&mixed), "hi");
        // Nested content lists recurse (title.py:111-114).
        let nested = json!([{"type": "text", "content": [{"type": "text", "text": "nested"}]}]);
        assert_eq!(content_to_text(&nested), "nested");
        // Falsy text falls back to content (the `or` at title.py:108).
        let fallback = json!([{"type": "text", "text": "", "content": "c"}]);
        assert_eq!(content_to_text(&fallback), "c");
        // Non-list, non-string renders via str() (title.py:100).
        assert_eq!(content_to_text(&json!(7)), "7");
    }

    #[test]
    fn strip_reasoning_blocks_vector() {
        assert_eq!(
            strip_reasoning_blocks("mid <think>hidden</think> end"),
            "mid  end"
        );
        assert_eq!(strip_reasoning_blocks("<THINKING>x</Thinking>kept"), "kept");
        assert_eq!(
            strip_reasoning_blocks("a<reasoning>r1<think>r2</think></reasoning>b"),
            "ab"
        );
        // Unclosed opener matches nothing.
        assert_eq!(strip_reasoning_blocks("a<think>oops"), "a<think>oops");
        // Mismatched close tag matches nothing.
        assert_eq!(
            strip_reasoning_blocks("a<think>x</thinking>b"),
            "a<think>x</thinking>b"
        );
    }

    #[test]
    fn clean_title_vectors() {
        assert_eq!(clean_title("  Fix the thing.  "), "Fix the thing");
        assert_eq!(clean_title("\"Real title\""), "Real title");
        assert_eq!(clean_title("line one\nline two"), "line one");
        assert_eq!(clean_title(""), "");
        assert_eq!(clean_title("   "), "");
        assert_eq!(clean_title("Title: Real title"), "Real title");
        assert_eq!(clean_title("title: Real title..."), "Real title");
        assert_eq!(
            clean_title("Trailing punctuation?!"),
            "Trailing punctuation"
        );
        assert_eq!(clean_title("<think>h</think>Real title"), "Real title");
        // 255-char truncation with post-truncation rstrip (title.py:137-138).
        let long = "x".repeat(300);
        assert_eq!(clean_title(&long), "x".repeat(255));
        let padded = format!("{}   tail", "y".repeat(253));
        assert_eq!(clean_title(&padded).len(), 253);
    }
}
