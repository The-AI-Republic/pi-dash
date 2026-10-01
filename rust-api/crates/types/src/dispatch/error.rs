//! Cloud Agent error sanitization and classification (D-11, stage 5).
//!
//! Port of `apps/api/pi_dash/cloud_agent/errors.py:7-44`:
//!
//! * `_SECRET_PATTERNS` (`:7-13`) → [`sanitize_error`] redaction.
//! * `sanitize_error` (`:16-20`) → [`sanitize_error`].
//! * `_is_usage_limit` (`:23-31`) → [`is_usage_limit`].
//! * `classify_error` (`:34-44`) → [`classify_error`] + [`ErrorCode`].
//!
//! Translation notes:
//!
//! * Python takes `BaseException`; Rust has no exception objects, so the
//!   functions are generic over `E: std::error::Error`. `str(exc)` is
//!   `Display`, and the `type(exc).__name__` fallback (`errors.py:17`) is the
//!   trailing segment of `std::any::type_name` (Rust paths carry modules,
//!   Python class names do not). The concrete type (not a trait object) must
//!   be passed for both the fallback and the usage-limit type branch.
//! * `_is_usage_limit` checks `isinstance(exc, UsageLimitExceeded)` from
//!   `pydantic_ai`, which has no Rust counterpart. The port defines the
//!   marker error [`UsageLimitExceeded`] at this seam so the type branch
//!   keeps its structure; the check is top-type only, exactly like
//!   `isinstance` (it does not walk `source()` chains).
//! * `text[:16_000]` (`errors.py:20`) counts Unicode code points; the port
//!   takes the first [`MAX_ERROR_TEXT_CHARS`] `char`s, which can neither
//!   split UTF-8 nor panic on a boundary (Semantic traps: byte vs code-point
//!   slicing).
//! * The three pre-compiled patterns use only `(?i)`, `\b`, `\s`, `(?:…)`,
//!   character classes and counted repetition — all supported by the `regex`
//!   crate (no lookaround or backreferences). `regex::Replacer` closures
//!   reproduce the `match.group(1) if match.lastindex else ""` prefix rule.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json`
//! (`sanitize_error`, `classify_error`, `is_usage_limit`).
//!
//! Ported bugs: none found in this unit on read-through.

use std::sync::OnceLock;

/// Diagnostic-text cap (`errors.py:20`), counted in Unicode code points.
pub const MAX_ERROR_TEXT_CHARS: usize = 16_000;

/// Secret-redaction replacement (`errors.py:19`).
const REDACTED: &str = "[REDACTED]";

fn secret_patterns() -> &'static [regex::Regex; 3] {
    static PATTERNS: OnceLock<[regex::Regex; 3]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            regex::Regex::new(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)[^\s,;]+")
                .expect("auth pattern compiles"),
            regex::Regex::new(r"\b(?:sk|gh[opsu])_[A-Za-z0-9_-]{12,}\b")
                .expect("token pattern compiles"),
            regex::Regex::new(r"\bsk-[A-Za-z0-9_-]{12,}\b").expect("byok pattern compiles"),
        ]
    })
}

/// Stable error-code taxonomy (`classify_error`, `errors.py:34-44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Model usage/iteration budget exhausted → `iteration_limit`.
    IterationLimit,
    /// Provider safety/content refusal → `provider_refusal` (REFUSED).
    ProviderRefusal,
    /// Anything else → `provider_error` (FAILED).
    ProviderError,
}

impl ErrorCode {
    /// The verbatim wire/DB code.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::IterationLimit => "iteration_limit",
            ErrorCode::ProviderRefusal => "provider_refusal",
            ErrorCode::ProviderError => "provider_error",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Usage-limit marker: the Rust analog of `pydantic_ai`'s
/// `UsageLimitExceeded` (`errors.py:28`), which the Rust backend cannot
/// import. The execution layer raises this (carrying the provider message)
/// when the model call exhausts its budget; [`is_usage_limit`] detects it by
/// type, exactly like the `isinstance` branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLimitExceeded(String);

impl UsageLimitExceeded {
    /// Carry the provider's usage-limit message.
    pub fn new(message: impl Into<String>) -> Self {
        UsageLimitExceeded(message.into())
    }

    /// The carried message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UsageLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UsageLimitExceeded {}

/// Bare type name: the `type(exc).__name__` analog (`errors.py:17`).
fn short_type_name<E>() -> &'static str {
    std::any::type_name::<E>()
        .rsplit("::")
        .next()
        .unwrap_or("Error")
}

/// First `max` Unicode code points of `text` (`errors.py:20`).
///
/// `str::len` counts bytes and is always `>=` the code-point count, so a
/// string that fits in `max` bytes needs no walk; otherwise `chars().take()`
/// can never split UTF-8.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// Secret-safe diagnostic text (`errors.py:16-20`).
///
/// `str(exc) or type(exc).__name__`, then the three `_SECRET_PATTERNS`
/// redactions in order, then truncation to [`MAX_ERROR_TEXT_CHARS`] code
/// points.
pub fn sanitize_error<E: std::error::Error>(exc: &E) -> String {
    let display = exc.to_string();
    let text = if display.is_empty() {
        short_type_name::<E>().to_string()
    } else {
        display
    };
    let patterns = secret_patterns();
    // Pattern 0 keeps its capture group 1 (the `match.lastindex` branch);
    // patterns 1-2 have no groups, so the whole match is replaced.
    let text = patterns[0].replace_all(&text, |caps: &regex::Captures| {
        format!("{}[REDACTED]", &caps[1])
    });
    let text = patterns[1].replace_all(&text, REDACTED);
    let text = patterns[2].replace_all(&text, REDACTED);
    truncate_chars(&text, MAX_ERROR_TEXT_CHARS)
}

/// Usage-limit type branch (`errors.py:23-31`).
///
/// Top-type only, like `isinstance` — chain causes are not consulted.
pub fn is_usage_limit<E: std::error::Error + 'static>(exc: &E) -> bool {
    let erased: &(dyn std::error::Error + 'static) = exc;
    erased.is::<UsageLimitExceeded>()
}

/// Classify a failure (`errors.py:34-44`).
///
/// Returns the [`ErrorCode`] and the sanitized text. The branch order is
/// verbatim: usage-limit (type or the two substrings), then the four refusal
/// markers — deliberately no bare `"refused"`, so transport errors like
/// "Connection refused" stay `provider_error` (`errors.py:39-41`).
pub fn classify_error<E: std::error::Error + 'static>(exc: &E) -> (ErrorCode, String) {
    let text = sanitize_error(exc);
    let lowered = text.to_lowercase();
    if is_usage_limit(exc)
        || lowered.contains("usage limit")
        || lowered.contains("would exceed the")
    {
        return (ErrorCode::IterationLimit, text);
    }
    if [
        "content filter",
        "content_filter",
        "safety refusal",
        "model refused",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
    {
        return (ErrorCode::ProviderRefusal, text);
    }
    (ErrorCode::ProviderError, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-01-types.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    /// Plain error with a caller-chosen message (the `Exception(msg)` analog).
    #[derive(Debug)]
    struct Plain(String);

    impl std::fmt::Display for Plain {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for Plain {}

    /// Error with an empty `Display` (the `str(exc) == ""` branch).
    #[derive(Debug)]
    struct EmptyDisplay;

    impl std::fmt::Display for EmptyDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "")
        }
    }

    impl std::error::Error for EmptyDisplay {}

    #[test]
    fn sanitize_plain_message_passes_through() {
        // Fixture vector 1: ValueError('boom') -> 'boom'.
        assert_eq!(sanitize_error(&Plain("boom".into())), "boom");
    }

    #[test]
    fn sanitize_redaction_vectors_match_fixture_outputs() {
        // The fixture records scenario labels + sanitized outputs (not the
        // secret-bearing inputs), so the inputs below are reconstructed
        // equivalents: any matching secret yields the same output.
        let owned = fixture();
        let vectors = owned["sanitize_error"].as_array().expect("vectors");
        let by_label = |label: &str| {
            vectors
                .iter()
                .find(|v| v["input"] == label)
                .unwrap_or_else(|| panic!("vector {label}"))
        };
        // "auth header leak": group 1 keeps its original case; the token is
        // the `[^\s,;]+` tail.
        assert_eq!(
            sanitize_error(&Plain(
                "authorization: Bearer token-value-9f8e and done".into()
            )),
            by_label("auth header leak")["output"]
                .as_str()
                .expect("output")
        );
        // "sk- key leak": the BYOK pattern needs 12+ trailing chars.
        assert_eq!(
            sanitize_error(&Plain("key sk-abc123def456ghi789 leaked here".into())),
            by_label("sk- key leak")["output"].as_str().expect("output")
        );
        // "ghp_ leak": the (sk|gh[opsu])_ pattern.
        assert_eq!(
            sanitize_error(&Plain("token ghp_abc123def456ghi789 here".into())),
            by_label("ghp_ leak")["output"].as_str().expect("output")
        );
    }

    #[test]
    fn sanitize_unit_test_vector_matches_python_test() {
        // pi_dash/tests/unit/cloud_agent/test_cloud_agent.py
        // test_error_sanitizer_redacts_byok_and_bearer_secrets.
        let text = sanitize_error(&Plain(
            "Authorization: Bearer token-value key sk-abc123def456ghi789".into(),
        ));
        assert!(
            !text.contains("token-value"),
            "bearer secret redacted: {text}"
        );
        assert!(
            !text.contains("sk-abc123def456ghi789"),
            "byok secret redacted: {text}"
        );
        assert!(text.contains(REDACTED), "marker present: {text}");
        assert!(
            text.starts_with("Authorization: Bearer "),
            "group 1 kept verbatim: {text}"
        );
    }

    #[test]
    fn sanitize_empty_display_falls_back_to_type_name() {
        // Fixture vector 5 is the `str(exc) or type(exc).__name__` branch;
        // the literal differs by language (Python: "RuntimeError"), the
        // semantic — bare type name — is identical.
        assert_eq!(sanitize_error(&EmptyDisplay), "EmptyDisplay");
    }

    #[test]
    fn sanitize_truncates_to_16000_code_points_without_splitting_utf8() {
        let owned = fixture();
        let vectors = owned["sanitize_error"].as_array().expect("vectors");
        let truncation = vectors
            .iter()
            .find(|v| v["input"] == "20000-char message")
            .expect("truncation vector");
        assert_eq!(truncation["output_len"], 16000);
        assert_eq!(truncation["truncated_to_16000"], true);

        let long = "x".repeat(20_000);
        let out = sanitize_error(&Plain(long));
        assert_eq!(out.chars().count(), MAX_ERROR_TEXT_CHARS);
        assert_eq!(out.len(), MAX_ERROR_TEXT_CHARS);

        // Multibyte input: truncation counts code points, never bytes, and
        // never panics on a UTF-8 boundary (Semantic traps).
        let wide = "é".repeat(20_000);
        let out = sanitize_error(&Plain(wide));
        assert_eq!(out.chars().count(), MAX_ERROR_TEXT_CHARS);
        assert!(out.is_char_boundary(out.len()));

        // Short input is untouched (the byte-length fast path).
        assert_eq!(sanitize_error(&Plain("short".into())), "short");
    }

    #[test]
    fn classify_vectors_match_fixture_verbatim() {
        let owned = fixture();
        let vectors = owned["classify_error"].as_array().expect("vectors");
        assert_eq!(vectors.len(), 8, "all classify vectors recorded");
        for vector in vectors {
            let message = vector["message"].as_str().expect("message");
            let (code, text) = classify_error(&Plain(message.to_string()));
            assert_eq!(
                code.as_str(),
                vector["code"].as_str().expect("code"),
                "vector {}",
                vector["input"]
            );
            // text_echo: the returned text is the (sanitized) message itself.
            assert_eq!(vector["text_echo"], true);
            assert_eq!(text, message, "echo vector {}", vector["input"]);
        }
    }

    #[test]
    fn connection_refused_stays_provider_error() {
        // The deliberate trap of errors.py:39-41, also a fixture vector.
        let (code, _) = classify_error(&Plain("Connection refused by host".into()));
        assert_eq!(code, ErrorCode::ProviderError);
    }

    #[test]
    fn usage_limit_type_branch_beats_message_content() {
        // Fixture: pydantic_ai absent -> plain errors never match the type
        // branch.
        assert_eq!(fixture()["is_usage_limit"]["result_for_plain"], false);
        assert!(!is_usage_limit(&Plain("usage limit hit".into())));

        // The marker matches by type even with an unrelated message, and
        // classify_error honors the type branch first.
        let marker = UsageLimitExceeded::new("totally unrelated provider text");
        assert!(is_usage_limit(&marker));
        let (code, text) = classify_error(&marker);
        assert_eq!(code, ErrorCode::IterationLimit);
        assert_eq!(text, "totally unrelated provider text");

        // The real pydantic-ai message shape also matches via substring, as
        // in Python (both branches agree).
        let (code, _) = classify_error(&Plain(
            "The next request would exceed the request_limit of 25".into(),
        ));
        assert_eq!(code, ErrorCode::IterationLimit);
    }

    #[test]
    fn python_generated_edge_vectors_match() {
        // Observed by executing the real `errors.py` on edge inputs (case and
        // separator variants, token-length boundaries, word-boundary edges,
        // multibyte truncation); each pair is observed Python behavior, not hand-derived.
        let sanitize_cases: &[(String, String)] = &[
            (
                "AUTHORIZATION=bearer X-token-1 and more".to_string(),
                "AUTHORIZATION=bearer [REDACTED] and more".to_string(),
            ),
            (
                "authorization:  tok1, next".to_string(),
                "authorization:  [REDACTED], next".to_string(),
            ),
            (
                "authorization: Bearer".to_string(),
                "authorization: [REDACTED]".to_string(),
            ),
            (
                "authorization: Bearer ".to_string(),
                "authorization: [REDACTED] ".to_string(),
            ),
            ("sk-short".to_string(), "sk-short".to_string()),
            (
                "prefix sk-abcdefghijkl suffix".to_string(),
                "prefix [REDACTED] suffix".to_string(),
            ),
            (
                "prefix sk-abcdefghijk suffix".to_string(),
                "prefix sk-abcdefghijk suffix".to_string(),
            ),
            (
                "xsk-abcdefghijklmnop".to_string(),
                "xsk-abcdefghijklmnop".to_string(),
            ),
            (
                "tok ghp_abc123def456ghi789 end".to_string(),
                "tok [REDACTED] end".to_string(),
            ),
            (
                "tok gho_abc123def456ghi789 end".to_string(),
                "tok [REDACTED] end".to_string(),
            ),
            (
                "tok ghs_abc123def456ghi789 end".to_string(),
                "tok [REDACTED] end".to_string(),
            ),
            (
                "tok ghu_abc123def456ghi789 end".to_string(),
                "tok [REDACTED] end".to_string(),
            ),
            (
                "tok ghx_abc123def456ghi789 end".to_string(),
                "tok ghx_abc123def456ghi789 end".to_string(),
            ),
            (
                "authorization: Bearer café-token-9 end".to_string(),
                "authorization: Bearer [REDACTED] end".to_string(),
            ),
            ("   ".to_string(), "   ".to_string()),
            (
                "mix sk-abc123def456ghi789 and ghp_abc123def456ghi789 done".to_string(),
                "mix [REDACTED] and [REDACTED] done".to_string(),
            ),
            (
                "semi: sk-abc123def456ghi789;tail".to_string(),
                "semi: [REDACTED];tail".to_string(),
            ),
            (
                "CAP SK-ABC123DEF456GHI789 end".to_string(),
                "CAP SK-ABC123DEF456GHI789 end".to_string(),
            ),
            ("x".repeat(16000), "x".repeat(16000)),
            ("y".repeat(16001), "y".repeat(16000)),
            ("é".repeat(16001), "é".repeat(16000)),
        ];
        for (input, expected) in sanitize_cases {
            assert_eq!(
                &sanitize_error(&Plain(input.clone())),
                expected,
                "input {input:?}"
            );
        }
        let classify_cases: &[(&str, &str)] = &[
            ("USAGE LIMIT REACHED", "iteration_limit"),
            ("quota ok; would exceed the budget soon", "iteration_limit"),
            ("Model Refused this one", "provider_refusal"),
            ("refused", "provider_error"),
            ("content-filter triggered", "provider_error"),
            ("CONTENT_FILTER triggered", "provider_refusal"),
            ("safety refusal!", "provider_refusal"),
            ("nothing wrong here", "provider_error"),
            ("usage limit", "iteration_limit"),
            (
                "The next request would exceed the request_limit of 25",
                "iteration_limit",
            ),
            ("Connection refused by host", "provider_error"),
        ];
        for (message, expected) in classify_cases {
            let (code, text) = classify_error(&Plain(message.to_string()));
            assert_eq!(code.as_str(), *expected, "message {message:?}");
            assert_eq!(text, *message, "echo {message:?}");
        }
    }

    #[test]
    fn error_codes_render_verbatim() {
        assert_eq!(ErrorCode::IterationLimit.as_str(), "iteration_limit");
        assert_eq!(ErrorCode::ProviderRefusal.as_str(), "provider_refusal");
        assert_eq!(ErrorCode::ProviderError.as_str(), "provider_error");
        assert_eq!(ErrorCode::ProviderRefusal.to_string(), "provider_refusal");
    }
}
