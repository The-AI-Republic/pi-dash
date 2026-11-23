//! Assistant surface: where a model choice is used (NEWPIDASH-392).
//!
//! The web assistant, the desktop assistant and the Pi Dash Agent each
//! keep their own saved model choice (`profiles.settings`
//! `assistant_models`, one entry per surface); turns are stamped with
//! the surface they ran on (`assistant_turn.surface`). The stored and
//! wire spelling is the snake_case string below, shared by the API,
//! jobs, services and the cloud crate through `pidash-sdk`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Where an assistant model choice applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    WebAssistant,
    DesktopAssistant,
    Agent,
}

impl Surface {
    /// The stored/wire spelling (`assistant_models` keys, `assistant_turn.surface`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Surface::WebAssistant => "web_assistant",
            Surface::DesktopAssistant => "desktop_assistant",
            Surface::Agent => "agent",
        }
    }

    /// Parse a stored/wire spelling; `None` is the `unknown_surface` arm
    /// (the model-selection endpoint answers 400 there).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "web_assistant" => Some(Surface::WebAssistant),
            "desktop_assistant" => Some(Surface::DesktopAssistant),
            "agent" => Some(Surface::Agent),
            _ => None,
        }
    }

    /// Every surface, in endpoint order (web, desktop, agent).
    pub fn all() -> [Surface; 3] {
        [
            Surface::WebAssistant,
            Surface::DesktopAssistant,
            Surface::Agent,
        ]
    }
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_round_trip() {
        for surface in Surface::all() {
            assert_eq!(Surface::parse(surface.as_str()), Some(surface));
            assert_eq!(surface.to_string(), surface.as_str());
        }
    }

    #[test]
    fn unknown_spelling_rejects() {
        for bad in ["", "web", "WEB_ASSISTANT", "web-assistant", "assistant"] {
            assert_eq!(Surface::parse(bad), None, "{bad} must not parse");
        }
    }

    #[test]
    fn serde_spelling_is_snake_case() {
        let json = serde_json::to_string(&Surface::DesktopAssistant).expect("serializes");
        assert_eq!(json, "\"desktop_assistant\"");
        assert_eq!(
            serde_json::from_str::<Surface>("\"agent\"").expect("deserializes"),
            Surface::Agent
        );
    }
}
