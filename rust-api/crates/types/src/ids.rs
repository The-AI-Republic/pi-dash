//! Newtype identifiers shared by every layer.
//!
//! The inner value is the exact string the Django backend stores and returns,
//! so JSON output stays byte-for-byte identical with the Python side.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(WorkspaceId);
string_id!(ProjectId);
string_id!(IssueId);
string_id!(UserId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_json_as_plain_strings() {
        let id = IssueId::new("PIDASHCONV-27");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, r#""PIDASHCONV-27""#);
        let back: IssueId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn ids_display_the_inner_value() {
        assert_eq!(WorkspaceId::from("ws-1").to_string(), "ws-1");
        assert_eq!(ProjectId::from("p-2").to_string(), "p-2");
    }
}
