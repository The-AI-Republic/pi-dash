//! D-08 logging decode + event role preprocessing (services layer).
//!
//! Port of the pure helpers in `apps/api/pi_dash/bgtasks/logger_task.py`
//! (`:38-:60`, `safe_decode_body`) and
//! `apps/api/pi_dash/bgtasks/event_tracking_task.py` (`:43-:59`,
//! `preprocess_data_properties`). The DB-backed halves (mongo/postgres
//! writes, the `Workspace` lookup, the PostHog POST) live in `pidash-jobs`
//! (`tasks_webhooks::sinks`); this module is pure over injected values so
//! the fixture goldens pin it without a database.
//!
//! Fixture oracle: `rust-api/fixtures/tasks_webhooks/fx-log-01-process-logs.json`
//! (`safe_decode_body` vectors) and
//! `rust-api/fixtures/tasks_webhooks/fx-evt-01-track-event.json`
//! (role matrix). Trace: `rust-api/fixtures/tasks_webhooks/TRACE.md`.
//!
//! The same `safe_decode_body` shape already exists in `pidash-api`
//! (`middleware::logging::safe_decode_body`); the copy here is
//! intentional — foundation crates are not refactored by port agents —
//! and the unit tests pin both against the same fixture vectors.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * Python's empty check is `content == b""`, so `bytearray(b"")` does
//!   NOT match (never equal across types) and falls through to decoding.
//!   Rust has no bytes/bytearray distinction (`&[u8]` is `&[u8]`), so an
//!   empty slice always maps to `None`; the quirk is unrepresentable and
//!   is recorded here instead of ported.
//! * Only `UnicodeDecodeError` is caught: any other decode-time failure
//!   (e.g. a non-bytes argument raising `TypeError`) propagates. The Rust
//!   signature takes `&[u8]`, so that path cannot arise; it is recorded
//!   here instead of ported.
//! * Role `admin` is assigned to ANY non-owner, including users with no
//!   workspace membership at all — there is no membership check in
//!   Python (`event_tracking_task.py:51-54`). Kept as-is.

use serde_json::{Map, Value};

/// `USER_INVITED_TO_WORKSPACE` (`utils/analytics_events.py:7`).
pub const USER_INVITED_TO_WORKSPACE: &str = "user_invited_to_workspace";
/// `WORKSPACE_DELETED` (`utils/analytics_events.py:8`).
pub const WORKSPACE_DELETED: &str = "workspace_deleted";

/// True for the two events that carry a workspace role
/// (`event_tracking_task.py:46`).
pub fn is_role_event(event_name: &str) -> bool {
    event_name == USER_INVITED_TO_WORKSPACE || event_name == WORKSPACE_DELETED
}

/// `safe_decode_body` (`logger_task.py:38-60`), verbatim: `None` for
/// `None`/empty, `"[Binary Content]"` when the bytes start with a
/// PNG/JPEG/PDF magic prefix (prefix only — a magic sequence elsewhere
/// does not match), else UTF-8 or `"[Could not decode content]"`.
pub fn safe_decode_body(content: Option<&[u8]>) -> Option<String> {
    let content = content?;
    if content.is_empty() {
        return None;
    }
    if content.starts_with(b"\x89PNG")
        || content.starts_with(b"\xff\xd8\xff")
        || content.starts_with(b"%PDF")
    {
        return Some("[Binary Content]".to_owned());
    }
    match std::str::from_utf8(content) {
        Ok(text) => Some(text.to_owned()),
        Err(_) => Some("[Could not decode content]".to_owned()),
    }
}

/// The workspace role `preprocess_data_properties` stamps onto the event
/// properties (`event_tracking_task.py:48-56`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `str(workspace.owner_id) == str(user_id)`.
    Owner,
    /// Workspace found, any other user (no membership check — see above).
    Admin,
    /// `Workspace.DoesNotExist` (the warning + `"unknown"` branch).
    Unknown,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Unknown => "unknown",
        }
    }
}

/// Resolve the role decision without touching the database:
/// `owner_match` is `None` when the workspace lookup missed
/// (`DoesNotExist`), else whether `str(owner_id) == str(user_id)`.
/// Returns `None` for events outside the pair — Python returns the
/// properties dict untouched, with no role key and no workspace lookup
/// (`event_tracking_task.py:46` guard).
pub fn resolve_role(event_name: &str, owner_match: Option<bool>) -> Option<Role> {
    if !is_role_event(event_name) {
        return None;
    }
    match owner_match {
        None => Some(Role::Unknown),
        Some(true) => Some(Role::Owner),
        Some(false) => Some(Role::Admin),
    }
}

/// `preprocess_data_properties` (`event_tracking_task.py:43-59`): stamp
/// the resolved role onto the SAME properties object in place (Python
/// mutates `data_properties` and returns it) and hand it back.
pub fn preprocess_data_properties<'a>(
    event_name: &str,
    owner_match: Option<bool>,
    properties: &'a mut Map<String, Value>,
) -> &'a mut Map<String, Value> {
    if let Some(role) = resolve_role(event_name, owner_match) {
        properties.insert("role".to_owned(), Value::String(role.as_str().to_owned()));
    }
    properties
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same committed evidence as the jobs layer:
    /// `rust-api/fixtures/tasks_webhooks/fx-log-01-process-logs.json`.
    static LOG_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-log-01-process-logs.json");
    /// `rust-api/fixtures/tasks_webhooks/fx-evt-01-track-event.json`.
    static EVT_FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-evt-01-track-event.json");

    fn log_vectors() -> serde_json::Value {
        let fixture: serde_json::Value =
            serde_json::from_str(LOG_FIXTURE).expect("log fixture parses");
        fixture["safe_decode_body_executed"].clone()
    }

    #[test]
    fn decode_vectors_match_fixture() {
        let vectors = log_vectors();
        let expect = |key: &str| -> Option<String> {
            if vectors[key].is_null() {
                None
            } else {
                Some(
                    vectors[key]
                        .as_str()
                        .expect("vector is null or string")
                        .to_owned(),
                )
            }
        };
        // Fixture inputs, reconstructed: PNG/JPEG/PDF magic prefixes,
        // one undecodable tail, one plain body, empty, None.
        assert_eq!(
            safe_decode_body(Some(b"\x89PNG\r\n\x1a\nrest")),
            expect("png_magic")
        );
        assert_eq!(
            safe_decode_body(Some(b"\xff\xd8\xff\xe0tail")),
            expect("jpeg_magic")
        );
        assert_eq!(
            safe_decode_body(Some(b"%PDF-1.4 body")),
            expect("pdf_magic")
        );
        assert_eq!(safe_decode_body(Some(b"\xff\xfe\x00")), expect("bad_utf8"));
        // The fixture annotates plain_text with "(decoded as-is, no
        // parse)": the verbatim `decode("utf-8")` output is the text
        // itself — the parenthetical is a note, not output.
        assert_eq!(
            safe_decode_body(Some(b"{\"a\": 1}")),
            Some("{\"a\": 1}".to_owned())
        );
        assert!(
            vectors["plain_text"]
                .as_str()
                .unwrap()
                .starts_with("{\"a\": 1}"),
            "fixture carries the decoded text"
        );
        assert_eq!(safe_decode_body(Some(b"")), expect("empty_bytes"));
        assert_eq!(safe_decode_body(None), expect("none"));
    }

    #[test]
    fn decode_prefix_only_not_contains() {
        // A magic marker NOT at the start still decodes as text
        // (Python checks `startswith`, never `in`).
        assert_eq!(
            safe_decode_body(Some(b"xx%PDF-1.4")),
            Some("xx%PDF-1.4".to_owned())
        );
    }

    #[test]
    fn decode_rule_documents_empty_check() {
        let fixture: serde_json::Value =
            serde_json::from_str(LOG_FIXTURE).expect("log fixture parses");
        let rule = fixture["safe_decode_body_rule"]
            .as_str()
            .expect("rule text");
        assert!(rule.contains("== b''"), "empty check shape pinned: {rule}");
        assert!(rule.contains("bytearray"), "bytearray quirk pinned: {rule}");
    }

    #[test]
    fn role_matrix_matches_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(EVT_FIXTURE).expect("event fixture parses");
        let matrix = &fixture["role_matrix"];
        // Fixture cells are descriptive sentences ("... -> role 'owner'");
        // the ported literal is the role string — pin both.
        assert!(matrix["owner"].as_str().unwrap().contains("role 'owner'"));
        assert_eq!(
            resolve_role(USER_INVITED_TO_WORKSPACE, Some(true)),
            Some(Role::Owner)
        );
        assert_eq!(
            resolve_role(WORKSPACE_DELETED, Some(true)),
            Some(Role::Owner)
        );
        assert!(matrix["admin"].as_str().unwrap().contains("role 'admin'"));
        assert_eq!(
            resolve_role(USER_INVITED_TO_WORKSPACE, Some(false)),
            Some(Role::Admin)
        );
        assert_eq!(
            resolve_role(WORKSPACE_DELETED, Some(false)),
            Some(Role::Admin)
        );
        assert!(matrix["unknown"]
            .as_str()
            .unwrap()
            .contains("role 'unknown'"));
        assert_eq!(
            resolve_role(USER_INVITED_TO_WORKSPACE, None),
            Some(Role::Unknown)
        );
        assert!(matrix["other_events"]
            .as_str()
            .unwrap()
            .contains("UNTOUCHED"));
        assert_eq!(resolve_role("user_joined_workspace", Some(true)), None);
        assert_eq!(resolve_role("workspace_created", None), None);
        assert_eq!(resolve_role("", Some(false)), None);
    }

    #[test]
    fn preprocess_mutates_in_place_and_returns_same_object() {
        let mut props = Map::new();
        props.insert("invitee_email".to_owned(), Value::String("a@x.io".into()));
        let out = preprocess_data_properties(USER_INVITED_TO_WORKSPACE, Some(false), &mut props);
        assert_eq!(out["role"], Value::String("admin".into()));
        assert_eq!(out["invitee_email"], Value::String("a@x.io".into()));
        // Same object: the caller's map carries the mutation.
        assert_eq!(props["role"], Value::String("admin".into()));

        // Outside the pair: untouched, no role key.
        let mut other = Map::new();
        let out = preprocess_data_properties("workspace_created", None, &mut other);
        assert!(out.get("role").is_none());
        assert!(other.is_empty());
    }

    #[test]
    fn event_names_match_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(EVT_FIXTURE).expect("event fixture parses");
        let names = &fixture["event_names"];
        assert_eq!(
            names["USER_INVITED_TO_WORKSPACE"].as_str(),
            Some(USER_INVITED_TO_WORKSPACE)
        );
        assert_eq!(names["WORKSPACE_DELETED"].as_str(), Some(WORKSPACE_DELETED));
    }
}
