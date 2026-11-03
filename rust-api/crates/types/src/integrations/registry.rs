//! Git provider registry (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/registry.py:18-53`: `get_adapter`
//! (case-insensitive lookup, `KeyError("Unsupported Git provider:
//! {provider}")` on unknown), `all_adapters` order (github, gitlab), the
//! first-match order of `parse_repository_url` / `parse_code_review_url`,
//! and the `provider_payload` key/display_name/code_review_term triplets.
//!
//! The adapter implementations themselves land in PIDASHCONV-141 (GitHub)
//! and PIDASHCONV-143 (GitLab), so this module owns the registration facts
//! (order, keys, triplets, lookup rule) while the URL-parse loops live with
//! the adapters. The first-match rule is restated on
//! [`ADAPTER_PARSE_ORDER`] so the adapter issues build to the same order.
//!
//! Lookup-rule notes carried over from `registry.py:18-22`:
//!
//! * Lookup lowercases the input (`(provider or "").lower()`); an absent
//!   provider (`None` in Python) maps to `""` and then fails lookup. Rust
//!   has no `None` spelling for `&str`, so callers pass `""`.
//! * The error message interpolates the *original* input, not the lowered
//!   key. Python raises `KeyError`, whose `str()` wraps the message in
//!   single quotes (`str(KeyError("m")) == "'m'"`); [`UnknownProvider`]
//!   exposes the bare [`UnknownProvider::message`] and renders it bare via
//!   `Display`, leaving any `str(exc)`-compatible quoting to the caller.
//! * Case folding uses Unicode lowercase on both sides (Python
//!   `str.lower`, Rust `str::to_lowercase`); provider keys are ASCII, so
//!   every input that can match behaves identically.

/// One row of `provider_payload` (`registry.py:45-53`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderEntry {
    pub key: &'static str,
    pub display_name: &'static str,
    pub code_review_term: &'static str,
}

/// Registration order (`_ADAPTERS`, `registry.py:12-15`): GitHub first,
/// GitLab second. `all_adapters` and `provider_payload` both iterate this
/// order; the URL-parse loops try adapters in this order and return the
/// first match (`registry.py:29-42`).
pub const PROVIDERS: [ProviderEntry; 2] = [
    ProviderEntry {
        key: "github",
        display_name: "GitHub",
        code_review_term: "pull request",
    },
    ProviderEntry {
        key: "gitlab",
        display_name: "GitLab",
        code_review_term: "merge request",
    },
];

/// Adapter keys in first-match parse order (github before gitlab).
///
/// `parse_repository_url` and `parse_code_review_url` try each adapter in
/// turn and return the first non-`None` result; unknown hosts fall through
/// to `None` (`registry.py:29-42`).
pub const ADAPTER_PARSE_ORDER: [&str; 2] = ["github", "gitlab"];

/// Lowercase lookup key for a provider name (`(provider or "").lower()`).
pub fn normalize_provider_key(provider: &str) -> String {
    provider.to_lowercase()
}

/// Unknown-provider lookup failure (`KeyError`, `registry.py:21`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProvider {
    provider: String,
}

impl UnknownProvider {
    /// The `KeyError` message with the *original* input interpolated:
    /// `Unsupported Git provider: {provider}`.
    pub fn message(&self) -> String {
        format!("Unsupported Git provider: {}", self.provider)
    }

    /// The offending input, verbatim.
    pub fn provider(&self) -> &str {
        &self.provider
    }
}

impl std::fmt::Display for UnknownProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unsupported Git provider: {}", self.provider)
    }
}

impl std::error::Error for UnknownProvider {}

/// `get_adapter` key resolution (`registry.py:18-22`).
///
/// Returns the canonical adapter key (which is also the lookup key, since
/// keys are already lowercase). Unknown providers fail with
/// [`UnknownProvider`]; matching is case-insensitive.
pub fn resolve_adapter_key(provider: &str) -> Result<&'static str, UnknownProvider> {
    let lowered = normalize_provider_key(provider);
    for entry in PROVIDERS {
        if entry.key == lowered {
            return Ok(entry.key);
        }
    }
    Err(UnknownProvider {
        provider: provider.to_string(),
    })
}

/// `all_adapters` order (`registry.py:25-26`): github, then gitlab.
pub fn all_adapter_keys() -> [&'static str; 2] {
    ADAPTER_PARSE_ORDER
}

/// `provider_payload` (`registry.py:45-53`): one
/// key/display_name/code_review_term triplet per adapter, in registration
/// order.
pub fn provider_payload() -> [ProviderEntry; 2] {
    PROVIDERS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/registry.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    #[test]
    fn unknown_provider_message_matches_fixture() {
        // Fixture records KeyError: 'Unsupported Git provider: bitbucket'
        // plus the str() form with repr quotes.
        let err = resolve_adapter_key("bitbucket").expect_err("unknown fails");
        assert_eq!(err.message(), "Unsupported Git provider: bitbucket");
        assert_eq!(err.to_string(), "Unsupported Git provider: bitbucket");
        assert_eq!(err.provider(), "bitbucket");
        let fixture = golden();
        let fixture_msg = fixture
            .get("get_adapter")
            .and_then(|g| g.get("unknown"))
            .and_then(|u| u.get("error"))
            .and_then(Value::as_str)
            .expect("fixture error string");
        assert!(
            fixture_msg.contains(&err.message()),
            "fixture {fixture_msg} contains bare message"
        );
    }

    #[test]
    fn lookup_is_case_insensitive_and_reports_original() {
        // registry.py:19 lowercases; the fixture pins get_adapter('GitHub').
        assert_eq!(resolve_adapter_key("GitHub"), Ok("github"));
        assert_eq!(resolve_adapter_key("GITHUB"), Ok("github"));
        assert_eq!(resolve_adapter_key("GitLab"), Ok("gitlab"));
        assert_eq!(resolve_adapter_key("gitlab"), Ok("gitlab"));
        assert_eq!(normalize_provider_key("GitHub"), "github");
        // The message keeps the original input, not the lowered key.
        let err = resolve_adapter_key("BitBucket").expect_err("unknown fails");
        assert_eq!(err.message(), "Unsupported Git provider: BitBucket");
        // Absent provider (Python None -> "") fails lookup too.
        let empty = resolve_adapter_key("").expect_err("empty fails");
        assert_eq!(empty.message(), "Unsupported Git provider: ");
    }

    #[test]
    fn adapter_order_matches_fixture() {
        let fixture = golden();
        let order: Vec<&str> = fixture
            .get("all_adapters")
            .and_then(|a| a.get("order"))
            .and_then(Value::as_array)
            .expect("fixture order")
            .iter()
            .map(|v| v.as_str().expect("key"))
            .collect();
        assert_eq!(all_adapter_keys().to_vec(), order);
        assert_eq!(ADAPTER_PARSE_ORDER, ["github", "gitlab"]);
    }

    #[test]
    fn provider_payload_triplets_match_fixture() {
        let fixture = golden();
        let expected = fixture
            .get("provider_payload")
            .and_then(|p| p.get("output"))
            .expect("fixture payload");
        let replayed = serde_json::to_value(
            provider_payload()
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "key": e.key,
                        "display_name": e.display_name,
                        "code_review_term": e.code_review_term,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .expect("value");
        assert_eq!(&replayed, expected);
    }
}
