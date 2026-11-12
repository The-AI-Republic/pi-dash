//! OAuth error-code table + error-dict renderer (D-17 shapes layer).
//!
//! Port of `apps/api/pi_dash/authentication/adapter/error.py`:
//!
//! * [`OAUTH_ERROR_ROWS`] — the `# Oauth` block (`error.py:41-50`).
//! * [`AuthenticationException`] + [`AuthenticationException::get_error_dict`]
//!   (`error.py:77-92`).
//! * [`authentication_error_code`] — the provider selector
//!   (`authentication/adapter/oauth.py:49-59`).
//!
//! Fixture ids: AUTHOAUTH-F5 (`rust-api/fixtures/auth_oauth/F5_error_codes.golden.json`).
//!
//! Out of scope (other issues): the HTTP exchange that raises these errors
//! (`adapter/oauth.py` queries issue), the views that render them (handlers
//! issues).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-5: `AuthenticationException.__init__` takes a mutable default
//!   `payload={}` (`error.py:77`). Every default-constructed Python instance
//!   aliases the same dict object. The observable ctor + `get_error_dict`
//!   behavior is reproduced exactly here; the aliasing itself cannot exist
//!   in Rust — [`AuthenticationException`] owns its payload entries, so two
//!   default instances never share state. No caller in the ported sources
//!   mutates the default, so nothing observable is lost.

use serde_json::Value;

/// One `(name, code)` row of the `# Oauth` block (`error.py:41-50`).
///
/// Order is the file order, which the fixture pins deliberately: `5122`
/// (`GITHUB_USER_NOT_IN_ORG`) sits between `5110` and `5111`, not in numeric
/// position.
pub const OAUTH_ERROR_ROWS: [(&str, i32); 10] = [
    ("OAUTH_NOT_CONFIGURED", 5104),
    ("GOOGLE_NOT_CONFIGURED", 5105),
    ("GITHUB_NOT_CONFIGURED", 5110),
    ("GITHUB_USER_NOT_IN_ORG", 5122),
    ("GITLAB_NOT_CONFIGURED", 5111),
    ("GITEA_NOT_CONFIGURED", 5112),
    ("GOOGLE_OAUTH_PROVIDER_ERROR", 5115),
    ("GITHUB_OAUTH_PROVIDER_ERROR", 5120),
    ("GITLAB_OAUTH_PROVIDER_ERROR", 5121),
    ("GITEA_OAUTH_PROVIDER_ERROR", 5123),
];

/// Look up an oauth error name by code, or by name to code in reverse.
pub fn error_code_by_name(name: &str) -> Option<i32> {
    OAUTH_ERROR_ROWS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

pub fn error_name_by_code(code: i32) -> Option<&'static str> {
    OAUTH_ERROR_ROWS
        .iter()
        .find(|(_, c)| *c == code)
        .map(|(n, _)| *n)
}

/// Provider → `*_OAUTH_PROVIDER_ERROR` selector
/// (`adapter/oauth.py:49-59`): `google`/`github`/`gitlab`/`gitea` map to
/// their provider error, anything else falls back to `OAUTH_NOT_CONFIGURED`.
pub fn authentication_error_code(provider: &str) -> &'static str {
    match provider {
        "google" => "GOOGLE_OAUTH_PROVIDER_ERROR",
        "github" => "GITHUB_OAUTH_PROVIDER_ERROR",
        "gitlab" => "GITLAB_OAUTH_PROVIDER_ERROR",
        "gitea" => "GITEA_OAUTH_PROVIDER_ERROR",
        _ => "OAUTH_NOT_CONFIGURED",
    }
}

/// Provider → `*_NOT_CONFIGURED` name raised by that provider's constructor
/// when its client id/secret (and host, where applicable) are missing
/// (`provider/oauth/{google,github,gitlab,gitea}.py` `__init__` guards).
pub fn not_configured_name(provider: &str) -> Option<&'static str> {
    match provider {
        "google" => Some("GOOGLE_NOT_CONFIGURED"),
        "github" => Some("GITHUB_NOT_CONFIGURED"),
        "gitlab" => Some("GITLAB_NOT_CONFIGURED"),
        "gitea" => Some("GITEA_NOT_CONFIGURED"),
        _ => None,
    }
}

/// Port of `AuthenticationException` (`error.py:77-92`).
///
/// `payload` is an ordered entry list mirroring the Python dict's insertion
/// order (matters for byte-identical rendering; `serde_json::Map` without
/// `preserve_order` would sort keys, so the dict is not stored as a `Map`).
#[derive(Debug, Clone, Default)]
pub struct AuthenticationException {
    pub error_code: i32,
    pub error_message: String,
    pub payload: Vec<(String, Value)>,
}

impl AuthenticationException {
    pub fn new(
        error_code: i32,
        error_message: impl Into<String>,
        payload: Vec<(String, Value)>,
    ) -> Self {
        Self {
            error_code,
            error_message: error_message.into(),
            payload,
        }
    }

    /// `*_NOT_CONFIGURED` constructor shortcut for the provider `__init__`
    /// guards: looks up the code from [`OAUTH_ERROR_ROWS`].
    pub fn not_configured(provider: &str) -> Option<Self> {
        let name = not_configured_name(provider)?;
        Some(Self {
            error_code: error_code_by_name(name)?,
            error_message: name.to_owned(),
            payload: Vec::new(),
        })
    }

    /// Port of `get_error_dict` (`error.py:88-92`):
    /// `{"error_code", "error_message"}` first, then every payload entry in
    /// order, overwriting a base key on collision (`payload_keys_merge_over_base`).
    pub fn get_error_dict(&self) -> Vec<(String, Value)> {
        let mut entries: Vec<(String, Value)> = vec![
            ("error_code".to_owned(), Value::from(self.error_code)),
            (
                "error_message".to_owned(),
                Value::from(self.error_message.clone()),
            ),
        ];
        for (key, value) in &self.payload {
            match entries.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value.clone(),
                None => entries.push((key.clone(), value.clone())),
            }
        }
        entries
    }

    /// Byte-exact JSON rendering of [`Self::get_error_dict`] in Python key
    /// order (`{"error_code":…, "error_message":…[, payload…]}`).
    pub fn error_dict_json(&self) -> String {
        // Render from the merged entries (not base + tail): a payload key
        // colliding with a base key overwrites it in place.
        let dict = self.get_error_dict();
        let mut out = String::from("{");
        for (i, (key, value)) in dict.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&serde_json::to_string(key).expect("string serializes"));
            out.push(':');
            out.push_str(&serde_json::to_string(value).expect("value serializes"));
        }
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_oauth/F5_error_codes.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn oauth_rows_match_fixture_file_order() {
        let rows = fixture()["oauth_rows_in_file_order"]
            .as_array()
            .expect("rows array")
            .clone();
        assert_eq!(rows.len(), OAUTH_ERROR_ROWS.len(), "row count");
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(
                row["name"].as_str().unwrap(),
                OAUTH_ERROR_ROWS[i].0,
                "name {i}"
            );
            assert_eq!(
                row["code"].as_i64().unwrap(),
                i64::from(OAUTH_ERROR_ROWS[i].1),
                "code {i}"
            );
        }
        // The trap the order pins: 5122 precedes 5111/5112 in file order.
        assert_eq!(OAUTH_ERROR_ROWS[3], ("GITHUB_USER_NOT_IN_ORG", 5122));
        assert_eq!(OAUTH_ERROR_ROWS[4], ("GITLAB_NOT_CONFIGURED", 5111));
    }

    #[test]
    fn golden_error_dict_bytes() {
        let golden = &fixture()["exception"]["get_error_dict"]["golden_example"];
        let exc = AuthenticationException::new(
            golden["in"]["error_code"].as_i64().unwrap() as i32,
            golden["in"]["error_message"].as_str().unwrap(),
            Vec::new(),
        );
        let rendered = exc.error_dict_json();
        let expected = serde_json::to_string(&golden["out"]).expect("golden serializes");
        assert_eq!(rendered, expected);
        assert_eq!(
            rendered,
            r#"{"error_code":5105,"error_message":"GOOGLE_NOT_CONFIGURED"}"#
        );
    }

    #[test]
    fn payload_merges_over_base_in_order() {
        let exc = AuthenticationException::new(
            5105,
            "GOOGLE_NOT_CONFIGURED",
            vec![
                ("state".to_owned(), json!("STATE123")),
                // Collision overwrites the base value in place (dict update).
                ("error_message".to_owned(), json!("OVERRIDDEN")),
            ],
        );
        let dict = exc.get_error_dict();
        let keys: Vec<&str> = dict.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["error_code", "error_message", "state"]);
        assert_eq!(dict[1].1, json!("OVERRIDDEN"));
        assert_eq!(
            exc.error_dict_json(),
            r#"{"error_code":5105,"error_message":"OVERRIDDEN","state":"STATE123"}"#
        );
    }

    #[test]
    fn provider_error_selector_matches_fixture() {
        for provider in ["google", "github", "gitlab", "gitea"] {
            let name = authentication_error_code(provider);
            assert!(error_code_by_name(name).is_some(), "{provider}");
            assert!(name.ends_with("_OAUTH_PROVIDER_ERROR"));
        }
        assert_eq!(authentication_error_code("unknown"), "OAUTH_NOT_CONFIGURED");
        assert_eq!(error_code_by_name("OAUTH_NOT_CONFIGURED"), Some(5104));
    }

    #[test]
    fn not_configured_names_and_codes() {
        for (provider, name, code) in [
            ("google", "GOOGLE_NOT_CONFIGURED", 5105),
            ("github", "GITHUB_NOT_CONFIGURED", 5110),
            ("gitlab", "GITLAB_NOT_CONFIGURED", 5111),
            ("gitea", "GITEA_NOT_CONFIGURED", 5112),
        ] {
            assert_eq!(not_configured_name(provider), Some(name));
            let exc = AuthenticationException::not_configured(provider).unwrap();
            assert_eq!(exc.error_code, code);
            assert_eq!(exc.error_message, name);
        }
        assert_eq!(not_configured_name("unknown"), None);
    }
}
