//! Per-surface assistant model choices (NEWPIDASH-392, OSS-1).
//!
//! Each user keeps one saved model choice per surface (`web_assistant`,
//! `desktop_assistant`, `agent`) in the existing JSONB bag on
//! `profiles.settings`, under the `assistant_models` namespace:
//!
//! ```json
//! {
//!   "assistant_models": {
//!     "web_assistant": { "model": "gpt-6", "connection_id": "6f1c…" },
//!     "agent":         { "model": "deepseek/deepseek-v4-pro" }
//!   }
//! }
//! ```
//!
//! The namespace stays undeclared in the settings schema on purpose, so
//! the generic profile PATCH refuses it (`unknown settings namespace`)
//! and the model-selection endpoint (the only writer) validates the
//! connection and the model. `connection_id` is always absent on OSS
//! (cloud: the connection serving the model; missing means the user's
//! OpenHub connection).
//!
//! This module owns the namespace spelling and the tolerant read; the
//! atomic single-surface writes live in
//! [`crate::domain::assistant`] (the ORM write-guard's domain rule),
//! and the choice resolution in the seam layer.

use serde_json::Value;
use uuid::Uuid;

use pidash_types::assistant::surface::Surface;

/// The `profiles.settings` namespace holding the per-surface choices.
pub const ASSISTANT_MODELS_NAMESPACE: &str = "assistant_models";

/// One saved per-surface choice: the model ID plus, on cloud, the
/// connection serving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub model: String,
    pub connection_id: Option<Uuid>,
}

impl ModelChoice {
    /// Render for the model-selection endpoint (`connection_id` omitted
    /// when absent, matching the stored shape).
    pub fn to_json(&self) -> Value {
        match self.connection_id {
            Some(id) => serde_json::json!({
                "model": self.model,
                "connection_id": id.to_string(),
            }),
            None => serde_json::json!({ "model": self.model }),
        }
    }
}

/// Strict parse of one stored entry: `model` must be a non-empty
/// string, `connection_id` (when present and non-null) a UUID string.
/// Anything else is not a choice — the caller falls back as if nothing
/// were saved. Stored entries are written only by the validated
/// endpoint, so a malformed entry is unreachable in practice.
pub fn parse_choice(value: &Value) -> Option<ModelChoice> {
    let entry = value.as_object()?;
    let model = entry.get("model")?.as_str()?;
    if model.is_empty() {
        return None;
    }
    let connection_id = match entry.get("connection_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.parse::<Uuid>().ok()?),
        Some(_) => return None,
    };
    Some(ModelChoice {
        model: model.to_owned(),
        connection_id,
    })
}

/// Read one surface's saved choice from the stored settings bag.
/// `None` covers a missing namespace, a missing surface and a malformed
/// entry alike.
pub fn read_entry(settings: &Value, surface: Surface) -> Option<ModelChoice> {
    settings
        .get(ASSISTANT_MODELS_NAMESPACE)?
        .get(surface.as_str())
        .and_then(parse_choice)
}

/// The raw saved entry JSON for one surface, when the namespace and the
/// surface both hold objects (even a malformed one). Resolution uses
/// this to tell "nothing saved" apart from "saved but invalid".
pub fn saved_entry_json(settings: &Value, surface: Surface) -> Option<&Value> {
    let namespace = settings.get(ASSISTANT_MODELS_NAMESPACE)?;
    if !namespace.is_object() {
        return None;
    }
    let entry = namespace.get(surface.as_str())?;
    entry.is_object().then_some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> Value {
        json!({
            "assistant_models": {
                "web_assistant": { "model": "gpt-6", "connection_id": "6f1c2b3a-9d4e-4f5a-8b6c-7d8e9f0a1b2c" },
                "agent": { "model": "deepseek/deepseek-v4-pro" },
                "desktop_assistant": { "model": "" },
            },
            "other": { "x": 1 },
        })
    }

    #[test]
    fn read_entry_parses_model_and_connection() {
        let choice = read_entry(&settings(), Surface::WebAssistant).expect("entry");
        assert_eq!(choice.model, "gpt-6");
        assert_eq!(
            choice.connection_id.expect("connection").to_string(),
            "6f1c2b3a-9d4e-4f5a-8b6c-7d8e9f0a1b2c"
        );
    }

    #[test]
    fn read_entry_without_connection_gives_none() {
        let choice = read_entry(&settings(), Surface::Agent).expect("entry");
        assert_eq!(choice.model, "deepseek/deepseek-v4-pro");
        assert_eq!(choice.connection_id, None);
    }

    #[test]
    fn read_entry_rejects_empty_model() {
        assert_eq!(read_entry(&settings(), Surface::DesktopAssistant), None);
    }

    #[test]
    fn read_entry_missing_namespace_or_surface_gives_none() {
        assert_eq!(read_entry(&json!({}), Surface::Agent), None);
        assert_eq!(
            read_entry(&json!({"assistant_models": {}}), Surface::Agent),
            None
        );
        assert_eq!(
            read_entry(&json!({"assistant_models": null}), Surface::Agent),
            None
        );
    }

    #[test]
    fn read_entry_rejects_bad_connection_id() {
        let settings = json!({
            "assistant_models": { "agent": { "model": "m", "connection_id": "nope" } },
        });
        assert_eq!(read_entry(&settings, Surface::Agent), None);
        let settings = json!({
            "assistant_models": { "agent": { "model": "m", "connection_id": 7 } },
        });
        assert_eq!(read_entry(&settings, Surface::Agent), None);
    }

    #[test]
    fn choice_renders_stored_shape() {
        let with = ModelChoice {
            model: "gpt-6".to_owned(),
            connection_id: Some(Uuid::nil()),
        };
        assert_eq!(
            with.to_json(),
            json!({
                "model": "gpt-6",
                "connection_id": Uuid::nil().to_string(),
            })
        );
        let without = ModelChoice {
            model: "m".to_owned(),
            connection_id: None,
        };
        assert_eq!(without.to_json(), json!({ "model": "m" }));
    }

    #[test]
    fn saved_entry_json_sees_malformed_entries() {
        let settings = settings();
        assert!(saved_entry_json(&settings, Surface::DesktopAssistant).is_some());
        assert!(saved_entry_json(&settings, Surface::WebAssistant).is_some());
        assert_eq!(saved_entry_json(&settings, Surface::Agent).is_some(), true);
        assert_eq!(saved_entry_json(&json!({}), Surface::Agent), None);
    }
}

// ---------------------------------------------------------------------------
// OSS resolution (NEWPIDASH-392 §5, OSS steps 1, 2 and 5)
// ---------------------------------------------------------------------------

/// Why step 1 (the saved per-surface entry) did not apply. The saved
/// entry is never rewritten by a fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// No entry saved: the legacy selection (step 2) applies.
    NoneSaved,
    /// The entry's model is not in the enabled list (OSS: the key's
    /// `models`).
    ModelRemoved,
    /// The entry names a connection that no longer exists (OSS: any
    /// `connection_id`, which OSS never writes).
    ConnectionRemoved,
    /// The lane is not usable (OSS: no key, or the agent surface, which
    /// has no lane).
    LaneUnavailable,
    /// The choice is not allowed for the surface (agent + BYOK/Codex).
    NotAllowedForSurface,
    /// The plan forbids the model (cloud free-plan rule; never emitted
    /// on OSS).
    PlanBlocked,
}

impl FallbackReason {
    /// The wire spelling in the model-selection responses.
    pub fn as_str(&self) -> &'static str {
        match self {
            FallbackReason::NoneSaved => "none_saved",
            FallbackReason::ModelRemoved => "model_removed",
            FallbackReason::ConnectionRemoved => "connection_removed",
            FallbackReason::LaneUnavailable => "lane_unavailable",
            FallbackReason::NotAllowedForSurface => "not_allowed_for_surface",
            FallbackReason::PlanBlocked => "plan_blocked",
        }
    }
}

/// One surface's resolution: the effective choice (if any) plus why
/// step 1 did not apply (unset when the saved entry applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub effective: Option<ModelChoice>,
    pub fallback_reason: Option<FallbackReason>,
}

/// OSS resolution over already-read inputs: the saved entry (if any),
/// the key's model list and whether the key is present. Steps 1, 2
/// and 5 only: a saved BYOK entry never applies to the agent surface
/// (D-12), and the legacy selection is `models[0]`. Pure, so the seam
/// and the turn path share it.
pub fn resolve_oss(
    saved: Option<&ModelChoice>,
    models: &[String],
    has_key: bool,
    surface: Surface,
) -> Resolution {
    let legacy = || {
        (has_key && !models.is_empty()).then(|| ModelChoice {
            model: models[0].clone(),
            connection_id: None,
        })
    };
    // The agent has no lane on OSS: BYOK is never served to it and there
    // is no catalog. The saved entry stands, the resolution is empty.
    if surface == Surface::Agent {
        return Resolution {
            effective: None,
            fallback_reason: Some(FallbackReason::LaneUnavailable),
        };
    }
    let Some(entry) = saved else {
        return Resolution {
            effective: legacy(),
            fallback_reason: Some(FallbackReason::NoneSaved),
        };
    };
    if entry.connection_id.is_some() {
        // OSS never writes a connection id; one present names a
        // connection that (here) does not exist.
        return Resolution {
            effective: legacy(),
            fallback_reason: Some(FallbackReason::ConnectionRemoved),
        };
    }
    if !has_key || models.is_empty() {
        return Resolution {
            effective: None,
            fallback_reason: Some(FallbackReason::LaneUnavailable),
        };
    }
    if models.iter().any(|model| model == &entry.model) {
        return Resolution {
            effective: Some(entry.clone()),
            fallback_reason: None,
        };
    }
    Resolution {
        effective: legacy(),
        fallback_reason: Some(FallbackReason::ModelRemoved),
    }
}

/// Model-selection validation failure (NEWPIDASH-392 §6.1): the 400
/// code plus a human detail. The endpoint renders `{error: code,
/// detail}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceValidation {
    pub code: &'static str,
    pub detail: String,
}

/// Operational failure resolving or validating a choice (no database
/// wired, or the read failed): the endpoint answers the generic 500.
/// Never a wrong choice — a seam without a database refuses loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOperational(pub String);

/// `validate_choice` outcome: the accepted choice, a 400 validation
/// failure, or an operational failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceError {
    Invalid(ChoiceValidation),
    Operational(ChoiceOperational),
}

/// Failure codes for the model-selection PUT.
pub mod choice_code {
    /// The path surface names nothing (`PUT .../{surface}/`).
    pub const UNKNOWN_SURFACE: &str = "unknown_surface";
    /// The model is not available to the user (not in the key's
    /// `models`, no key, or no usable lane).
    pub const MODEL_NOT_AVAILABLE: &str = "model_not_available";
    /// The connection does not exist (also for another user's
    /// connection; OSS has no connections at all).
    pub const CONNECTION_NOT_FOUND: &str = "connection_not_found";
    /// BYOK or Codex for the agent surface.
    pub const NOT_ALLOWED_FOR_SURFACE: &str = "not_allowed_for_surface";
    /// The plan forbids the model (cloud free-plan rule; OSS never
    /// emits it).
    pub const PLAN_BLOCKED: &str = "plan_blocked";
}

/// OSS validation of one PUT entry (NEWPIDASH-392 §6.1): the agent
/// surface takes no BYOK model, a connection id names nothing on OSS,
/// and the model must be in the key's list with a key present.
pub fn validate_oss_choice(
    entry: &ModelChoice,
    models: &[String],
    has_key: bool,
    surface: Surface,
) -> Result<ModelChoice, ChoiceValidation> {
    if surface == Surface::Agent {
        return Err(ChoiceValidation {
            code: choice_code::NOT_ALLOWED_FOR_SURFACE,
            detail: "The Pi Dash Agent runs on the OpenHub lane, not on BYOK models.".to_owned(),
        });
    }
    if entry.connection_id.is_some() {
        return Err(ChoiceValidation {
            code: choice_code::CONNECTION_NOT_FOUND,
            detail: "Unknown connection.".to_owned(),
        });
    }
    if entry.model.is_empty() {
        return Err(ChoiceValidation {
            code: choice_code::MODEL_NOT_AVAILABLE,
            detail: "A model is required.".to_owned(),
        });
    }
    if !has_key || !models.iter().any(|model| model == &entry.model) {
        return Err(ChoiceValidation {
            code: choice_code::MODEL_NOT_AVAILABLE,
            detail: "That model is not available.".to_owned(),
        });
    }
    Ok(entry.clone())
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    fn choice(model: &str) -> ModelChoice {
        ModelChoice {
            model: model.to_owned(),
            connection_id: None,
        }
    }

    fn keyed(models: &[&str]) -> (Vec<String>, bool) {
        (
            models.iter().map(|model| (*model).to_owned()).collect(),
            true,
        )
    }

    #[test]
    fn saved_entry_applies_without_reason() {
        let (models, key) = keyed(&["a", "b"]);
        let got = resolve_oss(Some(&choice("b")), &models, key, Surface::WebAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: Some(choice("b")),
                fallback_reason: None,
            }
        );
    }

    #[test]
    fn missing_entry_falls_back_to_legacy() {
        let (models, key) = keyed(&["a"]);
        let got = resolve_oss(None, &models, key, Surface::DesktopAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: Some(choice("a")),
                fallback_reason: Some(FallbackReason::NoneSaved),
            }
        );
    }

    #[test]
    fn missing_entry_without_lane_is_empty() {
        // The reason still names step 1's absence (`none_saved`); the
        // null effective choice says step 2 failed too.
        let got = resolve_oss(None, &[], false, Surface::WebAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: None,
                fallback_reason: Some(FallbackReason::NoneSaved),
            }
        );
    }

    #[test]
    fn removed_model_falls_back_with_reason() {
        let (models, key) = keyed(&["a"]);
        let got = resolve_oss(Some(&choice("gone")), &models, key, Surface::WebAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: Some(choice("a")),
                fallback_reason: Some(FallbackReason::ModelRemoved),
            }
        );
    }

    #[test]
    fn removed_model_without_legacy_is_empty() {
        let got = resolve_oss(Some(&choice("gone")), &[], false, Surface::WebAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: None,
                fallback_reason: Some(FallbackReason::LaneUnavailable),
            }
        );
    }

    #[test]
    fn connection_entry_is_removed() {
        let (models, key) = keyed(&["a"]);
        let entry = ModelChoice {
            model: "a".to_owned(),
            connection_id: Some(Uuid::nil()),
        };
        let got = resolve_oss(Some(&entry), &models, key, Surface::WebAssistant);
        assert_eq!(
            got,
            Resolution {
                effective: Some(choice("a")),
                fallback_reason: Some(FallbackReason::ConnectionRemoved),
            }
        );
    }

    #[test]
    fn agent_surface_never_resolves_on_oss() {
        let (models, key) = keyed(&["a"]);
        for saved in [None, Some(choice("a"))] {
            let got = resolve_oss(saved.as_ref(), &models, key, Surface::Agent);
            assert_eq!(
                got,
                Resolution {
                    effective: None,
                    fallback_reason: Some(FallbackReason::LaneUnavailable),
                }
            );
        }
    }

    #[test]
    fn empty_bags_resolve_exactly_like_today() {
        // No saved entry, legacy single model: step 2 returns `models[0]`,
        // which the writers keep equal to `model_name`.
        let (models, key) = keyed(&["m"]);
        let got = resolve_oss(None, &models, key, Surface::WebAssistant);
        assert_eq!(got.effective.expect("legacy").model, "m");
        let (models, key) = keyed(&["m"]);
        let got = resolve_oss(None, &models, key, Surface::DesktopAssistant);
        assert_eq!(got.effective.expect("legacy").model, "m");
    }

    #[test]
    fn validate_accepts_listed_model() {
        let (models, key) = keyed(&["a", "b"]);
        assert_eq!(
            validate_oss_choice(&choice("b"), &models, key, Surface::WebAssistant),
            Ok(choice("b"))
        );
    }

    #[test]
    fn validate_rejects_agent_surface() {
        let (models, key) = keyed(&["a"]);
        let err =
            validate_oss_choice(&choice("a"), &models, key, Surface::Agent).expect_err("agent");
        assert_eq!(err.code, choice_code::NOT_ALLOWED_FOR_SURFACE);
    }

    #[test]
    fn validate_rejects_connection_id() {
        let (models, key) = keyed(&["a"]);
        let entry = ModelChoice {
            model: "a".to_owned(),
            connection_id: Some(Uuid::nil()),
        };
        let err = validate_oss_choice(&entry, &models, key, Surface::WebAssistant)
            .expect_err("connection");
        assert_eq!(err.code, choice_code::CONNECTION_NOT_FOUND);
    }

    #[test]
    fn validate_rejects_unknown_and_keyless_models() {
        let (models, key) = keyed(&["a"]);
        let err = validate_oss_choice(&choice("nope"), &models, key, Surface::WebAssistant)
            .expect_err("unknown");
        assert_eq!(err.code, choice_code::MODEL_NOT_AVAILABLE);
        let err = validate_oss_choice(&choice("a"), &models, false, Surface::WebAssistant)
            .expect_err("keyless");
        assert_eq!(err.code, choice_code::MODEL_NOT_AVAILABLE);
        let err = validate_oss_choice(&choice(""), &models, key, Surface::WebAssistant)
            .expect_err("empty");
        assert_eq!(err.code, choice_code::MODEL_NOT_AVAILABLE);
    }

    #[test]
    fn fallback_reason_spellings_match_design() {
        assert_eq!(FallbackReason::NoneSaved.as_str(), "none_saved");
        assert_eq!(FallbackReason::ModelRemoved.as_str(), "model_removed");
        assert_eq!(
            FallbackReason::ConnectionRemoved.as_str(),
            "connection_removed"
        );
        assert_eq!(FallbackReason::LaneUnavailable.as_str(), "lane_unavailable");
        assert_eq!(
            FallbackReason::NotAllowedForSurface.as_str(),
            "not_allowed_for_surface"
        );
        assert_eq!(FallbackReason::PlanBlocked.as_str(), "plan_blocked");
    }
}
