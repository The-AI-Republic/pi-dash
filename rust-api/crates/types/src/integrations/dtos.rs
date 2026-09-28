//! Git provider data-transfer objects (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/dtos.py:1-118`: all nine
//! dataclasses (`ParsedRepository`, `ParsedCodeReview`, `RemoteRepository`,
//! `GitProviderCapabilities` including `as_dict`, `RemoteIssue`,
//! `RemoteComment`, `RemoteCodeReview`, `ProviderWebhookEvent`,
//! `RepositoryPage`).
//!
//! Layering notes:
//!
//! * Struct field declaration order is the Python `dataclass` field order,
//!   which is also the JSON key order on serialize (serde emits struct
//!   fields in declaration order regardless of the workspace `serde_json`
//!   `preserve_order` feature, which only affects maps).
//! * Datetimes cross this boundary as pre-rendered ISO-8601 strings in
//!   `Option<String>` fields (e.g. `"2024-01-02T03:04:05+00:00"`, the Python
//!   `datetime.isoformat()` form the adapters produce). Rendering and parsing
//!   are owned by the adapter layer (PIDASHCONV-141/143); this crate takes no
//!   `chrono` dependency, per the license-domain precedent.
//! * `metadata` / `*_ref` / `payload` (`dict[str, Any]` in Python, default
//!   `{}`) are [`serde_json::Value`]s defaulting to the empty object. An
//!   explicit `None` has no Python spelling here — the attribute always
//!   exists — so these fields are plain `Value`, never `Option`.
//! * `None` vs absent key (semantic trap): every `Option` field serializes
//!   present (`None` renders `null`, matching `dataclasses.asdict`), while
//!   deserialization treats an absent key as `None` (`#[serde(default)]`),
//!   matching Python's `field(default=None)` constructors. `RemoteCodeReview`
//!   deliberately has no `created_at`, mirroring `dtos.py:91-100`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default for `dict[str, Any]` fields: Python `field(default_factory=dict)`.
fn empty_object() -> Value {
    Value::Object(Default::default())
}

/// `ParsedRepository` (`dtos.py:12-19`, frozen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedRepository {
    pub provider: String,
    pub host_url: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
}

/// `ParsedCodeReview` (`dtos.py:22-29`, frozen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedCodeReview {
    pub provider: String,
    pub host_url: String,
    pub namespace: String,
    pub repo_name: String,
    pub external_iid: String,
    pub url: String,
}

/// `RemoteRepository` (`dtos.py:32-44`, mutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRepository {
    pub provider: String,
    pub external_id: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    #[serde(default)]
    pub clone_url_http: String,
    #[serde(default)]
    pub clone_url_ssh: String,
    #[serde(default)]
    pub default_branch: String,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default = "empty_object")]
    pub metadata: Value,
}

/// `GitProviderCapabilities` (`dtos.py:47-62`).
///
/// Every flag defaults to `False` in Python; `clone` is `False` for both
/// adapters today (fixture `capabilities.golden.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitProviderCapabilities {
    #[serde(default)]
    pub read_repositories: bool,
    #[serde(default)]
    pub read_issues: bool,
    #[serde(default)]
    pub write_comments: bool,
    #[serde(default)]
    pub manage_webhooks: bool,
    #[serde(default)]
    pub clone: bool,
}

impl GitProviderCapabilities {
    /// `as_dict` (`dtos.py:55-62`): exactly these five keys in this order.
    ///
    /// Returned as an array (not a map) so the order is structural: a
    /// `serde_json::Map` without the `preserve_order` feature would iterate
    /// alphabetically. JSON-object rendering is owned by the services layer.
    pub fn as_dict(&self) -> [(&'static str, bool); 5] {
        [
            ("read_repositories", self.read_repositories),
            ("read_issues", self.read_issues),
            ("write_comments", self.write_comments),
            ("manage_webhooks", self.manage_webhooks),
            ("clone", self.clone),
        ]
    }
}

/// `RemoteIssue` (`dtos.py:65-76`, mutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIssue {
    pub external_id: String,
    pub external_iid: String,
    pub title: String,
    pub body: String,
    pub state: String,
    pub author: String,
    pub web_url: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default = "empty_object")]
    pub metadata: Value,
}

/// `RemoteComment` (`dtos.py:79-87`, mutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteComment {
    pub external_id: String,
    pub body: String,
    pub author: String,
    #[serde(default)]
    pub web_url: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default = "empty_object")]
    pub metadata: Value,
}

/// `RemoteCodeReview` (`dtos.py:90-100`, mutable).
///
/// Note the asymmetry carried over from Python: there is `updated_at` but no
/// `created_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCodeReview {
    pub external_id: String,
    pub external_iid: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub web_url: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default = "empty_object")]
    pub metadata: Value,
}

/// `ProviderWebhookEvent` (`dtos.py:103-111`, mutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderWebhookEvent {
    pub provider: String,
    pub event: String,
    #[serde(default)]
    pub action: String,
    #[serde(default = "empty_object")]
    pub repository_ref: Value,
    #[serde(default = "empty_object")]
    pub code_review_ref: Value,
    #[serde(default = "empty_object")]
    pub issue_ref: Value,
    #[serde(default = "empty_object")]
    pub payload: Value,
}

/// `RepositoryPage` (`dtos.py:114-118`, mutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryPage {
    pub repositories: Vec<RemoteRepository>,
    pub page: i64,
    pub has_next_page: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn golden(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/dtos/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Top-level JSON key order of a struct's serialization.
    ///
    /// Read off the serialized string, not a `serde_json::Value`: the
    /// workspace `serde_json` has no `preserve_order` feature, so `Value`
    /// objects iterate alphabetically while struct serialization always
    /// emits declaration order.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                '"' if depth == 1 => {
                    let mut key = String::new();
                    let inner = chars.by_ref();
                    while let Some(ch) = inner.next() {
                        if ch == '\\' {
                            if let Some(e) = inner.next() {
                                key.push(e);
                            }
                        } else if ch == '"' {
                            break;
                        } else {
                            key.push(ch);
                        }
                    }
                    if chars.peek() == Some(&':') {
                        chars.next();
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    fn fixture_fields(name: &str) -> Vec<String> {
        golden(name)
            .get("fields")
            .expect("golden has fields")
            .as_array()
            .expect("fields array")
            .iter()
            .map(|v| v.as_str().expect("field name").to_string())
            .collect()
    }

    fn example_repository() -> RemoteRepository {
        RemoteRepository {
            provider: "github".into(),
            external_id: "123".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            web_url: "https://github.com/acme/web".into(),
            clone_url_http: "https://github.com/acme/web.git".into(),
            clone_url_ssh: "git@github.com:acme/web.git".into(),
            default_branch: "main".into(),
            is_private: true,
            metadata: json!({"id": 123}),
        }
    }

    #[test]
    fn parsed_repository_field_order_matches_fixture() {
        let parsed = ParsedRepository {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/Acme/Web.git".into(),
        };
        let rendered = serde_json::to_string(&parsed).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"provider":"github","host_url":"https://github.com","namespace":"acme","name":"web","full_name":"acme/web","clone_url":"https://github.com/Acme/Web.git"}"#
        );
        assert_eq!(
            serialized_keys(&parsed),
            fixture_fields("parsed_repository.golden.json")
        );
    }

    #[test]
    fn parsed_code_review_field_order_matches_fixture() {
        let parsed = ParsedCodeReview {
            provider: "github".into(),
            host_url: "https://github.com".into(),
            namespace: "acme".into(),
            repo_name: "web".into(),
            external_iid: "42".into(),
            url: "https://github.com/acme/web/pull/42".into(),
        };
        let rendered = serde_json::to_string(&parsed).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"provider":"github","host_url":"https://github.com","namespace":"acme","repo_name":"web","external_iid":"42","url":"https://github.com/acme/web/pull/42"}"#
        );
        assert_eq!(
            serialized_keys(&parsed),
            fixture_fields("parsed_code_review.golden.json")
        );
    }

    #[test]
    fn capabilities_defaults_and_as_dict_order_match_fixture() {
        let golden_caps = golden("capabilities.golden.json");
        let defaults = golden_caps.get("defaults").expect("defaults");
        let replayed = serde_json::to_value(GitProviderCapabilities {
            read_repositories: false,
            read_issues: false,
            write_comments: false,
            manage_webhooks: false,
            clone: false,
        })
        .expect("value");
        assert_eq!(&replayed, defaults);

        let keys: Vec<&str> = GitProviderCapabilities {
            read_repositories: true,
            read_issues: true,
            write_comments: true,
            manage_webhooks: true,
            clone: false,
        }
        .as_dict()
        .iter()
        .map(|(k, _)| *k)
        .collect();
        let expected: Vec<&str> = golden_caps
            .get("as_dict_keys")
            .expect("as_dict_keys")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("key"))
            .collect();
        assert_eq!(keys, expected);

        // Struct serialization emits the same five keys in the same order.
        let rendered = serde_json::to_string(&GitProviderCapabilities {
            read_repositories: true,
            read_issues: true,
            write_comments: true,
            manage_webhooks: true,
            clone: false,
        })
        .expect("serializes");
        assert_eq!(
            rendered,
            r#"{"read_repositories":true,"read_issues":true,"write_comments":true,"manage_webhooks":true,"clone":false}"#
        );
    }

    #[test]
    fn remote_repository_field_order_and_defaults_match_fixture() {
        let repo = example_repository();
        assert_eq!(
            serialized_keys(&repo),
            fixture_fields("remote_repository.golden.json")
        );
        let rendered = serde_json::to_string(&repo).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"provider":"github","external_id":"123","namespace":"acme","name":"web","full_name":"acme/web","web_url":"https://github.com/acme/web","clone_url_http":"https://github.com/acme/web.git","clone_url_ssh":"git@github.com:acme/web.git","default_branch":"main","is_private":true,"metadata":{"id":123}}"#
        );

        // Python defaults ("", "", "", False, {}) apply on absent keys.
        let minimal: RemoteRepository = serde_json::from_value(json!({
            "provider": "github",
            "external_id": "123",
            "namespace": "acme",
            "name": "web",
            "full_name": "acme/web",
            "web_url": "https://github.com/acme/web"
        }))
        .expect("deserializes");
        assert_eq!(minimal.clone_url_http, "");
        assert_eq!(minimal.clone_url_ssh, "");
        assert_eq!(minimal.default_branch, "");
        assert!(!minimal.is_private);
        assert_eq!(minimal.metadata, json!({}));
    }

    #[test]
    fn remote_issue_none_renders_null_and_absent_reads_none() {
        let mut issue = RemoteIssue {
            external_id: "1001".into(),
            external_iid: "7".into(),
            title: "Upstream title".into(),
            body: "Upstream **body**".into(),
            state: "open".into(),
            author: "octo".into(),
            web_url: "https://github.com/acme/web/issues/7".into(),
            created_at: None,
            updated_at: None,
            metadata: json!({}),
        };
        // None renders as a present null (dataclass attribute always exists).
        assert_eq!(
            serialized_keys(&issue),
            fixture_fields("remote_issue.golden.json")
        );
        let value = serde_json::to_value(&issue).expect("value");
        assert_eq!(value.get("created_at"), Some(&Value::Null));
        assert_eq!(value.get("updated_at"), Some(&Value::Null));

        // Absent and explicit-null keys both read back as None.
        let absent: RemoteIssue = serde_json::from_value(json!({
            "external_id": "1001", "external_iid": "7", "title": "t",
            "body": "b", "state": "open", "author": "octo",
            "web_url": "https://github.com/acme/web/issues/7"
        }))
        .expect("absent datetimes deserialize");
        assert_eq!(absent.created_at, None);
        assert_eq!(absent.updated_at, None);
        assert_eq!(absent.metadata, json!({}));

        issue.created_at = Some("2024-01-02T03:04:05+00:00".into());
        issue.updated_at = Some("2024-01-02T04:00:00+00:00".into());
        let rendered = serde_json::to_string(&issue).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"external_id":"1001","external_iid":"7","title":"Upstream title","body":"Upstream **body**","state":"open","author":"octo","web_url":"https://github.com/acme/web/issues/7","created_at":"2024-01-02T03:04:05+00:00","updated_at":"2024-01-02T04:00:00+00:00","metadata":{}}"#
        );
    }

    #[test]
    fn remote_comment_web_url_default_and_field_order_match_fixture() {
        let comment = RemoteComment {
            external_id: "2002".into(),
            body: "Nice fix".into(),
            author: "octo".into(),
            web_url: "".into(),
            created_at: Some("2024-01-02T03:04:05+00:00".into()),
            updated_at: Some("2024-01-02T03:05:00+00:00".into()),
            metadata: json!({}),
        };
        assert_eq!(
            serialized_keys(&comment),
            fixture_fields("remote_comment.golden.json")
        );

        // web_url defaults to "" and an absent key reads back as "".
        let minimal: RemoteComment = serde_json::from_value(json!({
            "external_id": "2002", "body": "Nice fix", "author": "octo"
        }))
        .expect("deserializes");
        assert_eq!(minimal.web_url, "");
        assert_eq!(minimal.created_at, None);
    }

    #[test]
    fn remote_code_review_has_no_created_at_matching_fixture() {
        let review = RemoteCodeReview {
            external_id: "3003".into(),
            external_iid: "42".into(),
            title: "Add widget".into(),
            state: "open".into(),
            merged: false,
            draft: false,
            web_url: "https://github.com/acme/web/pull/42".into(),
            updated_at: Some("2024-01-02T03:04:05+00:00".into()),
            metadata: json!({}),
        };
        assert_eq!(
            serialized_keys(&review),
            fixture_fields("remote_code_review.golden.json")
        );
        let value = serde_json::to_value(&review).expect("value");
        assert!(!value
            .as_object()
            .expect("object")
            .contains_key("created_at"));
    }

    #[test]
    fn webhook_event_defaults_and_field_order_match_fixture() {
        let event = ProviderWebhookEvent {
            provider: "github".into(),
            event: "pull_request".into(),
            action: "opened".into(),
            repository_ref: json!({}),
            code_review_ref: json!({}),
            issue_ref: json!({}),
            payload: json!({"action": "opened"}),
        };
        assert_eq!(
            serialized_keys(&event),
            fixture_fields("webhook_event.golden.json")
        );

        let golden_event = golden("webhook_event.golden.json");
        let defaults = golden_event.get("defaults").expect("defaults");
        let bare = serde_json::to_value(ProviderWebhookEvent {
            provider: "gitlab".into(),
            event: "Issue Hook".into(),
            action: String::new(),
            repository_ref: json!({}),
            code_review_ref: json!({}),
            issue_ref: json!({}),
            payload: json!({}),
        })
        .expect("value");
        for (key, expected) in defaults.as_object().expect("defaults object") {
            if key == "provider" || key == "event" {
                continue;
            }
            assert_eq!(bare.get(key), Some(expected), "default for {key}");
        }
    }

    #[test]
    fn repository_page_field_order_and_nesting_match_fixture() {
        let page = RepositoryPage {
            repositories: vec![example_repository()],
            page: 1,
            has_next_page: false,
        };
        assert_eq!(
            serialized_keys(&page),
            fixture_fields("repository_page.golden.json")
        );
        let rendered = serde_json::to_string(&page).expect("serializes");
        assert!(
            rendered.starts_with(r#"{"repositories":[{"provider":"github","external_id":"123""#)
        );
        assert!(rendered.ends_with(r#""page":1,"has_next_page":false}"#));
        let round_trip: RepositoryPage = serde_json::from_str(&rendered).expect("round-trips");
        assert_eq!(round_trip, page);
    }
}
