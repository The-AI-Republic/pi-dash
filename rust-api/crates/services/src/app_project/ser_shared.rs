#![forbid(unsafe_code)]

//! Shared + nested serializers for app:project (D-25, L4).
//!
//! Port of `apps/api/pi_dash/app/serializers/`:
//!
//! * `estimate.py:44-50` (`WorkspaceEstimateSerializer`, nested
//!   `EstimatePointSerializer` `:20-32`) — consumed by D-24
//!   `app/views/workspace/estimate.py:32`.
//! * `project.py:249-256` (`ProjectMemberLiteSerializer`, nested app
//!   `UserLiteSerializer` `user.py:141-153`) — consumed by D-26
//!   `app/views/issue/subscriber.py:56`.
//! * `project.py:243-246` (`ProjectIdentifierSerializer`) and `:269-273`
//!   (`ProjectPublicMemberSerializer`) — unreferenced except
//!   `serializers/__init__.py`; ported as-is.
//!
//! Pure output kernels: each `to_representation` takes a row borrowed from
//! the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields. Goldens: `FX-APROJ-04.serializers_shared.json`
//! (`TRACE.md`: serializers/shared); key orders below were additionally
//! confirmed by a live-DRF probe (Django 4.2.30 / DRF 3.15.2).
//!
//! DRF order rule (`__all__`): declared fields first — `id`
//! (`BaseSerializer`, `base.py:8-9`), then the subclass declares (`points`)
//! — then non-relational concrete columns in `_meta` order (`created_at`,
//! `updated_at`, `deleted_at`, the model's own columns), then forward
//! relations trailing (`created_by`, `updated_by`, `project`, `workspace`,
//! …). `ProjectMemberLiteSerializer` has an explicit `Meta.fields` list
//! (`:255`), so its order is that list: `member`, `id`, `is_subscribed`.
//!
//! * `is_subscribed` is a read-only `BooleanField` (`:251`) with no
//!   annotation at its only call site, so DRF raises `SkipField` and the
//!   key is absent — not `null` (fixture `member_lite_as_d26_calls_it`).
//!   Here that is `Option<bool>` with `skip_serializing_if`, sitting at
//!   index 2 when present (fixture `member_lite_annotated`).
//! * `member` (`:250`) renders `null` when the FK is null
//!   (`ProjectMember.member`, `null=True`,
//!   `db/models/project.py:332-382`): a nested serializer short-circuits
//!   `None` to a present-but-null key (probed live). Here that is
//!   `Option<UserLiteView>` with no skip.
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`. The one exception is
//!   `ProjectIdentifier.id`: `AuditModel` carries no UUID pk, so Django's
//!   auto `BigAutoField` renders a JSON integer
//!   (`db/models/project.py:386-403`, fixture `project_identifier_pk_type`).
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings — formatting owns to the DB edge, so rendering here is a
//!   byte-exact passthrough.
//! * `avatar_url` is a model `@property`
//!   (`db/models/user.py:142-151`: avatar-asset URL, else `avatar`, else
//!   `None`) resolved by the caller; the view passes it through verbatim.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`estimate.py:50`, `project.py:256,273`) constrain
//! writes, of which this port has none.
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * `is_subscribed` is declared but no queryset ever annotates it, so the
//!   live wire omits the key on every response from the D-26 consumer.
//!   [`ProjectMemberLiteView::is_subscribed`] keeps the `None`-omits shape.
//! * `WorkspaceEstimateSerializer` (`estimate.py:44-50`) is line-identical
//!   to `EstimateReadSerializer` (`:35-41`): a duplicate class in Python,
//!   ported once here (the L3 read shape lives with its own issue).
//! * `EstimatePointSerializer.validate` (`estimate.py:21-28`, the 20-char
//!   `value` cap) is write-path validation owned by L3 (FX-APROJ-03); the
//!   nested read shape here carries no validation, as on the wire.

use serde::Serialize;

/// `WorkspaceEstimateSerializer` wire keys (`estimate.py:44-50`), in live
/// DRF order: declared `id`/`points`, then `__all__` (`Estimate`,
/// `db/models/estimate.py:18-40`).
pub const WORKSPACE_ESTIMATE_WIRE_FIELDS: [&str; 13] = [
    "id",
    "points",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "type",
    "last_used",
    "created_by",
    "updated_by",
    "project",
    "workspace",
];

/// Nested `EstimatePointSerializer` wire keys (`estimate.py:20-32`), in live
/// DRF order (`EstimatePoint`, `db/models/estimate.py:43-57`).
pub const ESTIMATE_POINT_WIRE_FIELDS: [&str; 12] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "key",
    "description",
    "value",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "estimate",
];

/// An `EstimatePoint` row for nested rendering. `key` is a Django
/// `IntegerField` (32-bit); datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatePointRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub key: i32,
    pub description: &'a str,
    pub value: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub estimate: &'a str,
}

/// Nested `EstimatePointSerializer.to_representation` output
/// (`estimate.py:20-32`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EstimatePointView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub key: i32,
    pub description: &'a str,
    pub value: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub estimate: &'a str,
}

/// Port of the nested `EstimatePointSerializer` read shape
/// (`estimate.py:20-32`).
pub fn estimate_point_to_representation<'a>(
    row: &'a EstimatePointRow<'a>,
) -> EstimatePointView<'a> {
    EstimatePointView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        key: row.key,
        description: row.description,
        value: row.value,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        estimate: row.estimate,
    }
}

/// An `Estimate` row for workspace rendering: the `Estimate` columns
/// (`db/models/estimate.py:18-40`) plus the prefetched `points` the D-24
/// consumer loads (`app/views/workspace/estimate.py:27-30`).
/// `estimate_type` renders under the `type` key (`type` is a Rust keyword).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceEstimateRow<'a> {
    pub id: &'a str,
    pub points: &'a [EstimatePointRow<'a>],
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub estimate_type: &'a str,
    pub last_used: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
}

/// `WorkspaceEstimateSerializer.to_representation` output
/// (`estimate.py:44-50`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkspaceEstimateView<'a> {
    pub id: &'a str,
    pub points: Vec<EstimatePointView<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    #[serde(rename = "type")]
    pub estimate_type: &'a str,
    pub last_used: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
}

/// Port of `WorkspaceEstimateSerializer` (`estimate.py:44-50`).
pub fn workspace_estimate_to_representation<'a>(
    row: &'a WorkspaceEstimateRow<'a>,
) -> WorkspaceEstimateView<'a> {
    WorkspaceEstimateView {
        id: row.id,
        points: row
            .points
            .iter()
            .map(estimate_point_to_representation)
            .collect(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        estimate_type: row.estimate_type,
        last_used: row.last_used,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
    }
}

/// App `UserLiteSerializer` wire keys (`user.py:141-153`), in
/// `Meta.fields` order. This is the nested `member` shape, not the
/// `api`-flavoured lite (which carries `email` instead of `is_bot`).
pub const MEMBER_USER_WIRE_FIELDS: [&str; 7] = [
    "id",
    "first_name",
    "last_name",
    "avatar",
    "avatar_url",
    "is_bot",
    "display_name",
];

/// A `User` row for nested lite rendering (`db/models/user.py:56-137`):
/// `id` UUID string, names, `avatar` text, the resolved `avatar_url`
/// (property, `user.py:142-151`), `is_bot` flag, `display_name`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserLiteRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Nested app `UserLiteSerializer.to_representation` output
/// (`user.py:141-153`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserLiteView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Port of the nested app `UserLiteSerializer` (`user.py:141-153`).
pub fn user_lite_to_representation<'a>(row: &'a UserLiteRow<'a>) -> UserLiteView<'a> {
    UserLiteView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
    }
}

/// `ProjectMemberLiteSerializer.Meta.fields` (`project.py:249-256`), wire
/// order: the explicit list `:255`, not `_meta` order.
pub const MEMBER_LITE_WIRE_FIELDS: [&str; 3] = ["member", "id", "is_subscribed"];

/// A `ProjectMember` row for lite rendering (`db/models/project.py:332-382`):
/// membership `id` UUID string, the nested `member` user (`None` when the
/// nullable FK is null — renders a present `null`), and the optional
/// `is_subscribed` annotation (`None` when unannotated — the key is absent,
/// DRF `SkipField`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberLiteRow<'a> {
    pub member: Option<UserLiteRow<'a>>,
    pub id: &'a str,
    pub is_subscribed: Option<bool>,
}

/// `ProjectMemberLiteSerializer.to_representation` output
/// (`project.py:249-256`), in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectMemberLiteView<'a> {
    pub member: Option<UserLiteView<'a>>,
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,
}

/// Port of `ProjectMemberLiteSerializer` (`project.py:249-256`).
pub fn project_member_lite_to_representation<'a>(
    row: &'a ProjectMemberLiteRow<'a>,
) -> ProjectMemberLiteView<'a> {
    ProjectMemberLiteView {
        member: row.member.as_ref().map(user_lite_to_representation),
        id: row.id,
        is_subscribed: row.is_subscribed,
    }
}

/// `ProjectIdentifierSerializer` wire keys (`project.py:243-246`), in live
/// DRF order: declared `id`, then `__all__` (`ProjectIdentifier`,
/// `db/models/project.py:386-403`). `workspace` precedes `project` in
/// `_meta` definition order (`:387-388`).
pub const PROJECT_IDENTIFIER_WIRE_FIELDS: [&str; 9] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "created_by",
    "updated_by",
    "workspace",
    "project",
];

/// A `ProjectIdentifier` row for rendering. `id` is the auto
/// `BigAutoField` integer (`AuditModel` has no UUID pk); `workspace` is
/// nullable (`null=True`, `:387`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectIdentifierRow<'a> {
    pub id: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub project: &'a str,
}

/// `ProjectIdentifierSerializer.to_representation` output
/// (`project.py:243-246`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectIdentifierView<'a> {
    pub id: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub project: &'a str,
}

/// Port of `ProjectIdentifierSerializer` (`project.py:243-246`).
pub fn project_identifier_to_representation<'a>(
    row: &'a ProjectIdentifierRow<'a>,
) -> ProjectIdentifierView<'a> {
    ProjectIdentifierView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        project: row.project,
    }
}

/// `ProjectPublicMemberSerializer` wire keys (`project.py:269-273`), in live
/// DRF order: declared `id`, then `__all__` (`ProjectPublicMember`,
/// `db/models/project.py:442-461`).
pub const PROJECT_PUBLIC_MEMBER_WIRE_FIELDS: [&str; 9] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "member",
];

/// A `ProjectPublicMember` row for rendering: `id` UUID string, audit
/// columns, and the three non-nullable FKs as UUID strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPublicMemberRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub member: &'a str,
}

/// `ProjectPublicMemberSerializer.to_representation` output
/// (`project.py:269-273`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectPublicMemberView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub member: &'a str,
}

/// Port of `ProjectPublicMemberSerializer` (`project.py:269-273`).
pub fn project_public_member_to_representation<'a>(
    row: &'a ProjectPublicMemberRow<'a>,
) -> ProjectPublicMemberView<'a> {
    ProjectPublicMemberView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        member: row.member,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_project/FX-APROJ-04.serializers_shared.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn case<'a>(golden: &'a Value, name: &str) -> &'a Value {
        golden
            .get(name)
            .unwrap_or_else(|| panic!("golden lacks {name}"))
    }

    /// Required string field; panics when missing or not a string.
    fn req<'a>(value: &'a Value, key: &str) -> &'a str {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{key} is a string"))
    }

    /// Nullable string field: JSON `null` (or missing) renders `None`, as a
    /// null FK does on the wire.
    fn opt<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key).and_then(Value::as_str)
    }

    fn keys(value: &Value) -> Vec<String> {
        value
            .get("keys")
            .and_then(Value::as_array)
            .expect("keys array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect()
    }

    /// Canonical form: objects with recursively sorted keys, so the replay
    /// comparison is byte-identical regardless of serializer field order.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for key in keys {
                    sorted.insert(key.clone(), canonical(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    /// Field-for-field equality with the golden plus byte-identical replay
    /// of the canonical form the goldens are stored in.
    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    /// Top-level JSON key order of a struct's serialization.
    ///
    /// Read off the serialized string, not a `serde_json::Value`: struct
    /// serialization always emits declaration order, while `Value` objects
    /// iterate alphabetically.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    fn point_row<'a>(data: &'a Value) -> EstimatePointRow<'a> {
        EstimatePointRow {
            id: req(data, "id"),
            created_at: req(data, "created_at"),
            updated_at: req(data, "updated_at"),
            deleted_at: opt(data, "deleted_at"),
            key: i32::try_from(data["key"].as_i64().expect("key int")).expect("key fits i32"),
            description: req(data, "description"),
            value: req(data, "value"),
            created_by: opt(data, "created_by"),
            updated_by: opt(data, "updated_by"),
            project: req(data, "project"),
            workspace: req(data, "workspace"),
            estimate: req(data, "estimate"),
        }
    }

    fn user_row<'a>(data: &'a Value) -> UserLiteRow<'a> {
        UserLiteRow {
            id: req(data, "id"),
            first_name: req(data, "first_name"),
            last_name: req(data, "last_name"),
            avatar: req(data, "avatar"),
            avatar_url: opt(data, "avatar_url"),
            is_bot: data["is_bot"].as_bool().expect("is_bot bool"),
            display_name: req(data, "display_name"),
        }
    }

    #[test]
    fn wire_field_consts_match_fixture_keys() {
        let golden = fixture();
        assert_eq!(
            WORKSPACE_ESTIMATE_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            keys(case(&golden, "workspace_estimate")),
        );
        assert_eq!(
            MEMBER_LITE_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            keys(case(&golden, "member_lite_annotated")),
        );
        assert_eq!(
            PROJECT_IDENTIFIER_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            keys(case(&golden, "project_identifier")),
        );
        assert_eq!(
            PROJECT_PUBLIC_MEMBER_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            keys(case(&golden, "project_public_member")),
        );
        // The nested shapes pin values, not key arrays; their consts are
        // asserted through the serialized order below.
        assert_eq!(ESTIMATE_POINT_WIRE_FIELDS.len(), 12);
        assert_eq!(MEMBER_USER_WIRE_FIELDS.len(), 7);
    }

    #[test]
    fn workspace_estimate_replays_golden() {
        // Fixture workspace_estimate: Estimate columns plus one nested point
        // (estimate.py:44-50, points :45).
        let golden = fixture();
        let case = case(&golden, "workspace_estimate");
        let data = &case["data"];
        let point_rows = [point_row(&data["points"][0])];
        let row = WorkspaceEstimateRow {
            id: req(data, "id"),
            points: &point_rows,
            created_at: req(data, "created_at"),
            updated_at: req(data, "updated_at"),
            deleted_at: opt(data, "deleted_at"),
            name: req(data, "name"),
            description: req(data, "description"),
            estimate_type: req(data, "type"),
            last_used: data["last_used"].as_bool().expect("last_used bool"),
            created_by: opt(data, "created_by"),
            updated_by: opt(data, "updated_by"),
            project: req(data, "project"),
            workspace: req(data, "workspace"),
        };
        let view = workspace_estimate_to_representation(&row);
        assert_eq!(serialized_keys(&view), keys(case));
        assert_eq!(
            serialized_keys(&view.points[0]),
            ESTIMATE_POINT_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "nested point keys follow EstimatePointSerializer order"
        );
        assert_replay(&serde_json::to_value(&view).expect("serializes"), data);
    }

    #[test]
    fn member_lite_unannotated_omits_is_subscribed() {
        // Fixture member_lite_as_d26_calls_it: the D-26 consumer passes no
        // annotation (subscriber.py:56), so DRF SkipFields is_subscribed and
        // the wire carries [member, id] only (project.py:249-256).
        let golden = fixture();
        let case = case(&golden, "member_lite_as_d26_calls_it");
        let data = &case["data"];
        let row = ProjectMemberLiteRow {
            member: Some(user_row(&data["member"])),
            id: req(data, "id"),
            is_subscribed: None,
        };
        let view = project_member_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), keys(case));
        assert_eq!(
            serialized_keys(&view),
            vec!["member".to_string(), "id".to_string()]
        );
        assert_eq!(
            serialized_keys(view.member.as_ref().expect("member present")),
            MEMBER_USER_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "nested member keys follow app UserLiteSerializer order"
        );
        assert_replay(&serde_json::to_value(&view).expect("serializes"), data);
        // Direct byte check on the critical SkipField case: same keys in the
        // same order with deterministic rendering is the whole wire.
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"member":{"id":"86ce5cd4-f3fe-4d07-909f-2be168a0fab6","first_name":"Fix","last_name":"Ture","avatar":"","avatar_url":null,"is_bot":false,"display_name":"fx02-4e33a3a3"},"id":"544cdab3-8768-4ade-8f31-459e2ffab297"}"#,
        );
    }

    #[test]
    fn member_lite_annotated_keeps_is_subscribed() {
        // Fixture member_lite_annotated: with the annotation present the
        // third key renders in Meta.fields position (:255).
        let golden = fixture();
        let case = case(&golden, "member_lite_annotated");
        let data = &case["data"];
        let row = ProjectMemberLiteRow {
            member: Some(user_row(&data["member"])),
            id: req(data, "id"),
            is_subscribed: data["is_subscribed"].as_bool(),
        };
        let view = project_member_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), keys(case));
        assert_replay(&serde_json::to_value(&view).expect("serializes"), data);
    }

    #[test]
    fn member_lite_null_member_renders_null() {
        // Source-derived arm (not in the fixture; probed live against DRF):
        // ProjectMember.member is nullable, and a nested serializer renders
        // None as a present null key — unlike the skipped annotation.
        let row = ProjectMemberLiteRow {
            member: None,
            id: "544cdab3-8768-4ade-8f31-459e2ffab297",
            is_subscribed: None,
        };
        let view = project_member_lite_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"member":null,"id":"544cdab3-8768-4ade-8f31-459e2ffab297"}"#,
        );
    }

    #[test]
    fn project_identifier_replays_golden() {
        // Fixture project_identifier: int pk (BigAutoField — AuditModel has
        // no UUID pk), workspace-before-project relation order (:387-388).
        let golden = fixture();
        let case = case(&golden, "project_identifier");
        let data = &case["data"];
        assert_eq!(
            golden["project_identifier_pk_type"],
            Value::String("int".to_string())
        );
        let row = ProjectIdentifierRow {
            id: data["id"].as_i64().expect("id int"),
            created_at: req(data, "created_at"),
            updated_at: req(data, "updated_at"),
            deleted_at: opt(data, "deleted_at"),
            name: req(data, "name"),
            created_by: opt(data, "created_by"),
            updated_by: opt(data, "updated_by"),
            workspace: opt(data, "workspace"),
            project: req(data, "project"),
        };
        let view = project_identifier_to_representation(&row);
        assert_eq!(serialized_keys(&view), keys(case));
        assert_replay(&serde_json::to_value(&view).expect("serializes"), data);
    }

    #[test]
    fn project_identifier_null_workspace_renders_null() {
        // Source-derived arm (model null=True, :387): a null workspace FK
        // renders a present null, like every other null FK on these shapes.
        let row = ProjectIdentifierRow {
            id: 4,
            created_at: "2026-10-02T22:14:20.181576Z",
            updated_at: "2026-10-02T22:14:20.181585Z",
            deleted_at: None,
            name: "P86FED31",
            created_by: None,
            updated_by: None,
            workspace: None,
            project: "05ea0bca-e678-4db9-a3af-0ebff8515d7f",
        };
        let produced =
            serde_json::to_value(project_identifier_to_representation(&row)).expect("serializes");
        assert_eq!(produced["workspace"], Value::Null);
        assert!(produced
            .as_object()
            .expect("object")
            .contains_key("workspace"));
    }

    #[test]
    fn project_public_member_replays_golden() {
        // Fixture project_public_member (project.py:269-273).
        let golden = fixture();
        let case = case(&golden, "project_public_member");
        let data = &case["data"];
        let row = ProjectPublicMemberRow {
            id: req(data, "id"),
            created_at: req(data, "created_at"),
            updated_at: req(data, "updated_at"),
            deleted_at: opt(data, "deleted_at"),
            created_by: opt(data, "created_by"),
            updated_by: opt(data, "updated_by"),
            project: req(data, "project"),
            workspace: req(data, "workspace"),
            member: req(data, "member"),
        };
        let view = project_public_member_to_representation(&row);
        assert_eq!(serialized_keys(&view), keys(case));
        assert_replay(&serde_json::to_value(&view).expect("serializes"), data);
    }
}
