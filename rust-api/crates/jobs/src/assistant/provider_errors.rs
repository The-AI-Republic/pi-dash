//! Turn a failed model call into the error the user is shown.
//!
//! Port of `apps/api/pi_dash/assistant/runtime/provider_errors.py` (`:1-171`
//! on `origin/main`, PDASHOSS01-287): the status-first classifier that
//! replaced the substring text table. The same HTTP status means different
//! things depending on how the user reaches the model, so
//! [`classify_provider_error`] takes the connection kind, and the
//! [`TurnSeam::turn_error_for_user`][crate::assistant::run_turn::TurnSeam::turn_error_for_user]
//! seam method is what picks it.
//!
//! Python walks the exception's `__cause__` chain for the status, the
//! structured body and the model name. Rust has no exception chain: the
//! caller passes those flat (`status`, `body_detail` pre-extracted by
//! [`provider_detail`], `model`), already resolved at the error boundary.
//! Text markers stay a fallback for failures that carry no status at all.

use serde_json::Value;

/// The user supplied the credential (an API key in Settings).
pub const BYOK: &str = "byok";
/// The credential is minted for the user's session by a metered gateway.
pub const GATEWAY: &str = "gateway";

/// Provider text is shown to the user, so it is kept to one short line.
const MAX_PROVIDER_DETAIL: usize = 300;

/// Provider error *codes* and phrases specific enough to trust without a status.
const CREDIT_MARKERS: &[&str] = &[
    "payment required",
    "insufficient_credits",
    "insufficient credits",
    "insufficient_quota",
    "no_credit_account",
    "quota exceeded",
];
/// A 429 is a rate limit unless it carries one of these: "quota exceeded" on
/// its own also describes a per-minute request quota.
const CREDIT_CODES_ON_429: &[&str] = &[
    "insufficient_quota",
    "insufficient_credits",
    "insufficient credits",
    "no_credit_account",
];
const AUTH_MARKERS: &[&str] = &[
    "unauthorized",
    "api key",
    "authentication",
    "invalid_api_key",
];
const RATE_LIMIT_MARKERS: &[&str] = &["rate limit", "rate_limit", "too many requests"];
const MODEL_MARKERS: &[&str] = &[
    "unsupported model",
    "model_not_found",
    "does not exist",
    "unknown model",
    "no such model",
];
const UNREACHABLE_MARKERS: &[&str] = &[
    "connection",
    "timeout",
    "timed out",
    "unreachable",
    "could not connect",
    "name resolution",
];

/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_OUT_OF_CREDIT: &str = "provider_out_of_credit";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_AUTH_FAILED: &str = "provider_auth_failed";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_RATE_LIMITED: &str = "provider_rate_limited";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_UNREACHABLE: &str = "provider_unreachable";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_MODEL_INVALID: &str = "model_invalid";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_SESSION_EXPIRED: &str = "provider_session_expired";
/// Turn-runtime failure codes (`errors.py:78-128`), verbatim.
pub const CODE_MODEL_UNAVAILABLE: &str = "model_unavailable";

/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_OUT_OF_CREDIT: &str = "The provider rejected the request for lack of credit.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_OUT_OF_CREDIT_GATEWAY: &str =
    "You are out of credit for the model provider. Add credit to keep going.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_AUTH_FAILED: &str = "Your API key was rejected by the provider.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_SESSION_EXPIRED: &str =
    "Your session with the model provider has expired. Sign out and sign back in to continue.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_RATE_LIMITED: &str =
    "Too many requests to the model provider. Wait a moment and try again.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_MODEL_INVALID_SUFFIX: &str =
    "was not accepted by the provider. Check the model name in Settings.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_MODEL_UNAVAILABLE_SUFFIX: &str =
    "is not available right now. Pick another model in Settings.";
/// User-facing details, verbatim (`provider_errors.py:139-169`).
pub const DETAIL_UNREACHABLE: &str = "Could not reach the configured provider endpoint.";

/// The provider's own one-line explanation, or `""` when it gave none
/// (`provider_errors.py:84-102`).
///
/// Read from the structured response body only: `message`, then
/// `error.message` when `error` is an object, then `error` itself when it
/// is a string. The exception's `str()` is never used: it carries the
/// status line, the model name and often a request id or URL, none of
/// which belong in front of the user. Runs of whitespace collapse to one
/// space; longer text caps at 300 chars with an ellipsis.
pub fn provider_detail(body: Option<&Value>) -> String {
    let Some(Value::Object(map)) = body else {
        return String::new();
    };
    let nested = map.get("error");
    let message = map.get("message").and_then(Value::as_str);
    let nested_message = match nested {
        Some(Value::Object(inner)) => inner.get("message").and_then(Value::as_str),
        Some(Value::String(text)) => Some(text.as_str()),
        _ => None,
    };
    for candidate in [message, nested_message].into_iter().flatten() {
        if candidate.trim().is_empty() {
            continue;
        }
        // `" ".join(candidate.split())`: collapse every whitespace run.
        let text = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
        // `text[:299].rstrip() + "…"`, counted in chars like the slice.
        if text.chars().count() > MAX_PROVIDER_DETAIL {
            let head: String = text.chars().take(MAX_PROVIDER_DETAIL - 1).collect();
            return format!("{}…", head.trim_end());
        }
        return text;
    }
    String::new()
}

fn with_detail(message: &str, detail: &str) -> String {
    if detail.is_empty() {
        message.to_owned()
    } else {
        format!("{message} The provider said: {detail}")
    }
}

/// Map a provider failure onto a typed `(code, detail)` pair, or `None`
/// when unrecognised (`provider_errors.py:117-171`).
///
/// `status` is the provider's HTTP status when the failure carries one;
/// `text` the failure message (matched lowercased); `body_detail` the
/// [`provider_detail`] one-liner (`""` when the body gave none);
/// `connection` is [`BYOK`] or [`GATEWAY`] and decides the wording;
/// `model` names the model for the message when the failure does not.
///
/// The status is read from the failure, never substring-matched: "401"
/// and "402" appear in request ids and token counts, so matching them in
/// the message text misclassifies unrelated errors. Text markers are a
/// fallback for failures that carry no status at all.
pub fn classify_provider_error(
    status: Option<u16>,
    text: &str,
    body_detail: &str,
    connection: &str,
    model: &str,
) -> Option<(String, String)> {
    let gateway = connection == GATEWAY;
    let lower = text.to_lowercase();
    let has = |markers: &[&str]| markers.iter().any(|m| lower.contains(m));

    // Credit exhaustion comes first: routed providers answer 402 for it, and
    // OpenAI reports an empty balance as a 429 with ``insufficient_quota``,
    // which must not read as "retry shortly".
    if status == Some(402)
        || has(if status == Some(429) {
            CREDIT_CODES_ON_429
        } else {
            CREDIT_MARKERS
        })
    {
        if gateway {
            return Some((
                CODE_OUT_OF_CREDIT.to_owned(),
                with_detail(DETAIL_OUT_OF_CREDIT_GATEWAY, body_detail),
            ));
        }
        return Some((
            CODE_OUT_OF_CREDIT.to_owned(),
            with_detail(DETAIL_OUT_OF_CREDIT, body_detail),
        ));
    }

    // Not every provider answers 401 for a bad key: Gemini's OpenAI-compatible
    // endpoint says 400 "API key not valid", others 403. The text decides for
    // those, but never for a 5xx.
    if status == Some(401) || (matches!(status, None | Some(400) | Some(403)) && has(AUTH_MARKERS))
    {
        if gateway {
            return Some((
                CODE_SESSION_EXPIRED.to_owned(),
                DETAIL_SESSION_EXPIRED.to_owned(),
            ));
        }
        return Some((CODE_AUTH_FAILED.to_owned(), DETAIL_AUTH_FAILED.to_owned()));
    }

    if status == Some(429) || (status.is_none() && has(RATE_LIMIT_MARKERS)) {
        return Some((CODE_RATE_LIMITED.to_owned(), DETAIL_RATE_LIMITED.to_owned()));
    }

    if (matches!(status, Some(400) | Some(404)) || status.is_none()) && has(MODEL_MARKERS) {
        let quoted = if model.is_empty() {
            "The selected model".to_owned()
        } else {
            format!("The model \"{model}\"")
        };
        if gateway {
            return Some((
                CODE_MODEL_UNAVAILABLE.to_owned(),
                format!("{quoted} {DETAIL_MODEL_UNAVAILABLE_SUFFIX}"),
            ));
        }
        return Some((
            CODE_MODEL_INVALID.to_owned(),
            format!("{quoted} {DETAIL_MODEL_INVALID_SUFFIX}"),
        ));
    }

    // A status means the endpoint answered, so only a timeout or a server-side
    // failure can still be a reachability problem.
    if (status.is_none() || status == Some(408) || status.is_some_and(|s| s >= 500))
        && has(UNREACHABLE_MARKERS)
    {
        return Some((CODE_UNREACHABLE.to_owned(), DETAIL_UNREACHABLE.to_owned()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_detail_reads_openai_style_nested_errors() {
        // Port of Django's `test_provider_detail_reads_openai_style_nested_errors`:
        // the nested message wins, with whitespace runs collapsed.
        let body = json!({"error": {"message": "  Insufficient\n credits  ", "code": 402}});
        assert_eq!(provider_detail(Some(&body)), "Insufficient credits");
        // A blank top-level message falls through to the nested one.
        let body = json!({"message": "  ", "error": {"message": "nested"}});
        assert_eq!(provider_detail(Some(&body)), "nested");
    }

    #[test]
    fn provider_detail_is_empty_without_a_structured_body() {
        // Port of Django's `test_provider_detail_is_empty_without_a_structured_body`.
        let body = json!("<html>Payment Required</html>");
        assert_eq!(provider_detail(Some(&body)), "");
        assert_eq!(provider_detail(None), "");
    }

    #[test]
    fn provider_detail_is_capped() {
        // Port of Django's `test_provider_detail_is_capped`: 299 chars,
        // rstripped, plus the ellipsis — 300 chars total, counted in
        // chars like the Python slice.
        let body = json!({"message": "x".repeat(5000)});
        let detail = provider_detail(Some(&body));
        assert_eq!(detail.chars().count(), 300);
        assert_eq!(detail, format!("{}…", "x".repeat(299)));
    }
}
