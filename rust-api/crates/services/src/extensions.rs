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
//! ports (D-06, D-11) and extend these same traits; the cloud-agent
//! toolsets seam below already carries that runtime extension
//! (run/user-aware async methods), while the model seams stay on the
//! portable decision surface, fully tested, with no placeholders.
//!
//! Explicit map for the private crate:
//!
//! | Seam trait | Python source | CE default | Overlay replaces with |
//! |---|---|---|
//! | `AssistantModelSeam` | `ee/assistant/model_provider.py` | api `CeAssistantSeam` | platform credentials (OpenHub lane) |
//! | `SttSeam` | `ee/assistant/stt_provider.py` | `ByoSttSeam` | OpenHub relay endpoint |
//! | `DesktopGate` (in `pidash-auth`) | `ee/authentication/desktop.py` | `SessionDesktopGate` | OIDC `client` claim |
//! | `UserSettingsSchema` (in `user_settings`) | `ee/settings/user_settings.py` | `CeUserSettings` | extended schema |

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

use pidash_types::assistant::errors::AssistantError;
use uuid::Uuid;

/// A seam future: boxed so the trait stays object-safe behind the
/// `Arc<dyn AssistantModelSeam>` the serving state carries (the jobs
/// registry's `Pin<Box<dyn Future>>` precedent — no new dependency).
pub type SeamFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// Reason codes from `managed_runner/errors.py::ManagedRunnerReason`,
/// in the values the desktop profile endpoint actually serialises.
pub mod reason {
    /// The viewer has no usable LLM configuration at all.
    pub const LLM_CONFIG_MISSING: &str = "llm_config_missing";
    /// The viewer's provider is BYOK, which the desktop engine does not serve.
    pub const BYOK_UNSUPPORTED: &str = "byok_not_supported_on_desktop";
}

/// How the desktop agent engine should reach a model for one user
/// (`ee/assistant/model_provider.py::PidashAgentProfile`).
///
/// The credential itself is never part of this object — the desktop is
/// handed it separately through the agent-token endpoint. `reason_code` is
/// empty exactly when `available` is true. `top_up_url` is where a user
/// adds credit for this lane; the overlay fills it, CE leaves it empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidashAgentProfile {
    pub available: bool,
    pub lane: String,
    pub base_url: String,
    pub model: String,
    pub reason_code: String,
    pub top_up_url: String,
}

/// CE credential failure (`ManagedRunnerUnavailable`): raising rather than
/// returning empty keeps the failure loud — a caller that ignored the
/// profile must not silently receive a blank credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialUnavailable {
    pub reason: String,
    pub message: String,
}

/// Desktop-credential failure: the two `AgentModelTokenEndpoint.post`
/// error arms (`agent_profile.py:87-99`). `Unavailable` is the
/// `ManagedRunnerUnavailable` arm (the client should have consulted the
/// profile first); `Classified` is the `_classify_credential_error` arm —
/// the cloud seam's pre-classified gateway outcome. CE only ever produces
/// `Unavailable`. `WalletEmpty` is the built-in engine's model-token
/// classification (§8, `runner/views/engine.py:_classify_gateway_error`):
/// the gateway's wallet ran out, so the engine pauses with
/// `wallet_empty` (402). The legacy agent-token endpoint renders it as
/// the transient arm it always rendered wallet failures as
/// (`gateway_unavailable`, 503), so its response is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// Profile-refused: renders `409 {error: reason, detail}`.
    Unavailable(CredentialUnavailable),
    /// Classified gateway failure: renders `{error: code}` with NO
    /// `detail` key at `status` (401 `gateway_session_revoked` or 503
    /// `gateway_unavailable`).
    Classified { code: String, status: u16 },
    /// Wallet-exhausted gateway failure (402 on the engine model-token
    /// endpoint; `detail` is the classifier's exception name, never a
    /// provider body). The legacy agent-token endpoint maps it to its
    /// transient arm.
    WalletEmpty { detail: String },
}

/// BYOK title inputs for [`TitleRequest`]: the caller's read of the user's
/// config row (`None` at the request level when there is no row). Data,
/// not handles — the caller extracts these exactly like the presence bits
/// the other methods take.
#[derive(Debug, Clone)]
pub struct ByokTitleInputs {
    pub model_name: String,
    pub provider_kind: String,
    pub base_url: String,
    /// The caller's execution-time SSRF verdict on `base_url`.
    pub base_url_blocked: bool,
    /// Raw `api_key_encrypted` bytes; the implementor decrypts.
    pub api_key_encrypted: Option<Vec<u8>>,
    /// `cfg is not None and cfg.has_api_key`.
    pub has_api_key: bool,
}

/// One title-generation call (`generate_title_for_user`, `ee` seam): who
/// it is for, what to title, and the BYOK row the CE path runs on.
#[derive(Debug, Clone)]
pub struct TitleRequest {
    pub user_id: Uuid,
    pub description: String,
    /// `None` when the user holds no BYOK config row. CE answers
    /// `llm_config_missing` then; the overlay may route elsewhere.
    pub byok: Option<ByokTitleInputs>,
}

/// Title-generation failure: an assistant error rendered with its own
/// code/detail/status, or an operational failure answering the generic
/// 500 (the `crypto_failure` split: data problems render, operational
/// problems propagate). Provider HTTP failures arrive as
/// [`AssistantError::ProviderUnreachable`], which renders byte-identical
/// to the view-inline 502.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    Assistant(AssistantError),
    ServerError,
}

/// Overlay seam for `ee/assistant/model_provider.py`.
///
/// CE is BYOK-only: every user brings their own LLM key, and the desktop
/// engine deliberately does not serve that lane (the stored key is
/// decrypted only inside a server process at the moment of use, so
/// honouring BYOK on the desktop would mean adding the first endpoint that
/// returns a stored key to a client).
///
/// The serving state carries one of these (`Arc<dyn AssistantModelSeam>`)
/// and the non-turn HTTP callers consult it instead of the CE-concrete
/// free functions, so the private crate implements this trait once and
/// every caller follows. (P4: the execute-path `resolve_model_for_user`
/// died with the retired run plane; the chat free function
/// [`crate::assistant::seams::resolve_model_for_user`] stays.)
/// Turn-time model/toolset/label routing is NOT
/// here — it stays with the jobs `TurnSeam`, which already covers it.
/// Methods are async with the user id because the overlay branches on the
/// user's selected connection, which it reads itself; the CE defaults
/// ignore the id and delegate to the ported
/// [`crate::assistant::seams`] logic, so there is exactly one CE
/// implementation. Title generation has no default: it needs the serving
/// boundary's transport, which lives in the api crate's `CeAssistantSeam`
/// (the `title` layering rule), so every implementor spells it out and
/// the compiler refuses a silent 502.
pub trait AssistantModelSeam: Send + Sync + std::fmt::Debug {
    /// Cheap presence check for request-time gating. Must not build a
    /// model or decrypt anything; `has_api_key` is the caller's read of
    /// the user's config.
    fn has_usable_llm_config(&self, user_id: Uuid, has_api_key: bool) -> SeamFuture<'_, bool> {
        let _ = user_id;
        Box::pin(async move { crate::assistant::seams::has_usable_llm_config(has_api_key) })
    }


    /// Describe the model endpoint the desktop engine may call. CE has
    /// exactly one lane, BYOK, which the desktop does not serve — so this
    /// is always unavailable; only the reason differs.
    fn pidash_agent_profile(
        &self,
        user_id: Uuid,
        has_api_key: bool,
    ) -> SeamFuture<'_, PidashAgentProfile> {
        let _ = user_id;
        Box::pin(async move {
            let profile = crate::assistant::seams::pidash_agent_profile_for_user(has_api_key);
            PidashAgentProfile {
                available: profile.available,
                lane: profile.lane,
                base_url: profile.base_url,
                model: profile.model,
                reason_code: profile.reason_code,
                top_up_url: profile.top_up_url,
            }
        })
    }

    /// Hand out the desktop engine's model credential for one user. CE has
    /// no lane the desktop can use, so there is nothing to hand out —
    /// always [`CredentialError::Unavailable`]. The overlay mints the
    /// user's own short-lived gateway credential here, answering
    /// [`CredentialError::Classified`] (its `_classify_credential_error`
    /// outcome) when the gateway fetch fails.
    fn pidash_agent_credential(
        &self,
        user_id: Uuid,
    ) -> SeamFuture<'_, Result<(String, String), CredentialError>> {
        let _ = user_id;
        Box::pin(async move {
            crate::assistant::seams::pidash_agent_credential_for_user().map_err(|unavailable| {
                CredentialError::Unavailable(CredentialUnavailable {
                    reason: unavailable.code,
                    message: unavailable.detail,
                })
            })
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

    /// Single-prompt title for `request.description`, from the same
    /// provider the user's conversation would use. `Ok` carries the raw
    /// title (possibly empty — the caller maps empty to
    /// `generation_failed`, mirroring the view's `if not title`).
    fn generate_title(&self, request: TitleRequest) -> SeamFuture<'_, Result<String, TitleError>>;
}

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
#[cfg(test)]
mod tests {
    use super::*;

    /// The trait defaults with a canned title: the CE behaviour lives in
    /// the defaults (the api crate's `CeAssistantSeam` only adds the
    /// title transport), so this pins the default surface.
    #[derive(Debug)]
    struct TestSeam;

    impl AssistantModelSeam for TestSeam {
        fn generate_title(
            &self,
            request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            let _ = request;
            Box::pin(async move { Ok("canned".to_owned()) })
        }
    }

    fn user() -> Uuid {
        Uuid::nil()
    }

    #[tokio::test]
    async fn byok_presence_is_a_passthrough() {
        let seam = TestSeam;
        assert!(seam.has_usable_llm_config(user(), true).await);
        assert!(!seam.has_usable_llm_config(user(), false).await);
    }

    #[tokio::test]
    async fn ce_desktop_profile_is_never_available() {
        let seam = TestSeam;
        let with_key = seam.pidash_agent_profile(user(), true).await;
        assert!(!with_key.available);
        assert_eq!(with_key.lane, "byok");
        assert_eq!(with_key.base_url, "");
        assert_eq!(with_key.model, "");
        assert_eq!(with_key.reason_code, reason::BYOK_UNSUPPORTED);
        assert_eq!(reason::BYOK_UNSUPPORTED, "byok_not_supported_on_desktop");
        let without_key = seam.pidash_agent_profile(user(), false).await;
        assert!(!without_key.available);
        assert!(without_key.lane.is_empty());
        assert_eq!(without_key.reason_code, reason::LLM_CONFIG_MISSING);
        assert_eq!(reason::LLM_CONFIG_MISSING, "llm_config_missing");
        assert!(with_key.top_up_url.is_empty());
        assert!(without_key.top_up_url.is_empty());
    }

    #[tokio::test]
    async fn ce_hands_out_no_desktop_credential() {
        let error = TestSeam
            .pidash_agent_credential(user())
            .await
            .expect_err("CE has no desktop lane");
        let CredentialError::Unavailable(unavailable) = &error else {
            panic!("CE refuses via the profile arm, got {error:?}");
        };
        assert_eq!(unavailable.reason, reason::BYOK_UNSUPPORTED);
        assert_eq!(
            unavailable.message,
            "This build has no model lane the desktop agent can use."
        );
    }

    #[test]
    fn model_label_is_empty_without_config() {
        let seam = TestSeam;
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

    /// A replacement seam behind the trait changes behaviour without
    /// touching callers — the overlay contract in miniature.
    #[derive(Debug)]
    struct OpenHubLikeSeam;

    impl AssistantModelSeam for OpenHubLikeSeam {
        fn has_usable_llm_config(&self, user_id: Uuid, has_api_key: bool) -> SeamFuture<'_, bool> {
            let _ = (user_id, has_api_key);
            // The gateway lane counts as configured without a BYOK key.
            Box::pin(async move { true })
        }

        fn pidash_agent_profile(
            &self,
            user_id: Uuid,
            has_api_key: bool,
        ) -> SeamFuture<'_, PidashAgentProfile> {
            let _ = (user_id, has_api_key);
            Box::pin(async move {
                PidashAgentProfile {
                    available: true,
                    lane: "openhub".to_string(),
                    base_url: "https://gateway.example.com".to_string(),
                    model: "hub-default".to_string(),
                    reason_code: String::new(),
                    top_up_url: "https://gateway.example.com/account/credits".to_string(),
                }
            })
        }

        fn pidash_agent_credential(
            &self,
            user_id: Uuid,
        ) -> SeamFuture<'_, Result<(String, String), CredentialError>> {
            let _ = user_id;
            Box::pin(async move {
                Ok((
                    "gateway-token".to_string(),
                    "2030-01-01T00:00:00Z".to_string(),
                ))
            })
        }

        fn generate_title(
            &self,
            request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            let _ = request;
            Box::pin(async move { Ok("cloud title".to_string()) })
        }
    }

    #[tokio::test]
    async fn replacement_seam_wins_behind_the_traits() {
        let seam = OpenHubLikeSeam;
        // Keyless yet usable: the gateway lane, not BYOK.
        assert!(seam.has_usable_llm_config(user(), false).await);
        let profile = seam.pidash_agent_profile(user(), false).await;
        assert!(profile.available);
        assert_eq!(profile.lane, "openhub");
        assert_eq!(profile.base_url, "https://gateway.example.com");
        assert_eq!(profile.model, "hub-default");
        assert!(profile.reason_code.is_empty());
        assert_eq!(
            profile.top_up_url,
            "https://gateway.example.com/account/credits"
        );
        let (token, expires_at) = seam
            .pidash_agent_credential(user())
            .await
            .expect("gateway credential");
        assert_eq!(token, "gateway-token");
        assert_eq!(expires_at, "2030-01-01T00:00:00Z");
        fn title_request() -> TitleRequest {
            TitleRequest {
                user_id: user(),
                description: "Fix the thing".to_string(),
                byok: None,
            }
        }
        let title = seam
            .generate_title(title_request())
            .await
            .expect("cloud title");
        assert_eq!(title, "cloud title");
        // The toolsets seam died in P4; model/credential/title survive.
    }
}
