//! Assistant error taxonomy (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/errors.py:15-107`: the `AssistantError`
//! base (`code = "internal"`, `http_status = 500`) with its 13 subclasses,
//! plus the thread/message size limits (`MAX_THREAD_MESSAGES = 200`,
//! `MAX_MESSAGE_CHARS = 32_000`).
//!
//! Shape notes:
//!
//! * Every Python subclass is an `AssistantError` (`except AssistantError`
//!   catches all of them); the Rust enum makes this structural — every
//!   variant *is* an [`AssistantError`].
//! * `str(exc)` is the detail, or the class name when the detail is empty
//!   (`super().__init__(detail or self.__class__.__name__)`, `errors.py:22`);
//!   [`AssistantError::detail`] is the raw detail (possibly empty) while the
//!   [`Display`](std::fmt::Display) impl reproduces the fallback. Verified
//!   against the live module: `str(TurnActive()) == "TurnActive"`,
//!   `str(AssistantError()) == "AssistantError"`.
//! * The base `__init__` accepts per-instance `code=` / `http_status=`
//!   overrides (`errors.py:21-27`), inherited by every subclass. No caller in
//!   `assistant/` or `ee/assistant/` uses them (zero `code=` / `http_status=`
//!   call sites; `AssistantError(` is never constructed directly), so the
//!   override spells as the [`AssistantError::Custom`] variant carrying the
//!   base only — subclass-level overrides have no exercised behavior to port.
//! * Codes surface either as synchronous HTTP errors on the REST endpoints or
//!   as `turn_failed` events paired with an `error` message row
//!   (`errors.py:5-10`); that surfacing lives with the handlers/runtime
//!   layers, not here.

use std::fmt;

/// Thread cap (`errors.py:106`).
pub const MAX_THREAD_MESSAGES: u32 = 200;
/// Message body cap (`errors.py:107`; note the Python underscore literal).
pub const MAX_MESSAGE_CHARS: u32 = 32_000;

/// Typed assistant failure (`errors.py:15-102`).
///
/// Each variant carries the message Python would put in `Exception.args`
/// (surfaced via `str(exc)`); it may be empty, in which case `Display`
/// renders the class name, exactly like Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantError {
    /// Base failure (`AssistantError` raised directly) → `internal` / 500.
    Internal(String),
    /// No usable chat provider configured → `llm_config_missing` / 422.
    LlmConfigMissing(String),
    /// No usable dictation provider configured → `stt_config_missing` / 422.
    SttConfigMissing(String),
    /// Assistant backing unavailable → `assistant_not_configured` / 503.
    AssistantNotConfigured(String),
    /// Caller role may not use the assistant → `role_not_allowed` / 403.
    RoleNotAllowed(String),
    /// A turn is already running on the thread → `turn_active` / 409.
    TurnActive(String),
    /// Thread hit [`MAX_THREAD_MESSAGES`] → `thread_full` / 409.
    ThreadFull(String),
    /// Caller exceeded its quota → `quota_exceeded` / 402.
    QuotaExceeded(String),
    /// Provider base URL failed the SSRF gate → `base_url_blocked` / 400.
    BaseUrlBlocked(String),
    /// Provider rejected the credential at runtime → `provider_auth_failed` / 502.
    ProviderAuthFailed(String),
    /// Provider unreachable at runtime → `provider_unreachable` / 502.
    ProviderUnreachable(String),
    /// Provider rejected the model name → `model_invalid` / 400.
    ModelInvalid(String),
    /// Turn exceeded its deadline → `turn_timeout` / 504.
    TurnTimeout(String),
    /// Agent exceeded its iteration budget → `iteration_limit` / 400.
    IterationLimit(String),
    /// Base raised with explicit `code=` / `http_status=` overrides
    /// (`errors.py:21-27`).
    Custom {
        code: String,
        http_status: u16,
        detail: String,
    },
}

impl AssistantError {
    /// Stable machine code (`code` class attribute, or the override).
    pub fn code(&self) -> &str {
        match self {
            AssistantError::Internal(_) => "internal",
            AssistantError::LlmConfigMissing(_) => "llm_config_missing",
            AssistantError::SttConfigMissing(_) => "stt_config_missing",
            AssistantError::AssistantNotConfigured(_) => "assistant_not_configured",
            AssistantError::RoleNotAllowed(_) => "role_not_allowed",
            AssistantError::TurnActive(_) => "turn_active",
            AssistantError::ThreadFull(_) => "thread_full",
            AssistantError::QuotaExceeded(_) => "quota_exceeded",
            AssistantError::BaseUrlBlocked(_) => "base_url_blocked",
            AssistantError::ProviderAuthFailed(_) => "provider_auth_failed",
            AssistantError::ProviderUnreachable(_) => "provider_unreachable",
            AssistantError::ModelInvalid(_) => "model_invalid",
            AssistantError::TurnTimeout(_) => "turn_timeout",
            AssistantError::IterationLimit(_) => "iteration_limit",
            AssistantError::Custom { code, .. } => code,
        }
    }

    /// HTTP status for this error (`http_status` class attribute, or the
    /// override).
    pub fn http_status(&self) -> u16 {
        match self {
            AssistantError::Internal(_) => 500,
            AssistantError::LlmConfigMissing(_) => 422,
            AssistantError::SttConfigMissing(_) => 422,
            AssistantError::AssistantNotConfigured(_) => 503,
            AssistantError::RoleNotAllowed(_) => 403,
            AssistantError::TurnActive(_) => 409,
            AssistantError::ThreadFull(_) => 409,
            AssistantError::QuotaExceeded(_) => 402,
            AssistantError::BaseUrlBlocked(_) => 400,
            AssistantError::ProviderAuthFailed(_) => 502,
            AssistantError::ProviderUnreachable(_) => 502,
            AssistantError::ModelInvalid(_) => 400,
            AssistantError::TurnTimeout(_) => 504,
            AssistantError::IterationLimit(_) => 400,
            AssistantError::Custom { http_status, .. } => *http_status,
        }
    }

    /// The raw detail (`self.detail`, possibly empty).
    pub fn detail(&self) -> &str {
        match self {
            AssistantError::Internal(d)
            | AssistantError::LlmConfigMissing(d)
            | AssistantError::SttConfigMissing(d)
            | AssistantError::AssistantNotConfigured(d)
            | AssistantError::RoleNotAllowed(d)
            | AssistantError::TurnActive(d)
            | AssistantError::ThreadFull(d)
            | AssistantError::QuotaExceeded(d)
            | AssistantError::BaseUrlBlocked(d)
            | AssistantError::ProviderAuthFailed(d)
            | AssistantError::ProviderUnreachable(d)
            | AssistantError::ModelInvalid(d)
            | AssistantError::TurnTimeout(d)
            | AssistantError::IterationLimit(d) => d,
            AssistantError::Custom { detail, .. } => detail,
        }
    }

    /// Python class name (the empty-detail `str(exc)` fallback).
    pub fn class_name(&self) -> &'static str {
        match self {
            AssistantError::Internal(_) => "AssistantError",
            AssistantError::LlmConfigMissing(_) => "LLMConfigMissing",
            AssistantError::SttConfigMissing(_) => "STTConfigMissing",
            AssistantError::AssistantNotConfigured(_) => "AssistantNotConfigured",
            AssistantError::RoleNotAllowed(_) => "RoleNotAllowed",
            AssistantError::TurnActive(_) => "TurnActive",
            AssistantError::ThreadFull(_) => "ThreadFull",
            AssistantError::QuotaExceeded(_) => "QuotaExceeded",
            AssistantError::BaseUrlBlocked(_) => "BaseUrlBlocked",
            AssistantError::ProviderAuthFailed(_) => "ProviderAuthFailed",
            AssistantError::ProviderUnreachable(_) => "ProviderUnreachable",
            AssistantError::ModelInvalid(_) => "ModelInvalid",
            AssistantError::TurnTimeout(_) => "TurnTimeout",
            AssistantError::IterationLimit(_) => "IterationLimit",
            AssistantError::Custom { .. } => "AssistantError",
        }
    }
}

impl fmt::Display for AssistantError {
    /// `str(exc)`: the detail, or the class name when empty
    /// (`errors.py:22`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = self.detail();
        if detail.is_empty() {
            write!(f, "{}", self.class_name())
        } else {
            write!(f, "{detail}")
        }
    }
}

impl std::error::Error for AssistantError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/errors.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn all_variants() -> Vec<AssistantError> {
        vec![
            AssistantError::Internal(String::new()),
            AssistantError::LlmConfigMissing(String::new()),
            AssistantError::SttConfigMissing(String::new()),
            AssistantError::AssistantNotConfigured(String::new()),
            AssistantError::RoleNotAllowed(String::new()),
            AssistantError::TurnActive(String::new()),
            AssistantError::ThreadFull(String::new()),
            AssistantError::QuotaExceeded(String::new()),
            AssistantError::BaseUrlBlocked(String::new()),
            AssistantError::ProviderAuthFailed(String::new()),
            AssistantError::ProviderUnreachable(String::new()),
            AssistantError::ModelInvalid(String::new()),
            AssistantError::TurnTimeout(String::new()),
            AssistantError::IterationLimit(String::new()),
        ]
    }

    #[test]
    fn base_code_and_status_match_fixture() {
        let fixture = golden();
        let base = &fixture["base"];
        assert_eq!(base["code"], "internal");
        assert_eq!(base["http_status"], 500);
        let err = AssistantError::Internal(String::new());
        assert_eq!(err.code(), "internal");
        assert_eq!(err.http_status(), 500);
    }

    #[test]
    fn every_subclass_code_and_status_matches_fixture() {
        let fixture = golden();
        let errors = fixture["errors"].as_array().expect("errors array");
        assert_eq!(errors.len(), 13, "all 13 subclasses recorded");
        let variants = all_variants();
        // all_variants()[0] is the base; the rest parallel the fixture order.
        assert_eq!(variants.len(), errors.len() + 1);
        for (variant, expected) in variants.iter().skip(1).zip(errors.iter()) {
            assert_eq!(variant.code(), expected["code"].as_str().expect("code"),);
            assert_eq!(
                variant.http_status(),
                expected["http_status"].as_u64().expect("status") as u16,
            );
        }
    }

    #[test]
    fn init_override_vector_matches_fixture() {
        // errors.json init_vector: AssistantError("detail-x",
        // code="turn_active", http_status=409).
        let vector = &golden()["init_vector"];
        let err = AssistantError::Custom {
            code: "turn_active".into(),
            http_status: 409,
            detail: "detail-x".into(),
        };
        assert_eq!(err.to_string(), vector["str"].as_str().expect("str"));
        assert_eq!(err.detail(), vector["detail"].as_str().expect("detail"));
        assert_eq!(err.code(), vector["code"].as_str().expect("code"));
        assert_eq!(
            err.http_status(),
            vector["http_status"].as_u64().expect("status") as u16
        );
    }

    #[test]
    fn empty_detail_renders_class_name_like_str_exc() {
        // str(TurnActive()) == "TurnActive"; str(AssistantError()) ==
        // "AssistantError" (probed against the live module).
        assert_eq!(
            AssistantError::TurnActive(String::new()).to_string(),
            "TurnActive"
        );
        assert_eq!(
            AssistantError::Internal(String::new()).to_string(),
            "AssistantError"
        );
        assert_eq!(
            AssistantError::TurnActive("boom".into()).to_string(),
            "boom"
        );
        // detail() itself stays raw (possibly empty).
        assert_eq!(AssistantError::TurnActive(String::new()).detail(), "");
    }

    #[test]
    fn limits_match_fixture() {
        let limits = &golden()["limits"];
        assert_eq!(
            MAX_THREAD_MESSAGES,
            limits["MAX_THREAD_MESSAGES"].as_u64().expect("limit") as u32
        );
        assert_eq!(
            MAX_MESSAGE_CHARS,
            limits["MAX_MESSAGE_CHARS"].as_u64().expect("limit") as u32
        );
        assert_eq!(MAX_THREAD_MESSAGES, 200);
        assert_eq!(MAX_MESSAGE_CHARS, 32_000);
    }

    #[test]
    fn every_variant_is_an_assistant_error() {
        // except AssistantError catches the subclasses: the enum makes this
        // structural, and Custom keeps the base override spelling.
        for err in all_variants() {
            let _: &AssistantError = &err;
            let _: &dyn std::error::Error = &err;
        }
        let custom = AssistantError::Custom {
            code: "x".into(),
            http_status: 400,
            detail: String::new(),
        };
        assert_eq!(custom.to_string(), "AssistantError");
        assert_eq!(custom.class_name(), "AssistantError");
    }
}
