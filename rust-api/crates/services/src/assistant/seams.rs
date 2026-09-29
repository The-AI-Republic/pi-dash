//! CE provider seams for the assistant (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/ee/assistant/model_provider.py:1-134` and
//! `apps/api/pi_dash/ee/assistant/stt_provider.py:1-72`: the open-source
//! side of the seams the assistant runtime calls instead of reading BYOK
//! tables directly, so the cloud build can overlay platform-provided
//! credentials at a single switch point. Fixture id F-A6-09
//! (`rust-api/fixtures/assistant/mcp.json`, `ee_model_provider` +
//! `ee_stt_provider`).
//!
//! Shape notes:
//!
//! * The cloud overlay stays out of scope: every function here is the CE
//!   branch only (BYOK key / BYO STT endpoint).
//! * Row fetches (`get_config`, the `UserSTTConfig` lookup) and the KMS
//!   decrypt stay with the handler layer, which owns database and network
//!   handles (the `llm.rs` precedent). Callers pass the already-read
//!   presence bits, verdicts and decrypted values; this module ports the
//!   gating order, the profile matrix, and the exact error codes/messages.
//! * `resolve_model_for_user` is CE-identical to `resolve_byok_model`
//!   (`model_provider.py:56-63`), and `generate_title_for_user` to the BYOK
//!   title helper; both delegate to the ported [`crate::assistant::llm`] /
//!   [`crate::assistant::title`] decision surface rather than duplicating
//!   it. `resolve_toolsets_for_user` is `build_toolsets` outright
//!   (`model_provider.py:122-134`); see [`crate::assistant::mcp`].

use pidash_types::assistant::errors::AssistantError;

use crate::assistant::llm::{self, ModelRef};

/// Desktop-lane reason when the user holds a BYOK key
/// (`managed_runner/errors.py:30`, `BYOK_UNSUPPORTED`).
pub const REASON_BYOK_UNSUPPORTED: &str = "byok_not_supported_on_desktop";
/// Desktop-lane reason when the user holds no usable LLM config
/// (`managed_runner/errors.py:25`, `LLM_CONFIG_MISSING`).
pub const REASON_LLM_CONFIG_MISSING: &str = "llm_config_missing";

/// Missing-STT-config detail (`stt_provider.py:67`).
pub const MSG_STT_CONFIG_MISSING: &str = "Configure dictation in Settings.";
/// Blocked-endpoint detail (`stt_provider.py:69`).
pub const MSG_BASE_URL_BLOCKED: &str = "That endpoint host is not allowed.";
/// Desktop-credential refusal detail (`model_provider.py:105-108`).
pub const MSG_NO_DESKTOP_LANE: &str = "This build has no model lane the desktop agent can use.";

/// True when the user has a usable LLM configuration
/// (`has_usable_llm_config`, `model_provider.py:45-53`): the config row
/// exists and holds a key. A cheap presence check for request-time gating —
/// it must not build a model or decrypt anything.
pub fn has_usable_llm_config(has_api_key: bool) -> bool {
    has_api_key
}

/// Return the in-process model for the user (`resolve_model_for_user`,
/// `model_provider.py:56-63`).
///
/// CE resolves BYOK directly, so this is [`llm::resolve_byok_model`] with
/// the same gates and `llm_config_missing` / `assistant_not_configured`
/// failures. Raises `AssistantError` with code `llm_config_missing` when
/// the user has no usable configuration.
pub fn resolve_model_for_user(
    has_api_key: bool,
    model_name: &str,
    provider_kind: &str,
    base_url: &str,
    base_url_blocked: bool,
) -> Result<ModelRef, AssistantError> {
    llm::resolve_byok_model(
        has_api_key,
        model_name,
        provider_kind,
        base_url,
        base_url_blocked,
    )
}

/// How the desktop agent engine should reach a model for one user
/// (`AgentModelProfile`, `model_provider.py:25-42`).
///
/// The credential itself is never part of this object (see
/// [`agent_model_credential_for_user`]). `reason_code` is a
/// `ManagedRunnerReason` value and is empty exactly when `available` is
/// true; CE never serves the desktop, so `available` is always false here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelProfile {
    pub available: bool,
    pub lane: String,
    pub base_url: String,
    pub model: String,
    pub reason_code: String,
}

/// Describe the model endpoint the desktop engine may call
/// (`agent_model_profile_for_user`, `model_provider.py:71-89`).
///
/// CE has exactly one lane, BYOK, and the desktop engine deliberately does
/// not serve it: the stored key is decrypted only inside a server process
/// at the moment of use and the API has no read path for it, so honouring
/// BYOK on the desktop would add the first endpoint returning a stored key
/// to a client. Matrix: key held → `{available: false, lane: "byok",
/// reason: byok_unsupported}`; no key → `{available: false, lane: "",
/// reason: llm_config_missing}`.
pub fn agent_model_profile_for_user(has_api_key: bool) -> AgentModelProfile {
    if has_api_key {
        AgentModelProfile {
            available: false,
            lane: "byok".to_owned(),
            base_url: String::new(),
            model: String::new(),
            reason_code: REASON_BYOK_UNSUPPORTED.to_owned(),
        }
    } else {
        AgentModelProfile {
            available: false,
            lane: String::new(),
            base_url: String::new(),
            model: String::new(),
            reason_code: REASON_LLM_CONFIG_MISSING.to_owned(),
        }
    }
}

/// Refusal when a managed run cannot be admitted
/// (`ManagedRunnerUnavailable`, `managed_runner/errors.py:36-46`):
/// `code` is the reason, `str(exc)` the detail (or the code when empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRunnerUnavailable {
    pub code: String,
    pub detail: String,
}

impl ManagedRunnerUnavailable {
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ManagedRunnerUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{}", self.detail)
        }
    }
}

impl std::error::Error for ManagedRunnerUnavailable {}

/// Return the desktop engine's model credential
/// (`agent_model_credential_for_user`, `model_provider.py:92-108`).
///
/// CE has no lane the desktop can use, so there is nothing to hand out.
/// Raising rather than returning empty keeps the failure loud: a caller
/// that ignored [`agent_model_profile_for_user`] must not silently receive
/// a blank credential and produce a 401 deep inside a run. The `Ok` shape
/// is `(token, expires_at)`, which only the cloud overlay ever returns.
pub fn agent_model_credential_for_user() -> Result<(String, String), ManagedRunnerUnavailable> {
    Err(ManagedRunnerUnavailable::new(
        REASON_BYOK_UNSUPPORTED,
        MSG_NO_DESKTOP_LANE,
    ))
}

/// Human-readable label for the model a turn will use
/// (`model_label_for_user`, `model_provider.py:111-119`): the BYOK
/// `<kind>:<model>` label, recorded on the turn as `model_used`; empty when
/// the user holds no config.
pub fn model_label_for_user(config: Option<(&str, &str)>) -> String {
    match config {
        Some((provider_kind, model_name)) => llm::model_label(provider_kind, model_name),
        None => String::new(),
    }
}

/// A ready-to-call transcription endpoint for one user
/// (`ResolvedSTTProvider`, `stt_provider.py:29-40`).
///
/// `base_url` is the OpenAI-compatible root; `api_key` the decrypted
/// credential for the `Authorization` header; `model` the transcription
/// model slug. The cloud overlay produces the same shape, so callers never
/// branch on build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSttProvider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// True when the user has a usable dictation configuration
/// (`has_usable_stt_config`, `stt_provider.py:43-51`): the config row
/// exists and holds a key. A cheap presence check for request-time gating
/// (rejecting a transcribe call before streaming a doomed upload) — it
/// must not decrypt anything.
pub fn has_usable_stt_config(has_api_key: bool) -> bool {
    has_api_key
}

/// Resolve the user's transcription endpoint
/// (`resolve_stt_provider`, `stt_provider.py:54-72`).
///
/// Gates, in order: no config or key → `STTConfigMissing`; configured host
/// refused by the SSRF guard → `BaseUrlBlocked`; otherwise the decrypted
/// key (a decrypt failure propagates as its `AssistantError`). The SSRF
/// guard is re-run here, at execution time, rather than trusting the
/// save-time check: DNS can be re-pointed at a private address after the
/// URL was stored. An empty `base_url` skips the SSRF check
/// (`if cfg.base_url and ssrf.is_blocked(...)`); the caller precomputes
/// `base_url_blocked` for the configured host.
pub fn resolve_stt_provider(
    has_api_key: bool,
    base_url: &str,
    model_name: &str,
    base_url_blocked: bool,
    decrypt: impl FnOnce() -> Result<String, AssistantError>,
) -> Result<ResolvedSttProvider, AssistantError> {
    if !has_api_key {
        return Err(AssistantError::SttConfigMissing(
            MSG_STT_CONFIG_MISSING.to_owned(),
        ));
    }
    if !base_url.is_empty() && base_url_blocked {
        return Err(AssistantError::BaseUrlBlocked(
            MSG_BASE_URL_BLOCKED.to_owned(),
        ));
    }
    let api_key = decrypt()?;
    Ok(ResolvedSttProvider {
        base_url: base_url.to_owned(),
        api_key,
        model: model_name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::llm::PROVIDER_ANTHROPIC;

    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/assistant/mcp.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn llm_gate_is_cheap_presence_check() {
        let fx = fixture();
        assert!(fx["ee_model_provider"]["has_usable_llm_config"]["rule"]
            .as_str()
            .unwrap()
            .contains("no decrypt"));
        assert!(has_usable_llm_config(true));
        assert!(!has_usable_llm_config(false));
    }

    #[test]
    fn resolve_model_delegates_to_byok() {
        let fx = fixture();
        assert!(fx["ee_model_provider"]["resolve_model_for_user"]["rule"]
            .as_str()
            .unwrap()
            .contains("resolve_byok_model"));
        let model =
            resolve_model_for_user(true, "claude-sonnet-4-5", PROVIDER_ANTHROPIC, "", false)
                .expect("resolves");
        assert_eq!(
            model,
            ModelRef::Anthropic {
                model: "claude-sonnet-4-5".to_owned()
            }
        );
        let err = resolve_model_for_user(false, "", "", "", false).expect_err("no key");
        assert_eq!(err.code(), "llm_config_missing");
    }

    #[test]
    fn agent_profile_matrix_matches_ce() {
        let fx = fixture();
        let matrix = fx["ee_model_provider"]["agent_model_profile_for_user"]["ce_matrix"]
            .as_array()
            .unwrap();
        assert_eq!(matrix.len(), 2);
        let with_key = agent_model_profile_for_user(true);
        assert!(!with_key.available);
        assert_eq!(with_key.lane, "byok");
        assert_eq!(with_key.base_url, "");
        assert_eq!(with_key.model, "");
        assert_eq!(with_key.reason_code, REASON_BYOK_UNSUPPORTED);
        assert_eq!(with_key.reason_code, "byok_not_supported_on_desktop");
        let without_key = agent_model_profile_for_user(false);
        assert!(!without_key.available);
        assert_eq!(without_key.lane, "");
        assert_eq!(without_key.reason_code, REASON_LLM_CONFIG_MISSING);
        assert_eq!(without_key.reason_code, "llm_config_missing");
    }

    #[test]
    fn agent_credential_always_raises_loud() {
        let fx = fixture();
        assert!(
            fx["ee_model_provider"]["agent_model_credential_for_user"]["rule"]
                .as_str()
                .unwrap()
                .contains("always raises")
        );
        let err = agent_model_credential_for_user().expect_err("CE never hands out");
        assert_eq!(err.code, REASON_BYOK_UNSUPPORTED);
        assert_eq!(err.detail, MSG_NO_DESKTOP_LANE);
        assert_eq!(
            err.to_string(),
            "This build has no model lane the desktop agent can use."
        );
        // Empty-detail Display falls back to the code (the ValueError init).
        let bare = ManagedRunnerUnavailable::new("llm_config_missing", "");
        assert_eq!(bare.to_string(), "llm_config_missing");
    }

    #[test]
    fn model_label_empty_without_config() {
        let fx = fixture();
        assert!(fx["ee_model_provider"]["model_label_for_user"]["rule"]
            .as_str()
            .unwrap()
            .contains("else ''"));
        assert_eq!(
            model_label_for_user(Some(("openai_compatible", "gpt-4o"))),
            "openai_compatible:gpt-4o"
        );
        assert_eq!(model_label_for_user(None), "");
    }

    #[test]
    fn stt_gate_is_cheap_presence_check() {
        let fx = fixture();
        assert!(fx["ee_stt_provider"]["has_usable_stt_config"]["rule"]
            .as_str()
            .unwrap()
            .contains("no decrypt"));
        assert!(has_usable_stt_config(true));
        assert!(!has_usable_stt_config(false));
    }

    #[test]
    fn stt_resolve_gate_order_and_codes() {
        let fx = fixture();
        let gates = fx["ee_stt_provider"]["resolve_stt_provider"]["gates"]
            .as_array()
            .unwrap();
        assert_eq!(gates.len(), 3);
        // No config/key first, even when the URL would also be blocked.
        let missing = resolve_stt_provider(false, "https://x.example/", "whisper-1", true, || {
            panic!("must not decrypt without a key")
        })
        .expect_err("missing config");
        assert_eq!(missing.code(), "stt_config_missing");
        assert_eq!(missing.http_status(), 422);
        assert_eq!(missing.detail(), MSG_STT_CONFIG_MISSING);
        assert_eq!(missing.detail(), "Configure dictation in Settings.");
        // Blocked host second, without decrypting.
        let blocked = resolve_stt_provider(true, "https://x.example/", "whisper-1", true, || {
            panic!("must not decrypt a blocked host")
        })
        .expect_err("blocked host");
        assert_eq!(blocked.code(), "base_url_blocked");
        assert_eq!(blocked.http_status(), 400);
        assert_eq!(blocked.detail(), MSG_BASE_URL_BLOCKED);
        // Decrypt failure propagates as its AssistantError.
        let undecryptable =
            resolve_stt_provider(true, "https://x.example/", "whisper-1", false, || {
                Err(AssistantError::AssistantNotConfigured(
                    "retired key".to_owned(),
                ))
            })
            .expect_err("bad ciphertext");
        assert_eq!(undecryptable.code(), "assistant_not_configured");
        // Happy path carries the endpoint shape verbatim.
        let resolved =
            resolve_stt_provider(true, "https://stt.example.com", "whisper-1", false, || {
                Ok("example-plaintext".to_owned())
            })
            .expect("resolves");
        assert_eq!(
            resolved,
            ResolvedSttProvider {
                base_url: "https://stt.example.com".to_owned(),
                api_key: "example-plaintext".to_owned(),
                model: "whisper-1".to_owned(),
            }
        );
        // Empty base URL skips the SSRF verdict (Python `and`).
        let empty_base = resolve_stt_provider(true, "", "whisper-1", true, || {
            Ok("example-plaintext".to_owned())
        })
        .expect("empty base skips the guard");
        assert_eq!(empty_base.base_url, "");
    }
}
