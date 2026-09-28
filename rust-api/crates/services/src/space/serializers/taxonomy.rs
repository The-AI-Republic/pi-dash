//! Space taxonomy serializers: cycle / module / label shapes.
//!
//! Port of `apps/api/pi_dash/space/serializer/`:
//!
//! * `cycle.py:10-21` (`CycleBaseSerializer`, `fields = "__all__"`)
//! * `module.py:10-21` (`ModuleBaseSerializer`, `fields = "__all__"`)
//! * `issue.py:50-57` (`LabelSerializer`, `fields = "__all__"` + nests)
//! * `issue.py:467-470` (`LabelLiteSerializer`, `[id, name, color]`)
//!
//! These are pure output shapes: each `to_representation` takes a row borrowed
//! from the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields in DRF order (declared fields first, then model
//! definition order). UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`. Datetimes
//! and dates cross this boundary already rendered as DRF `iso-8601` strings —
//! formatting owns to the DB edge, so rendering here is a byte-exact
//! passthrough.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`cycle.py:14-20`, `module.py:14-20`,
//! `issue.py:57`) constrain writes, of which this port has none.
//!
//! No ported bugs in these four serializers: straight field mappings with no
//! custom `create`/`update`/`to_representation`/`validate`.

use serde::Serialize;

use super::lite::{ProjectLiteView, WorkspaceLiteView};

/// The `CycleBaseSerializer` `fields = "__all__"` key set (`cycle.py:10-21`):
/// every concrete model field — `id` (`BaseModel`), audit columns
/// (`AuditModel`, `db/mixins.py:16-85`), FKs (`ProjectBaseModel`,
/// `db/models/project.py:302-304`: `project`, then `workspace`), then
/// `Cycle`'s own columns in definition order (`db/models/cycle.py:60-80`).
pub const CYCLE_ALL_FIELDS: [&str; 22] = [
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
    "start_date",
    "end_date",
    "owned_by",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
];

/// A database row for `Cycle` rendering. Datetimes are pre-rendered DRF
/// strings; `project`, `workspace` and `owned_by` are UUID strings
/// (`owned_by` is a required FK, `cycle.py:65-69`); JSON columns are borrowed
/// values.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub owned_by: &'a str,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: &'a serde_json::Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub timezone: &'a str,
    pub version: i32,
}

/// `CycleBaseSerializer.to_representation` output (`cycle.py:10-21`,
/// `fields = "__all__"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CycleView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub owned_by: &'a str,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub progress_snapshot: &'a serde_json::Value,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub timezone: &'a str,
    pub version: i32,
}

/// Port of `CycleBaseSerializer` (`cycle.py:10-21`). Field-for-field copy.
pub fn cycle_to_representation<'a>(row: &'a CycleRow<'a>) -> CycleView<'a> {
    CycleView {
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
        start_date: row.start_date,
        end_date: row.end_date,
        owned_by: row.owned_by,
        view_props: row.view_props,
        sort_order: row.sort_order,
        external_source: row.external_source,
        external_id: row.external_id,
        progress_snapshot: row.progress_snapshot,
        archived_at: row.archived_at,
        logo_props: row.logo_props,
        timezone: row.timezone,
        version: row.version,
    }
}

/// The `ModuleBaseSerializer` `fields = "__all__"` key set
/// (`module.py:10-21`): `id` + audit columns + `project`/`workspace`, then
/// `Module`'s own columns in definition order (`db/models/module.py:67-99`),
/// including the nullable `lead` FK and the `members` M2M PK list (DRF
/// `__all__` renders M2M as primary-key lists).
pub const MODULE_ALL_FIELDS: [&str; 23] = [
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
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead",
    "members",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
];

/// A database row for `Module` rendering. `lead` is a nullable FK
/// (`module.py:86`); `members` is the M2M PK list (`module.py:87-93`).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub status: &'a str,
    pub lead: Option<&'a str>,
    pub members: Vec<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
}

/// `ModuleBaseSerializer.to_representation` output (`module.py:10-21`,
/// `fields = "__all__"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub status: &'a str,
    pub lead: Option<&'a str>,
    pub members: Vec<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
}

/// Port of `ModuleBaseSerializer` (`module.py:10-21`). Field-for-field copy.
pub fn module_to_representation<'a>(row: &'a ModuleRow<'a>) -> ModuleView<'a> {
    ModuleView {
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
        description_text: row.description_text,
        description_html: row.description_html,
        start_date: row.start_date,
        target_date: row.target_date,
        status: row.status,
        lead: row.lead,
        members: row.members.clone(),
        view_props: row.view_props,
        sort_order: row.sort_order,
        external_source: row.external_source,
        external_id: row.external_id,
        archived_at: row.archived_at,
        logo_props: row.logo_props,
    }
}

/// The `Label` model field set (`db/models/label.py:11-24` over
/// `WorkspaceBaseModel`: `workspace`, then `project`,
/// `db/models/workspace.py:185-187`): the `fields = "__all__"` body of
/// `LabelSerializer` before its two declared nests.
pub const LABEL_ALL_FIELDS: [&str; 15] = [
    "id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "deleted_at",
    "workspace",
    "project",
    "parent",
    "name",
    "description",
    "color",
    "sort_order",
    "external_source",
    "external_id",
];

/// A database row for `Label` rendering. `workspace` is required;
/// `project` (`null=True`, `workspace.py:187`) and the self-FK `parent`
/// (`label.py:12-18`) are nullable.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub color: &'a str,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub project_detail: ProjectLiteView<'a>,
}

/// `LabelSerializer.to_representation` output (`issue.py:50-57`): the two
/// declared nests first (DRF `__all__` order), then every `Label` column.
/// `project_detail` renders the lite shape even when `project` is null —
/// the caller resolves the nest from the nullable FK (DRF would render
/// `None` for a null source; rows the space API serves always carry one).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LabelView<'a> {
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub project_detail: ProjectLiteView<'a>,
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub name: &'a str,
    pub description: &'a str,
    pub color: &'a str,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
}

/// Port of `LabelSerializer` (`issue.py:50-57`). Field-for-field copy.
pub fn label_to_representation<'a>(row: &'a LabelRow<'a>) -> LabelView<'a> {
    LabelView {
        workspace_detail: row.workspace_detail.clone(),
        project_detail: row.project_detail.clone(),
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        deleted_at: row.deleted_at,
        workspace: row.workspace,
        project: row.project,
        parent: row.parent,
        name: row.name,
        description: row.description,
        color: row.color,
        sort_order: row.sort_order,
        external_source: row.external_source,
        external_id: row.external_id,
    }
}

/// A database row for `Label` lite rendering: `id` UUID string, `name`,
/// `color`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
}

/// `LabelLiteSerializer.to_representation` output (`issue.py:467-470`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LabelLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
}

/// Port of `LabelLiteSerializer` (`issue.py:467-470`).
pub fn label_lite_to_representation<'a>(row: &'a LabelLiteRow<'a>) -> LabelLiteView<'a> {
    LabelLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn lite_leaves() -> Value {
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

    fn object_keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn cycle_replays_lite_leaves_golden() {
        // Fixture serializers/lite_leaves.golden.json: CycleBaseSerializer
        // (cycle.py:10-21, fields=__all__). The golden records a partial row
        // (6 keys); the unrecorded columns stand in with representative
        // values here, so every golden key replays byte-exact (the full
        // 22-key set is pinned below).
        let golden = lite_leaves();
        let input = &case(&golden, "CycleBaseSerializer")["input"];
        let output = &case(&golden, "CycleBaseSerializer")["output"];
        let start_date = opt(input, "start_date");
        let end_date = opt(input, "end_date");
        let empty = serde_json::json!({});
        let row = CycleRow {
            id: req(input, "id"),
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: req(input, "project"),
            workspace: req(input, "workspace"),
            name: req(input, "name"),
            description: "",
            start_date: start_date.as_deref(),
            end_date: end_date.as_deref(),
            owned_by: "99999999-9999-9999-9999-999999999999",
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: &empty,
            archived_at: None,
            logo_props: &empty,
            timezone: "UTC",
            version: 1,
        };
        let produced = serde_json::to_value(cycle_to_representation(&row)).expect("serializes");
        for (key, value) in output.as_object().expect("output object") {
            assert_eq!(produced.get(key), Some(value), "cycle key {key}");
        }
    }

    #[test]
    fn cycle_all_key_set_matches_model() {
        // cycle.py:10-21 fields=__all__ over Cycle(ProjectBaseModel):
        // id + AuditModel columns (mixins.py:16-85) + project/workspace
        // (project.py:302-304) + own columns in definition order
        // (cycle.py:61-80).
        assert_eq!(
            CYCLE_ALL_FIELDS,
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
                "start_date",
                "end_date",
                "owned_by",
                "view_props",
                "sort_order",
                "external_source",
                "external_id",
                "progress_snapshot",
                "archived_at",
                "logo_props",
                "timezone",
                "version",
            ]
        );
        let empty = serde_json::json!({});
        let row = CycleRow {
            id: "55555555-5555-5555-5555-555555555555",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            name: "Sprint 3",
            description: "",
            start_date: None,
            end_date: None,
            owned_by: "99999999-9999-9999-9999-999999999999",
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: &empty,
            archived_at: None,
            logo_props: &empty,
            timezone: "UTC",
            version: 1,
        };
        let produced = serde_json::to_value(cycle_to_representation(&row)).expect("serializes");
        let mut expected: Vec<String> =
            CYCLE_ALL_FIELDS.iter().map(|key| key.to_string()).collect();
        expected.sort();
        assert_eq!(object_keys(&produced), expected);
    }

    #[test]
    fn module_replays_lite_leaves_golden() {
        // Fixture serializers/lite_leaves.golden.json: ModuleBaseSerializer
        // (module.py:10-21, fields=__all__). Partial golden row (5 keys);
        // the rest stand in as null/empty, every golden key replays
        // byte-exact (full 23-key set pinned below).
        let golden = lite_leaves();
        let input = &case(&golden, "ModuleBaseSerializer")["input"];
        let output = &case(&golden, "ModuleBaseSerializer")["output"];
        let empty = serde_json::json!({});
        let row = ModuleRow {
            id: req(input, "id"),
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: req(input, "project"),
            workspace: req(input, "workspace"),
            name: req(input, "name"),
            description: "",
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: req(input, "status"),
            lead: None,
            members: Vec::new(),
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: &empty,
        };
        let produced = serde_json::to_value(module_to_representation(&row)).expect("serializes");
        for (key, value) in output.as_object().expect("output object") {
            assert_eq!(produced.get(key), Some(value), "module key {key}");
        }
    }

    #[test]
    fn module_all_key_set_matches_model() {
        // module.py:10-21 fields=__all__ over Module(ProjectBaseModel):
        // id + audit + project/workspace + own columns in definition order
        // (module.py:68-99), M2M members included (DRF __all__ renders M2M
        // as PK lists).
        assert_eq!(
            MODULE_ALL_FIELDS,
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
                "description_text",
                "description_html",
                "start_date",
                "target_date",
                "status",
                "lead",
                "members",
                "view_props",
                "sort_order",
                "external_source",
                "external_id",
                "archived_at",
                "logo_props",
            ]
        );
        let empty = serde_json::json!({});
        let row = ModuleRow {
            id: "66666666-6666-6666-6666-666666666666",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            name: "Auth",
            description: "",
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: "planned",
            lead: None,
            members: Vec::new(),
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: &empty,
        };
        let produced = serde_json::to_value(module_to_representation(&row)).expect("serializes");
        let mut expected: Vec<String> = MODULE_ALL_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        expected.sort();
        assert_eq!(object_keys(&produced), expected);
        assert_eq!(produced.get("members"), Some(&Value::Array(Vec::new())));
    }

    #[test]
    fn label_view_carries_all_columns_plus_nests() {
        // issue.py:50-57: fields=__all__ (LABEL_ALL_FIELDS, 15 keys) with
        // the declared workspace_detail/project_detail nests first (DRF
        // __all__ order); read_only_fields names workspace/project (:57).
        let icon = serde_json::json!({"color": "#fff"});
        let row = LabelRow {
            id: "77777777-7777-7777-7777-777777777777",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            workspace: "22222222-2222-2222-2222-222222222222",
            project: Some("33333333-3333-3333-3333-333333333333"),
            parent: None,
            name: "Bug",
            description: "",
            color: "#ff0000",
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            workspace_detail: WorkspaceLiteView {
                name: "Acme",
                slug: "acme",
                id: "22222222-2222-2222-2222-222222222222",
            },
            project_detail: ProjectLiteView {
                id: "33333333-3333-3333-3333-333333333333",
                identifier: "WEB",
                name: "Web",
                cover_image: None,
                icon_prop: &icon,
                emoji: Some("🚀"),
                description: "Ship it",
            },
        };
        let produced = serde_json::to_value(label_to_representation(&row)).expect("serializes");
        let mut expected: Vec<String> =
            LABEL_ALL_FIELDS.iter().map(|key| key.to_string()).collect();
        expected.push("workspace_detail".to_owned());
        expected.push("project_detail".to_owned());
        expected.sort();
        assert_eq!(object_keys(&produced), expected);
        assert_eq!(
            object_keys(&produced["workspace_detail"]),
            vec!["id", "name", "slug"]
        );
        assert_eq!(
            object_keys(&produced["project_detail"]),
            vec![
                "cover_image",
                "description",
                "emoji",
                "icon_prop",
                "id",
                "identifier",
                "name"
            ]
        );
        assert_eq!(
            produced.get("color").and_then(Value::as_str),
            Some("#ff0000")
        );
    }

    #[test]
    fn label_lite_renders_three_keys() {
        // issue.py:467-470: Meta.fields = [id, name, color], no read_only
        // declared (the whole serializer is effectively read-only output).
        let row = LabelLiteRow {
            id: "77777777-7777-7777-7777-777777777777",
            name: "Bug",
            color: "#ff0000",
        };
        let produced =
            serde_json::to_value(label_lite_to_representation(&row)).expect("serializes");
        assert_eq!(
            produced,
            serde_json::json!({
                "id": "77777777-7777-7777-7777-777777777777",
                "name": "Bug",
                "color": "#ff0000",
            })
        );
    }
}
