//! Space lite-leaf serializers: base/user/workspace/project/state shapes.
//!
//! Port of `apps/api/pi_dash/space/serializer/`:
//!
//! * `base.py:8-9` (`BaseSerializer`, `id = PrimaryKeyRelatedField(read_only=True)`)
//! * `base.py:12-62` (`DynamicBaseSerializer`, `fields=` filtering)
//! * `user.py:10-22` (`UserLiteSerializer`)
//! * `workspace.py:10-14` (`WorkspaceLiteSerializer`)
//! * `project.py:10-22` (`ProjectLiteSerializer`)
//! * `state.py:10-14` (`StateSerializer`, `fields = "__all__"`)
//! * `state.py:17-21` (`StateLiteSerializer`)
//!
//! These are pure output shapes: each `to_representation` takes a row borrowed
//! from the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`. Datetimes
//! cross this boundary already rendered as DRF `iso-8601` strings —
//! formatting owns to the DB edge, so rendering here is a byte-exact
//! passthrough. `avatar_url` is a model `@property`
//! (`db/models/user.py:142-151`: avatar-asset URL, else `avatar`, else `None`)
//! resolved by the caller; the view passes it through verbatim.
//!
//! Shape-only no-ops preserved as documentation, not code: `read_only_fields`
//! (`user.py:22`, `workspace.py:14`, `project.py:22`, `state.py:14,21`)
//! constrain writes, of which this port has none.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-filter (`base.py:41`): `_filter_fields` recurses as
//!   `self._filter_fields(self.fields[key], value)`, passing a `Field` where
//!   the `fields` list belongs. Iterating a DRF `Field` raises `TypeError`,
//!   so ANY nested-dict `fields=` entry raises. [`filter_fields`] returns
//!   [`FilterError::NestedNotSupported`] for that case. Space views never
//!   pass `fields=` (no `fields=` call site under `space/views/`), so the
//!   kernel path is live only for future callers.

use serde::Serialize;

/// One entry of a DRF `fields=` argument (`base.py:16,33-52`): either a plain
/// field name or a `{name: sub-fields}` dict entry. A dict whose value is not
/// a list keeps the key with no recursion (`base.py:40-41` guard); callers map
/// that to [`FieldSpec::Include`].
#[derive(Debug, Clone, PartialEq)]
pub enum FieldSpec {
    /// A plain field name (`isinstance(item, str)`, `base.py:47-48`).
    Include(String),
    /// A `{name: [...]}` dict entry: the key is kept in `allowed`
    /// (`base.py:51-52`) and the sub-list recurses (`base.py:40-41`).
    Nested(String, Vec<FieldSpec>),
}

/// Failure modes of [`filter_fields`], mirroring the Python raises:
/// unknown names raise `KeyError` (`self.fields[key]`, `base.py:41`), nested
/// dicts raise `TypeError` (iterating a `Field`, BUG-filter above).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FilterError {
    /// `KeyError`: the name (or nested key) is not a serializer field.
    #[error("unknown field: {0}")]
    UnknownField(String),
    /// `TypeError` parity (BUG-filter, `base.py:41`): nested `fields=` dicts
    /// always raise in Python, so they always fail here.
    #[error("nested fields= entry always raises TypeError in Python (base.py:41): {0}")]
    NestedNotSupported(String),
}

/// Port of `DynamicBaseSerializer.__init__` + `_filter_fields`
/// (`base.py:12-62`).
///
/// `available` is the serializer's field list in wire order; `specs` is the
/// `fields=` argument (`None` keeps everything, `base.py:22-23`). Returns the
/// kept field names in wire order (Python pops non-allowed keys in place, so
/// survivors keep their relative order, `base.py:54-60`).
pub fn filter_fields(
    available: &[&str],
    specs: Option<&[FieldSpec]>,
) -> Result<Vec<String>, FilterError> {
    let Some(specs) = specs else {
        return Ok(available.iter().map(|name| name.to_string()).collect());
    };
    let mut allowed: Vec<&str> = Vec::with_capacity(specs.len());
    for spec in specs {
        match spec {
            FieldSpec::Include(name) => {
                if !available.contains(&name.as_str()) {
                    return Err(FilterError::UnknownField(name.clone()));
                }
                allowed.push(name.as_str());
            }
            FieldSpec::Nested(key, _) => {
                if !available.contains(&key.as_str()) {
                    return Err(FilterError::UnknownField(key.clone()));
                }
                // BUG-filter (base.py:41): Python recurses into the Field
                // itself and raises TypeError — it never reaches the
                // allowed-list update for this entry either.
                return Err(FilterError::NestedNotSupported(key.clone()));
            }
        }
    }
    Ok(available
        .iter()
        .filter(|name| allowed.contains(name))
        .map(|name| name.to_string())
        .collect())
}

/// A database row for `User` lite rendering (`db/models/user.py:56-137`):
/// `id` UUID string, `avatar` text, resolved `avatar_url` (property,
/// `user.py:142-151`), `is_bot` flag, `display_name`.
// The `id` string passthrough is the `BaseSerializer` rule (`base.py:8-9`).
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

/// `UserLiteSerializer.to_representation` output (`user.py:10-22`), in
/// `Meta.fields` order.
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

/// Port of `UserLiteSerializer` (`user.py:10-22`).
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

/// A database row for `Workspace` lite rendering: `name`, `slug`, `id` UUID
/// string.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceLiteRow<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
}

/// `WorkspaceLiteSerializer.to_representation` output (`workspace.py:10-14`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkspaceLiteView<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
}

/// Port of `WorkspaceLiteSerializer` (`workspace.py:10-14`).
pub fn workspace_lite_to_representation<'a>(
    row: &'a WorkspaceLiteRow<'a>,
) -> WorkspaceLiteView<'a> {
    WorkspaceLiteView {
        name: row.name,
        slug: row.slug,
        id: row.id,
    }
}

/// A database row for `Project` lite rendering (`db/models/project.py:72+`):
/// `id` UUID string, `identifier`, `name`, nullable `cover_image`,
/// `icon_prop` JSON, nullable `emoji` (`project.py:95`, `null=True`),
/// `description`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLiteRow<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub icon_prop: &'a serde_json::Value,
    pub emoji: Option<&'a str>,
    pub description: &'a str,
}

/// `ProjectLiteSerializer.to_representation` output (`project.py:10-22`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectLiteView<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub icon_prop: &'a serde_json::Value,
    pub emoji: Option<&'a str>,
    pub description: &'a str,
}

/// Port of `ProjectLiteSerializer` (`project.py:10-22`).
pub fn project_lite_to_representation<'a>(row: &'a ProjectLiteRow<'a>) -> ProjectLiteView<'a> {
    ProjectLiteView {
        id: row.id,
        identifier: row.identifier,
        name: row.name,
        cover_image: row.cover_image,
        icon_prop: row.icon_prop,
        emoji: row.emoji,
        description: row.description,
    }
}

/// The `StateSerializer` `fields = "__all__"` key set (`state.py:10-14`):
/// every concrete model field — `id` (`BaseModel`, `db/models/base.py:18`),
/// audit columns (`AuditModel`, `db/mixins.py:17-89`), FKs (`ProjectBaseModel`,
/// `db/models/project.py:302-304`), then `State`'s own columns in definition
/// order (`db/models/state.py:93-107`). FK primary keys render as strings;
/// null FKs render `null`.
pub const STATE_ALL_FIELDS: [&str; 18] = [
    "id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "deleted_at",
    "project",
    "workspace",
    "name",
    "description",
    "color",
    "slug",
    "sequence",
    "group",
    "is_triage",
    "default",
    "external_source",
    "external_id",
];

/// A database row for `State` rendering. Datetimes are pre-rendered DRF
/// strings; FK columns (`project`, `workspace`, `created_by`, `updated_by`)
/// are UUID strings, `None` when null.
#[derive(Debug, Clone, PartialEq)]
pub struct StateRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub color: &'a str,
    pub slug: &'a str,
    pub sequence: f64,
    pub group: &'a str,
    pub is_triage: bool,
    pub default: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
}

/// `StateSerializer.to_representation` output (`state.py:10-14`,
/// `fields = "__all__"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub color: &'a str,
    pub slug: &'a str,
    pub sequence: f64,
    pub group: &'a str,
    pub is_triage: bool,
    pub default: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
}

/// Port of `StateSerializer` (`state.py:10-14`).
pub fn state_to_representation<'a>(row: &'a StateRow<'a>) -> StateView<'a> {
    StateView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        deleted_at: row.deleted_at,
        project: row.project,
        workspace: row.workspace,
        name: row.name,
        description: row.description,
        color: row.color,
        slug: row.slug,
        sequence: row.sequence,
        group: row.group,
        is_triage: row.is_triage,
        default: row.default,
        external_source: row.external_source,
        external_id: row.external_id,
    }
}

/// A database row for `State` lite rendering: `id` UUID string, `name`,
/// `color`, `group`.
#[derive(Debug, Clone, PartialEq)]
pub struct StateLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// `StateLiteSerializer.to_representation` output (`state.py:17-21`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// Port of `StateLiteSerializer` (`state.py:17-21`).
pub fn state_lite_to_representation<'a>(row: &'a StateLiteRow<'a>) -> StateLiteView<'a> {
    StateLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
        group: row.group,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/space/serializers/lite_leaves.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn case<'a>(golden: &'a Value, serializer: &str) -> &'a Value {
        golden
            .get("cases")
            .and_then(Value::as_array)
            .expect("cases array")
            .iter()
            .find(|case| case.get("serializer").and_then(Value::as_str) == Some(serializer))
            .unwrap_or_else(|| panic!("golden lacks {serializer} case"))
    }

    fn req<'a>(obj: &'a Value, key: &str) -> &'a str {
        obj.get(key).and_then(Value::as_str).unwrap_or_else(|| {
            panic!("golden input lacks required string key {key}");
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
            panic!("golden input lacks required bool key {key}");
        })
    }

    /// Canonical form: objects with recursively sorted keys. `Value` key
    /// order follows workspace feature unification (`preserve_order` from
    /// the `api`/`jobs` crates), so raw `to_string` is not comparable.
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

    /// Field-for-field equality with the golden output plus byte-identical
    /// replay of the canonical form the goldens are stored in.
    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden output"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    #[test]
    fn user_lite_replays_golden() {
        // Fixture serializers/lite_leaves.golden.json: UserLiteSerializer
        // input row -> exact output JSON, incl. the avatar_url null
        // passthrough (user.py:10-22).
        let golden = fixture();
        let input = &case(&golden, "UserLiteSerializer")["input"];
        let output = &case(&golden, "UserLiteSerializer")["output"];
        let avatar_url = opt(input, "avatar_url");
        let row = UserLiteRow {
            id: req(input, "id"),
            first_name: req(input, "first_name"),
            last_name: req(input, "last_name"),
            avatar: req(input, "avatar"),
            avatar_url: avatar_url.as_deref(),
            is_bot: boolean(input, "is_bot"),
            display_name: req(input, "display_name"),
        };
        let produced = serde_json::to_value(user_lite_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
    }

    #[test]
    fn user_lite_avatar_url_passes_through_non_null() {
        // avatar_url is a resolved model property (user.py:142-151); the
        // golden pins the null arm, this pins the asset/avatar arm.
        let row = UserLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "L",
            avatar: "https://cdn.example/a.png",
            avatar_url: Some("https://cdn.example/a.png"),
            is_bot: false,
            display_name: "Ada L",
        };
        let produced = serde_json::to_value(user_lite_to_representation(&row)).expect("serializes");
        assert_eq!(
            produced.get("avatar_url").and_then(Value::as_str),
            Some("https://cdn.example/a.png")
        );
        assert_eq!(
            produced.get("avatar").and_then(Value::as_str),
            Some("https://cdn.example/a.png")
        );
    }

    #[test]
    fn workspace_lite_replays_golden() {
        // Fixture serializers/lite_leaves.golden.json: WorkspaceLiteSerializer
        // (workspace.py:10-14, keys name/slug/id, all read-only).
        let golden = fixture();
        let input = &case(&golden, "WorkspaceLiteSerializer")["input"];
        let output = &case(&golden, "WorkspaceLiteSerializer")["output"];
        let row = WorkspaceLiteRow {
            name: req(input, "name"),
            slug: req(input, "slug"),
            id: req(input, "id"),
        };
        let produced =
            serde_json::to_value(workspace_lite_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
    }

    #[test]
    fn project_lite_replays_golden() {
        // Fixture serializers/lite_leaves.golden.json: ProjectLiteSerializer
        // (project.py:10-22), incl. the icon_prop JSON dict passthrough.
        let golden = fixture();
        let input = &case(&golden, "ProjectLiteSerializer")["input"];
        let output = &case(&golden, "ProjectLiteSerializer")["output"];
        let cover_image = opt(input, "cover_image");
        let emoji = opt(input, "emoji");
        let row = ProjectLiteRow {
            id: req(input, "id"),
            identifier: req(input, "identifier"),
            name: req(input, "name"),
            cover_image: cover_image.as_deref(),
            icon_prop: input.get("icon_prop").expect("icon_prop"),
            emoji: emoji.as_deref(),
            description: req(input, "description"),
        };
        let produced =
            serde_json::to_value(project_lite_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
    }

    #[test]
    fn project_lite_null_emoji_renders_null() {
        // Project.emoji is nullable (db/models/project.py:95, null=True):
        // DRF renders a null emoji as null, not "".
        let icon = serde_json::json!({"color": "#fff"});
        let row = ProjectLiteRow {
            id: "33333333-3333-3333-3333-333333333333",
            identifier: "WEB",
            name: "Web",
            cover_image: None,
            icon_prop: &icon,
            emoji: None,
            description: "Ship it",
        };
        let produced =
            serde_json::to_value(project_lite_to_representation(&row)).expect("serializes");
        assert_eq!(produced.get("emoji"), Some(&Value::Null));
    }

    #[test]
    fn state_replays_golden() {
        // Fixture serializers/lite_leaves.golden.json: StateSerializer
        // (state.py:10-14, fields=__all__). The golden records a partial row
        // (11 keys); audit/extra columns the fixture omits stand in as None
        // here, so every golden key replays byte-exact while the produced
        // object carries the full __all__ key set (pinned below).
        let golden = fixture();
        let input = &case(&golden, "StateSerializer")["input"];
        let output = &case(&golden, "StateSerializer")["output"];
        let created_at = opt(input, "created_at");
        let updated_at = opt(input, "updated_at");
        let created_by = opt(input, "created_by");
        let updated_by = opt(input, "updated_by");
        let deleted_at = opt(input, "deleted_at");
        let project = opt(input, "project");
        let workspace = opt(input, "workspace");
        let external_source = opt(input, "external_source");
        let external_id = opt(input, "external_id");
        let row = StateRow {
            id: req(input, "id"),
            created_at: created_at.as_deref(),
            updated_at: updated_at.as_deref(),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
            deleted_at: deleted_at.as_deref(),
            project: project.as_deref(),
            workspace: workspace.as_deref(),
            name: req(input, "name"),
            description: req(input, "description"),
            color: req(input, "color"),
            slug: req(input, "slug"),
            sequence: input
                .get("sequence")
                .and_then(Value::as_f64)
                .expect("sequence"),
            group: req(input, "group"),
            is_triage: boolean(input, "is_triage"),
            default: boolean(input, "default"),
            external_source: external_source.as_deref(),
            external_id: external_id.as_deref(),
        };
        let produced = serde_json::to_value(state_to_representation(&row)).expect("serializes");
        // Every golden key replays byte-exact.
        for (key, value) in output.as_object().expect("output object") {
            assert_eq!(produced.get(key), Some(value), "state key {key}");
        }
        // The produced object carries the full __all__ key set (missing
        // fixture columns stand in as null).
        let mut expected_full = output.as_object().expect("output object").clone();
        for key in STATE_ALL_FIELDS {
            expected_full.entry(key.to_owned()).or_insert(Value::Null);
        }
        assert_replay(&produced, &Value::Object(expected_full));
    }

    #[test]
    fn state_all_key_set_matches_model() {
        // state.py:10-14 fields=__all__ over State(ProjectBaseModel):
        // id + AuditModel columns (mixins.py:17-89) + project/workspace
        // (project.py:302-304) + own columns in definition order
        // (state.py:93-107).
        assert_eq!(
            STATE_ALL_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "deleted_at",
                "project",
                "workspace",
                "name",
                "description",
                "color",
                "slug",
                "sequence",
                "group",
                "is_triage",
                "default",
                "external_source",
                "external_id",
            ]
        );
    }

    #[test]
    fn state_lite_replays_golden() {
        // Fixture serializers/lite_leaves.golden.json: StateLiteSerializer
        // (state.py:17-21, keys id/name/color/group, all read-only).
        let golden = fixture();
        let input = &case(&golden, "StateLiteSerializer")["input"];
        let output = &case(&golden, "StateLiteSerializer")["output"];
        let row = StateLiteRow {
            id: req(input, "id"),
            name: req(input, "name"),
            color: req(input, "color"),
            group: req(input, "group"),
        };
        let produced =
            serde_json::to_value(state_lite_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
    }

    #[test]
    fn filter_fields_none_keeps_all() {
        let available = ["id", "name", "color"];
        assert_eq!(
            filter_fields(&available, None).expect("keeps all"),
            vec!["id", "name", "color"]
        );
    }

    #[test]
    fn filter_fields_flat_selection_keeps_wire_order() {
        // base.py:54-60: survivors keep serializer order, not fields= order.
        let available = ["id", "first_name", "last_name", "avatar"];
        let specs = [
            FieldSpec::Include("avatar".to_owned()),
            FieldSpec::Include("id".to_owned()),
        ];
        assert_eq!(
            filter_fields(&available, Some(&specs)).expect("filters"),
            vec!["id", "avatar"]
        );
    }

    #[test]
    fn filter_fields_unknown_name_is_key_error() {
        // Mirrors the KeyError from self.fields[name] on unknown names.
        let available = ["id", "name"];
        let specs = [FieldSpec::Include("nope".to_owned())];
        assert_eq!(
            filter_fields(&available, Some(&specs)),
            Err(FilterError::UnknownField("nope".to_owned()))
        );
    }

    #[test]
    fn filter_fields_nested_dict_always_fails() {
        // BUG-filter (base.py:41): Python passes the Field itself as the
        // fields list and raises TypeError before filtering anything.
        let available = ["id", "name", "color"];
        let specs = [FieldSpec::Nested(
            "name".to_owned(),
            vec![FieldSpec::Include("id".to_owned())],
        )];
        assert_eq!(
            filter_fields(&available, Some(&specs)),
            Err(FilterError::NestedNotSupported("name".to_owned()))
        );
    }
}
