#![forbid(unsafe_code)]

//! Overlay seams for the `ee/` stubs (inventory §4 item 2):
//! `ee/assistant/model_provider.py`, `ee/assistant/stt_provider.py`,
//! `ee/cloud_agent/model_provider.py`, `ee/cloud_agent/toolsets.py`.
//! (`ee/authentication/desktop.py` lives in `pidash-auth` next to its
//! permission; `ee/settings/user_settings.py` next to
//! [`crate::user_settings`].)
//!
//! Every seam is a trait with the CE behaviour as the default
//! implementation, following the `SettingsOverlay` precedent: the private
//! crate replaces the implementation, never the callers. Inputs are data,
//! not requests or ORM rows — the caller extracts what its build's check
//! needs (a presence flag, a session value, a decrypted key) exactly like
//! `is_desktop_session(authenticated, session_value)` already does.
//!
//! Model and toolset *construction* (pydantic-ai models, MCP toolsets)
//! needs the assistant/cloud-agent runtimes, which arrive with their domain
//! ports (D-06, D-11) and extend these same traits; what is here is the
//! portable decision surface, fully tested, with no placeholders.
//!
//! Explicit map for the private crate:
//!
//! | Seam trait | Python source | CE default | Overlay replaces with |
//! |---|---|---|
//! | `AssistantModelSeam` | `ee/assistant/model_provider.py` | `ByokAssistantSeam` | platform credentials (OpenHub lane) |
//! | `SttSeam` | `ee/assistant/stt_provider.py` | `ByoSttSeam` | OpenHub relay endpoint |
//! | `CloudAgentModelSeam` | `ee/cloud_agent/model_provider.py` | `CreatorModelSeam` | admission/model routing |
//! | `CloudAgentToolsetsSeam` | `ee/cloud_agent/toolsets.py` | `NoExtraToolsets` | gateway toolsets |
//! | `DesktopGate` (in `pidash-auth`) | `ee/authentication/desktop.py` | `SessionDesktopGate` | OIDC `client` claim |
//! | `UserSettingsSchema` (in `user_settings`) | `ee/settings/user_settings.py` | `CeUserSettings` | extended schema |

/// Reason codes from `managed_runner/errors.py::ManagedRunnerReason`,
/// in the values the desktop profile endpoint actually serialises.
pub mod reason {
    /// The viewer has no usable LLM configuration at all.
    pub const LLM_CONFIG_MISSING: &str = "llm_config_missing";
    /// The viewer's provider is BYOK, which the desktop engine does not serve.
    pub const BYOK_UNSUPPORTED: &str = "byok_not_supported_on_desktop";
}

/// How the desktop agent engine should reach a model for one user
/// (`ee/assistant/model_provider.py::AgentModelProfile`).
///
/// The credential itself is never part of this object — the desktop is
/// handed it separately through the agent-token endpoint. `reason_code` is
/// empty exactly when `available` is true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelProfile {
    pub available: bool,
    pub lane: String,
    pub base_url: String,
    pub model: String,
    pub reason_code: String,
}

/// CE credential failure (`ManagedRunnerUnavailable`): raising rather than
/// returning empty keeps the failure loud — a caller that ignored the
/// profile must not silently receive a blank credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialUnavailable {
    pub reason: String,
    pub message: String,
}

/// Overlay seam for `ee/assistant/model_provider.py`.
///
/// CE is BYOK-only: every user brings their own LLM key, and the desktop
/// engine deliberately does not serve that lane (the stored key is
/// decrypted only inside a server process at the moment of use, so
/// honouring BYOK on the desktop would mean adding the first endpoint that
/// returns a stored key to a client).
pub trait AssistantModelSeam {
    /// Cheap presence check for request-time gating. Must not build a
    /// model or decrypt anything; `has_api_key` is the caller's read of
    /// the user's config.
    fn has_usable_llm_config(&self, has_api_key: bool) -> bool {
        has_api_key
    }

    /// Describe the model endpoint the desktop engine may call. CE has
    /// exactly one lane, BYOK, which the desktop does not serve — so this
    /// is always unavailable; only the reason differs.
    fn agent_model_profile(&self, has_api_key: bool) -> AgentModelProfile {
        if has_api_key {
            AgentModelProfile {
                available: false,
                lane: "byok".to_string(),
                base_url: String::new(),
                model: String::new(),
                reason_code: reason::BYOK_UNSUPPORTED.to_string(),
            }
        } else {
            AgentModelProfile {
                available: false,
                lane: String::new(),
                base_url: String::new(),
                model: String::new(),
                reason_code: reason::LLM_CONFIG_MISSING.to_string(),
            }
        }
    }

    /// Hand out the desktop engine's model credential. CE has no lane the
    /// desktop can use, so there is nothing to hand out — always an error.
    fn agent_model_credential(&self) -> Result<(String, String), CredentialUnavailable> {
        Err(CredentialUnavailable {
            reason: reason::BYOK_UNSUPPORTED.to_string(),
            message: "This build has no model lane the desktop agent can use.".to_string(),
        })
    }

    /// Human-readable label for the model a turn will use, recorded on the
    /// turn as `model_used`. Empty when the user has no configuration.
    fn model_label(&self, configured: bool, byok_label: &str) -> String {
        if configured {
            byok_label.to_string()
        } else {
            String::new()
        }
    }
}

/// CE assistant seam: BYOK only.
pub struct ByokAssistantSeam;

impl AssistantModelSeam for ByokAssistantSeam {}

/// A ready-to-call transcription endpoint for one user
/// (`ee/assistant/stt_provider.py::ResolvedSTTProvider`).
///
/// `base_url` is the OpenAI-compatible root; `api_key` the credential to
/// send as bearer; `model` the transcription model slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSttProvider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// Inputs to STT resolution, extracted by the caller: the stored config
/// (if any), the execution-time SSRF verdict on its host, and the
/// decrypted key (if decryption succeeded).
pub struct SttResolution {
    pub has_api_key: bool,
    pub base_url: String,
    pub model: String,
    /// `None` when there is no stored key or it would not decrypt.
    pub api_key: Option<String>,
    /// The caller's SSRF guard verdict. Pass `false` for an empty base
    /// URL: Python only guards a configured host (`cfg.base_url and
    /// ssrf.is_blocked(...)`).
    pub base_url_blocked: bool,
}

/// STT resolution failures, mirroring the Python exception order:
/// missing config, then blocked host, then undecryptable key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttError {
    ConfigMissing(String),
    BaseUrlBlocked(String),
    KeyUndecryptable(String),
}

/// Overlay seam for `ee/assistant/stt_provider.py`.
///
/// CE is bring-your-own endpoint: base URL + API key + model from the
/// user's own STT config. The SSRF guard is re-run at execution time
/// rather than trusting the save-time check, so the caller passes its
/// verdict in; DNS can be re-pointed at a private address after the URL
/// was stored.
pub trait SttSeam {
    /// Cheap presence check for request-time gating (rejecting a
    /// transcribe call before streaming a doomed upload). Must not decrypt
    /// anything.
    fn has_usable_stt_config(&self, has_api_key: bool) -> bool {
        has_api_key
    }

    fn resolve_stt_provider(&self, input: SttResolution) -> Result<ResolvedSttProvider, SttError> {
        if !input.has_api_key {
            return Err(SttError::ConfigMissing(
                "Configure dictation in Settings.".to_string(),
            ));
        }
        if input.base_url_blocked {
            return Err(SttError::BaseUrlBlocked(
                "That endpoint host is not allowed.".to_string(),
            ));
        }
        match input.api_key {
            Some(api_key) => Ok(ResolvedSttProvider {
                base_url: input.base_url,
                api_key,
                model: input.model,
            }),
            None => Err(SttError::KeyUndecryptable(
                "The stored dictation key could not be decrypted.".to_string(),
            )),
        }
    }
}

/// CE STT seam: the user's own endpoint.
pub struct ByoSttSeam;

impl SttSeam for ByoSttSeam {}

/// Overlay seam for `ee/cloud_agent/model_provider.py`.
///
/// The Cloud Agent has no platform model config of its own: every run
/// executes against its creator's LLM config — the same resolution path
/// Pi Dash AI uses, so the two always agree. `model_owner_for_run` pins
/// that routing decision; the model construction itself goes through the
/// [`AssistantModelSeam`] once the assistant runtime is ported (D-06/D-11).
pub trait CloudAgentModelSeam {
    /// Whose LLM config this run executes against. CE always answers the
    /// creator: `resolve_model_for_creator` delegates to
    /// `resolve_model_for_user(run.created_by)`.
    fn model_owner_for_run(&self, created_by: &str) -> String {
        created_by.to_string()
    }
}

/// CE cloud-agent model seam: the creator's config.
pub struct CreatorModelSeam;

impl CloudAgentModelSeam for CreatorModelSeam {}

/// A named extra toolset offered to a run beyond its immutable tool plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraToolset {
    pub name: String,
}

/// Overlay seam for `ee/cloud_agent/toolsets.py`.
///
/// A run's tools come from its immutable tool plan; deployments that can
/// offer tools unknowable at plan time (discovered per-user, at connect
/// time) attach them here instead — admitted only what the plan allows,
/// degrading (never failing) on outage, and reporting what was dropped.
pub trait CloudAgentToolsetsSeam {
    /// Whether the user opted in to the extra toolsets. Read once at run
    /// creation and snapshotted onto the plan, never consulted at
    /// execution time. CE: nobody has — the CE settings schema declares no
    /// such preference, so the argument is ignored and this is false.
    fn extra_toolsets_enabled(&self, user_opted_in: bool) -> bool {
        let _ = user_opted_in;
        false
    }

    /// Additional toolsets for the run. CE: none.
    fn extra_toolsets(&self) -> Vec<ExtraToolset> {
        Vec::new()
    }

    /// Name of the tool that fetches a deferred tool schema. CE: none, so
    /// the prompt section naming it never renders.
    fn schema_tool_name(&self) -> &'static str {
        ""
    }
}

/// CE cloud-agent toolsets: no extras.
pub struct NoExtraToolsets;

impl CloudAgentToolsetsSeam for NoExtraToolsets {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byok_presence_is_a_passthrough() {
        let seam = ByokAssistantSeam;
        assert!(seam.has_usable_llm_config(true));
        assert!(!seam.has_usable_llm_config(false));
    }

    #[test]
    fn ce_desktop_profile_is_never_available() {
        let seam = ByokAssistantSeam;
        let with_key = seam.agent_model_profile(true);
        assert!(!with_key.available);
        assert_eq!(with_key.lane, "byok");
        assert_eq!(with_key.reason_code, reason::BYOK_UNSUPPORTED);
        assert_eq!(reason::BYOK_UNSUPPORTED, "byok_not_supported_on_desktop");
        let without_key = seam.agent_model_profile(false);
        assert!(!without_key.available);
        assert!(without_key.lane.is_empty());
        assert_eq!(without_key.reason_code, reason::LLM_CONFIG_MISSING);
    }

    #[test]
    fn ce_hands_out_no_desktop_credential() {
        let error = ByokAssistantSeam
            .agent_model_credential()
            .expect_err("CE has no desktop lane");
        assert_eq!(error.reason, reason::BYOK_UNSUPPORTED);
    }

    #[test]
    fn model_label_is_empty_without_config() {
        let seam = ByokAssistantSeam;
        assert_eq!(seam.model_label(true, "gpt-x"), "gpt-x");
        assert!(seam.model_label(false, "gpt-x").is_empty());
    }

    fn stt_input() -> SttResolution {
        SttResolution {
            has_api_key: true,
            base_url: "https://stt.example.com".to_string(),
            model: "whisper-1".to_string(),
            api_key: Some("key".to_string()),
            base_url_blocked: false,
        }
    }

    #[test]
    fn stt_resolves_the_own_endpoint() {
        let resolved = ByoSttSeam
            .resolve_stt_provider(stt_input())
            .expect("resolve");
        assert_eq!(
            resolved,
            ResolvedSttProvider {
                base_url: "https://stt.example.com".to_string(),
                api_key: "key".to_string(),
                model: "whisper-1".to_string(),
            }
        );
        assert!(ByoSttSeam.has_usable_stt_config(true));
        assert!(!ByoSttSeam.has_usable_stt_config(false));
    }

    #[test]
    fn stt_failures_fire_in_python_order() {
        let missing = SttResolution {
            has_api_key: false,
            ..stt_input()
        };
        assert!(matches!(
            ByoSttSeam.resolve_stt_provider(missing),
            Err(SttError::ConfigMissing(_))
        ));
        let blocked = SttResolution {
            base_url_blocked: true,
            ..stt_input()
        };
        assert!(matches!(
            ByoSttSeam.resolve_stt_provider(blocked),
            Err(SttError::BaseUrlBlocked(_))
        ));
        let undecryptable = SttResolution {
            api_key: None,
            ..stt_input()
        };
        assert!(matches!(
            ByoSttSeam.resolve_stt_provider(undecryptable),
            Err(SttError::KeyUndecryptable(_))
        ));
    }

    #[test]
    fn cloud_agent_run_uses_its_creators_config() {
        assert_eq!(
            CreatorModelSeam.model_owner_for_run("user-1"),
            "user-1".to_string(),
        );
    }

    #[test]
    fn ce_offers_no_extra_toolsets() {
        let seam = NoExtraToolsets;
        assert!(!seam.extra_toolsets_enabled(true));
        assert!(!seam.extra_toolsets_enabled(false));
        assert!(seam.extra_toolsets().is_empty());
        assert_eq!(seam.schema_tool_name(), "");
    }

    /// A replacement seam behind the trait changes behaviour without
    /// touching callers — the overlay contract in miniature.
    struct OpenHubLikeSeam;

    impl AssistantModelSeam for OpenHubLikeSeam {
        fn agent_model_profile(&self, _has_api_key: bool) -> AgentModelProfile {
            AgentModelProfile {
                available: true,
                lane: "openhub".to_string(),
                base_url: "https://gateway.example.com".to_string(),
                model: "hub-default".to_string(),
                reason_code: String::new(),
            }
        }
    }

    impl CloudAgentToolsetsSeam for OpenHubLikeSeam {
        fn extra_toolsets_enabled(&self, user_opted_in: bool) -> bool {
            user_opted_in
        }

        fn extra_toolsets(&self) -> Vec<ExtraToolset> {
            vec![ExtraToolset {
                name: "gateway".to_string(),
            }]
        }
    }

    #[test]
    fn replacement_seam_wins_behind_the_traits() {
        let seam = OpenHubLikeSeam;
        let profile = seam.agent_model_profile(false);
        assert!(profile.available);
        assert!(profile.reason_code.is_empty());
        assert!(seam.extra_toolsets_enabled(true));
        assert_eq!(seam.extra_toolsets().len(), 1);
    }
}
