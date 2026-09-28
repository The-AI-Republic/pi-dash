//! Prompting serializer shapes: section override + resolved section.
//!
//! Port of `apps/api/pi_dash/prompting/serializers.py` (54 lines):
//!
//! * `serializers.py:14-35` (`PromptSectionOverrideSerializer`,
//!   `ModelSerializer`, 12 `Meta.fields` in source order, every field in
//!   `read_only_fields`).
//! * `serializers.py:38-54` (`ResolvedSectionSerializer`, plain
//!   `Serializer`, 10 fields in declaration order).
//!
//! These are pure output shapes: each `to_representation` takes a row
//! borrowed from the caller and returns a `serde::Serialize` view whose
//! fields are the live DRF wire fields in `Meta.fields` / declaration
//! order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`); a null FK renders `null` (fixture
//! `override_golden_unsaved_row`: `workspace`, `user` and `updated_by`
//! are all `null` on an unsaved row). Datetimes cross this boundary
//! already rendered as DRF `iso-8601` strings with `Z` (`USE_TZ = True`,
//! `TIME_ZONE = "UTC"`, `settings/common.py:361-362`) — formatting owns
//! to the DB edge, so rendering here is a byte-exact passthrough.
//!
//! Caller-owned derivations (documented here, computed upstream):
//!
//! * `is_workspace_level` is a read-only model `@property`
//!   (`models.py:139-141`: `self.user_id is None`) surfaced as
//!   `BooleanField(read_only=True)` (`serializers.py:17`); the row
//!   carries the already-derived bool.
//! * Resolved dicts are built by `_section_breakdown`
//!   (`views.py:102-143`), not from the `ResolvedSection` dataclass
//!   (`composer.py:38-47`, which lacks `default_body`,
//!   `needs_attention` and the `editable_*` flags): `customizable` is
//!   the *effective* tier (`views.py:135`, `effective_customizability`,
//!   not the registry raw value), `default_body` is the pristine
//!   registry default (`views.py:138`), `needs_attention` comes from
//!   the override row that actually resolved else `False`
//!   (`views.py:140`), and `editable_at_workspace` /
//!   `editable_at_personal` are the tier capability gates
//!   (`views.py:141-142`).
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields = fields` (`serializers.py:35`) constrains writes,
//! of which this port has none.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-status (`views.py:243`, also in the fixture `bugs` array): PUT
//!   create returns `200` (not `201`) with the read serializer body.
//!   Status codes own to the handler layer (PIDASHCONV-158); this shape
//!   renders the same body either way.

use serde::Serialize;

/// `PromptSectionOverrideSerializer.Meta.fields` (`serializers.py:18-33`),
/// in source order. `read_only_fields` (`serializers.py:35`) is the same
/// list (fixture `override_all_read_only`).
pub const OVERRIDE_FIELDS: [&str; 12] = [
    "id",
    "workspace",
    "user",
    "section_key",
    "body",
    "is_active",
    "version",
    "needs_attention",
    "is_workspace_level",
    "updated_by",
    "created_at",
    "updated_at",
];

/// `ResolvedSectionSerializer` fields (`serializers.py:41-53`), in
/// declaration order.
pub const RESOLVED_FIELDS: [&str; 10] = [
    "key",
    "title",
    "customizable",
    "body",
    "default_body",
    "source",
    "version",
    "needs_attention",
    "editable_at_workspace",
    "editable_at_personal",
];

/// A `PromptSectionOverride` row for read serialization. UUID primary
/// keys and FK columns (`workspace`, `user`, `updated_by`) are UUID
/// strings, `None` when null. Datetimes are pre-rendered DRF `iso-8601`
/// `Z` strings. `is_workspace_level` is the caller-derived model
/// property (`models.py:139-141`).
#[derive(Debug, Clone, PartialEq)]
pub struct OverrideRow<'a> {
    pub id: &'a str,
    pub workspace: Option<&'a str>,
    pub user: Option<&'a str>,
    pub section_key: &'a str,
    pub body: &'a str,
    pub is_active: bool,
    pub version: i64,
    pub needs_attention: bool,
    pub is_workspace_level: bool,
    pub updated_by: Option<&'a str>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
}

/// `PromptSectionOverrideSerializer.to_representation` output
/// (`serializers.py:14-35`), in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverrideView<'a> {
    pub id: &'a str,
    pub workspace: Option<&'a str>,
    pub user: Option<&'a str>,
    pub section_key: &'a str,
    pub body: &'a str,
    pub is_active: bool,
    pub version: i64,
    pub needs_attention: bool,
    pub is_workspace_level: bool,
    pub updated_by: Option<&'a str>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
}

/// Port of `PromptSectionOverrideSerializer` (`serializers.py:14-35`).
pub fn override_to_representation<'a>(row: &'a OverrideRow<'a>) -> OverrideView<'a> {
    OverrideView {
        id: row.id,
        workspace: row.workspace,
        user: row.user,
        section_key: row.section_key,
        body: row.body,
        is_active: row.is_active,
        version: row.version,
        needs_attention: row.needs_attention,
        is_workspace_level: row.is_workspace_level,
        updated_by: row.updated_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// One `_section_breakdown` dict (`views.py:130-143`) for resolved
/// serialization. Every string is caller-supplied verbatim, including
/// `customizable` (the effective tier) and `source` (`"default"` |
/// `"workspace"` | `"user:<id>"`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRow<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub customizable: &'a str,
    pub body: &'a str,
    pub default_body: &'a str,
    pub source: &'a str,
    pub version: i64,
    pub needs_attention: bool,
    pub editable_at_workspace: bool,
    pub editable_at_personal: bool,
}

/// `ResolvedSectionSerializer.to_representation` output
/// (`serializers.py:38-54`), in declaration order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedView<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub customizable: &'a str,
    pub body: &'a str,
    pub default_body: &'a str,
    pub source: &'a str,
    pub version: i64,
    pub needs_attention: bool,
    pub editable_at_workspace: bool,
    pub editable_at_personal: bool,
}

/// Port of `ResolvedSectionSerializer` (`serializers.py:38-54`).
pub fn resolved_to_representation<'a>(row: &'a ResolvedRow<'a>) -> ResolvedView<'a> {
    ResolvedView {
        key: row.key,
        title: row.title,
        customizable: row.customizable,
        body: row.body,
        default_body: row.default_body,
        source: row.source,
        version: row.version,
        needs_attention: row.needs_attention,
        editable_at_workspace: row.editable_at_workspace,
        editable_at_personal: row.editable_at_personal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-serializers.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("field list is an array")
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .unwrap_or_else(|| panic!("field entry is not a string: {entry}"))
                    .to_owned()
            })
            .collect()
    }

    fn req<'a>(obj: &'a Value, key: &str) -> &'a str {
        obj.get(key).and_then(Value::as_str).unwrap_or_else(|| {
            panic!("golden lacks required string key {key}");
        })
    }

    fn opt(obj: &Value, key: &str) -> Option<String> {
        match obj.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => panic!("golden key {key} is not a string/null: {other}"),
        }
    }

    fn boolean(obj: &Value, key: &str) -> bool {
        obj.get(key).and_then(Value::as_bool).unwrap_or_else(|| {
            panic!("golden lacks required bool key {key}");
        })
    }

    fn integer(obj: &Value, key: &str) -> i64 {
        obj.get(key).and_then(Value::as_i64).unwrap_or_else(|| {
            panic!("golden lacks required int key {key}");
        })
    }

    /// Wire key order of one serialized view: byte positions of each
    /// `"key":` token in ascending order. Struct serialization order is
    /// the wire order, independent of any `Map` implementation detail.
    fn assert_key_order(body: &str, fields: &[&str]) {
        let mut cursor = 0;
        for field in fields {
            let token = format!("\"{field}\":");
            let pos = body[cursor..].find(&token).unwrap_or_else(|| {
                panic!("wire body lacks ordered key {field}: {body}");
            });
            cursor += pos + token.len();
        }
    }

    #[test]
    fn override_fields_match_fixture_order() {
        // Fixture data.shapes.override_fields / override_all_read_only:
        // Meta.fields (serializers.py:18-33) == read_only_fields (:35).
        let golden = fixture();
        let shapes = golden
            .get("data")
            .expect("data")
            .get("shapes")
            .expect("shapes");
        let expected: Vec<String> = OVERRIDE_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(str_list(&shapes["override_fields"]), expected);
        assert_eq!(str_list(&shapes["override_all_read_only"]), expected);
    }

    #[test]
    fn resolved_fields_match_fixture_order() {
        // Fixture data.shapes.resolved_fields: declaration order
        // (serializers.py:41-53).
        let golden = fixture();
        let shapes = golden
            .get("data")
            .expect("data")
            .get("shapes")
            .expect("shapes");
        let expected: Vec<String> = RESOLVED_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(str_list(&shapes["resolved_fields"]), expected);
    }

    #[test]
    fn override_workspace_row_replays_golden() {
        // Fixture data.saved_row.workspace_row_golden: a saved
        // workspace-level row (user null => is_workspace_level true).
        let golden = fixture();
        let output = &golden["data"]["saved_row"]["workspace_row_golden"];
        let workspace = opt(output, "workspace");
        let user = opt(output, "user");
        let updated_by = opt(output, "updated_by");
        let created_at = opt(output, "created_at");
        let updated_at = opt(output, "updated_at");
        let row = OverrideRow {
            id: req(output, "id"),
            workspace: workspace.as_deref(),
            user: user.as_deref(),
            section_key: req(output, "section_key"),
            body: req(output, "body"),
            is_active: boolean(output, "is_active"),
            version: integer(output, "version"),
            needs_attention: boolean(output, "needs_attention"),
            is_workspace_level: boolean(output, "is_workspace_level"),
            updated_by: updated_by.as_deref(),
            created_at: created_at.as_deref(),
            updated_at: updated_at.as_deref(),
        };
        assert!(
            row.is_workspace_level,
            "null user must read workspace-level"
        );
        let view = override_to_representation(&row);
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(
            produced, *output,
            "field-for-field mismatch against workspace_row_golden"
        );
        assert_key_order(
            &serde_json::to_string(&view).expect("serializes"),
            &OVERRIDE_FIELDS,
        );
    }

    #[test]
    fn override_unsaved_row_replays_golden_with_nulls() {
        // Fixture data.shapes.override_golden_unsaved_row: unsaved row —
        // workspace/user/updated_by all null, datetimes still render.
        let golden = fixture();
        let output = &golden["data"]["shapes"]["override_golden_unsaved_row"];
        let workspace = opt(output, "workspace");
        let user = opt(output, "user");
        let updated_by = opt(output, "updated_by");
        let created_at = opt(output, "created_at");
        let updated_at = opt(output, "updated_at");
        let row = OverrideRow {
            id: req(output, "id"),
            workspace: workspace.as_deref(),
            user: user.as_deref(),
            section_key: req(output, "section_key"),
            body: req(output, "body"),
            is_active: boolean(output, "is_active"),
            version: integer(output, "version"),
            needs_attention: boolean(output, "needs_attention"),
            is_workspace_level: boolean(output, "is_workspace_level"),
            updated_by: updated_by.as_deref(),
            created_at: created_at.as_deref(),
            updated_at: updated_at.as_deref(),
        };
        assert_eq!(row.workspace, None);
        assert_eq!(row.user, None);
        assert_eq!(row.updated_by, None);
        let view = override_to_representation(&row);
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(
            produced, *output,
            "field-for-field mismatch against override_golden_unsaved_row"
        );
        assert_key_order(
            &serde_json::to_string(&view).expect("serializes"),
            &OVERRIDE_FIELDS,
        );
    }

    #[test]
    fn resolved_section_replays_golden() {
        // Fixture data.shapes.resolved_golden: one _section_breakdown
        // dict through ResolvedSectionSerializer.
        let golden = fixture();
        let output = &golden["data"]["shapes"]["resolved_golden"];
        let row = ResolvedRow {
            key: req(output, "key"),
            title: req(output, "title"),
            customizable: req(output, "customizable"),
            body: req(output, "body"),
            default_body: req(output, "default_body"),
            source: req(output, "source"),
            version: integer(output, "version"),
            needs_attention: boolean(output, "needs_attention"),
            editable_at_workspace: boolean(output, "editable_at_workspace"),
            editable_at_personal: boolean(output, "editable_at_personal"),
        };
        let view = resolved_to_representation(&row);
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(
            produced, *output,
            "field-for-field mismatch against resolved_golden"
        );
        assert_key_order(
            &serde_json::to_string(&view).expect("serializes"),
            &RESOLVED_FIELDS,
        );
    }
}
