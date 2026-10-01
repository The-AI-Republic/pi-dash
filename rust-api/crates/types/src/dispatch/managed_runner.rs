//! Managed runner refusal reasons (D-11, stage 5).
//!
//! Port of `apps/api/pi_dash/managed_runner/errors.py:15-45`:
//!
//! * `ManagedRunnerReason` (`:15-33`) → [`ManagedRunnerReason`].
//! * `ManagedRunnerUnavailable` (`:36-45`) → [`ManagedRunnerUnavailable`].
//!
//! Translation notes:
//!
//! * The Python codes are plain class attributes; the port keeps them as
//!   associated `&'static str` constants in the same precedence order
//!   (`errors.py:16-18`). They surface as `error_code` values and as the
//!   `{"error": code, "detail": str}` 409 body
//!   (`assistant/views/agent_profile.py:89-95`); that surfacing lives with
//!   the handlers layer, not here.
//! * `str(exc)` is `detail or code` (`errors.py:45`); [`Display`] reproduces
//!   the fallback while [`ManagedRunnerUnavailable::detail`] stays raw.
//!   `ValueError` becomes `std::error::Error`.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json`
//! (`managed_runner`).
//!
//! Ported bugs: none found in this unit on read-through.

/// Reason codes, in the precedence order `managed_runner_availability`
/// evaluates them (`errors.py:16-18`): the first failing gate wins, which is
/// what makes the copy deterministic for a given user/project pair.
pub struct ManagedRunnerReason;

impl ManagedRunnerReason {
    /// The operator kill switch is off for this instance.
    pub const DISABLED: &'static str = "managed_runner_disabled";
    /// No viewer supplied, or the viewer's desktop app is not connected.
    pub const NOT_CONNECTED: &'static str = "desktop_not_connected";
    /// The viewer has no usable LLM configuration at all.
    pub const LLM_CONFIG_MISSING: &'static str = "llm_config_missing";
    /// The viewer's session predates the gateway scopes and cannot acquire them.
    pub const GATEWAY_SCOPES_MISSING: &'static str = "gateway_scopes_missing";
    /// The viewer's provider is BYOK, which the desktop engine does not serve
    /// in the MVP (no stored key is ever returned to a client).
    pub const BYOK_UNSUPPORTED: &'static str = "byok_not_supported_on_desktop";
    /// The desktop is connected but has not enrolled a runner for this project
    /// yet. The desktop resolves this silently by enrolling and retrying.
    pub const NO_RUNNER_FOR_PROJECT: &'static str = "no_managed_runner_for_project";
}

/// Raised at creation when a managed run cannot be admitted
/// (`errors.py:36-45`).
///
/// `code` is a [`ManagedRunnerReason`] value; `Display` is the human-readable
/// sentence for API responses (`detail or code`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRunnerUnavailable {
    code: String,
    detail: String,
}

impl ManagedRunnerUnavailable {
    /// `ManagedRunnerUnavailable(code, detail="")` (`errors.py:43-45`).
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        ManagedRunnerUnavailable {
            code: code.into(),
            detail: detail.into(),
        }
    }

    /// The reason code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The raw detail (possibly empty; see [`Display`](std::fmt::Display)).
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ManagedRunnerUnavailable {
    /// `str(exc)`: the detail, or the code when empty (`errors.py:45`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{}", self.detail)
        }
    }
}

impl std::error::Error for ManagedRunnerUnavailable {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-01-types.golden.json");

    fn managed_fixture() -> Value {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        fixture["managed_runner"].clone()
    }

    #[test]
    fn reason_codes_match_fixture_verbatim() {
        let codes = &managed_fixture()["codes"];
        assert_eq!(codes["DISABLED"], ManagedRunnerReason::DISABLED);
        assert_eq!(codes["NOT_CONNECTED"], ManagedRunnerReason::NOT_CONNECTED);
        assert_eq!(
            codes["LLM_CONFIG_MISSING"],
            ManagedRunnerReason::LLM_CONFIG_MISSING
        );
        assert_eq!(
            codes["GATEWAY_SCOPES_MISSING"],
            ManagedRunnerReason::GATEWAY_SCOPES_MISSING
        );
        assert_eq!(
            codes["BYOK_UNSUPPORTED"],
            ManagedRunnerReason::BYOK_UNSUPPORTED
        );
        assert_eq!(
            codes["NO_RUNNER_FOR_PROJECT"],
            ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
        );
        assert_eq!(codes.as_object().expect("codes").len(), 6);
    }

    #[test]
    fn unavailable_shape_matches_fixture() {
        let golden = managed_fixture();
        // is_ValueError: the Rust error implements std::error::Error.
        assert_eq!(golden["is_ValueError"], true);
        let err = ManagedRunnerUnavailable::new(ManagedRunnerReason::DISABLED, "");
        let _: &dyn std::error::Error = &err;

        // default_detail_is_code: str(exc) is the code when detail is empty.
        assert_eq!(golden["default_detail_is_code"], true);
        assert_eq!(err.to_string(), ManagedRunnerReason::DISABLED);
        assert_eq!(err.detail(), "");

        // code_attr + explicit_detail.
        assert_eq!(golden["code_attr"], ManagedRunnerReason::DISABLED);
        assert_eq!(err.code(), golden["code_attr"].as_str().expect("code"));
        let explicit = ManagedRunnerUnavailable::new(
            ManagedRunnerReason::DISABLED,
            golden["explicit_detail"].as_str().expect("detail"),
        );
        assert_eq!(explicit.to_string(), "custom detail");
        assert_eq!(explicit.code(), ManagedRunnerReason::DISABLED);
    }
}
