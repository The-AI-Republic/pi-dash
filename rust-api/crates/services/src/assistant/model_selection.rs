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
