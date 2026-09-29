#![forbid(unsafe_code)]

//! State / estimate serializers: validation, create-context and read shapes.
//!
//! Port of `apps/api/pi_dash/api/serializers/state.py:11-55` and
//! `apps/api/pi_dash/api/serializers/estimate.py:13-37`:
//!
//! * `state.py:19-27` (`StateSerializer.validate` — default-flip side
//!   effect, then the triage rejection)
//! * `state.py:28-41` (`StateSerializer.Meta`: `fields = "__all__"` +
//!   `read_only_fields`)
//! * `state.py:44-55` (`StateLiteSerializer`, all four fields read-only)
//! * `estimate.py:13-22` (`EstimateSerializer.Meta` + `create`, which
//!   injects `workspace` + `project` from the serializer context; the view
//!   passes `context={'workspace': ..., 'project': ...}`
//!   (`views/estimate.py:64`))
//! * `estimate.py:26-33` (`EstimatePointSerializer.validate`)
//! * `estimate.py:34-37` (`EstimatePointSerializer.Meta`)
//!
//! Related model rules this layer pins (shape + validate only; the column
//! lists themselves are FX-MODELS, owned by PIDASHCONV-352):
//! `State.save` (`db/models/state.py:131-139`, slug + max+15000 sequence)
//! and the workspace backfill (`db/models/project.py:309-311`) are
//! documented on the shape functions they affect.
//!
//! These are pure shapes plus the two validators: each
//! `*_to_representation` takes a row borrowed from the caller and returns
//! a `serde::Serialize` view whose fields are the live DRF wire fields in
//! DRF order (the declared `id` first — `serializers/base.py`, via
//! `BaseSerializer.id = PrimaryKeyRelatedField(read_only=True)` — then
//! model definition order). UUID and FK primary keys render as strings;
//! a null FK renders `null`. Datetimes cross this boundary already
//! rendered as DRF `iso-8601` strings — formatting owns to the DB edge,
//! so rendering here is a byte-exact passthrough.
//!
//! Ported bugs (translate as-is, do not fix):
//! BUG-4a — the default-flip runs inside `validate()`
//! (`state.py:21-22`): every sibling state loses `default` even if the
//! subsequent save fails, and it runs before the triage rejection below,
//! so a rejected triage payload still clears its siblings' defaults.
//! BUG-4b — a caller-supplied `sequence` is overwritten by
//! `State.save()` with max(sibling sequence)+15000 whenever the project
//! already has states (`db/models/state.py:131-139`); the golden output
//! below keeps the illustrative `1000.0` from FX-WORKFLOW-SER, which the
//! contract `test_create` never asserts back.
//!
//! Out of scope for this module (owned elsewhere): the estimate
//! URL-unregistered 404 (BUG-2, handler layer, PIDASHCONV-372);
//! the `{"error": "Estimate points are required"}` empty-list branch,
//! which lives in the estimate-points view (`views/estimate.py:205-210`),
//! not in this serializer; the state-name / external-id 409 bodies
//! (`views/state.py`, handler layer, PIDASHCONV-372).

use serde::Serialize;

/// `StateSerializer.Meta.read_only_fields` (`state.py:30-40`).
pub const STATE_READ_ONLY_FIELDS: [&str; 9] = [
    "id",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "workspace",
    "project",
    "deleted_at",
    "slug",
];

/// Writable `StateSerializer` fields: `__all__` minus
/// [`STATE_READ_ONLY_FIELDS`].
pub const STATE_WRITABLE_FIELDS: [&str; 9] = [
    "name",
    "description",
    "color",
    "sequence",
    "group",
    "is_triage",
    "default",
    "external_source",
    "external_id",
];

/// `StateLiteSerializer.Meta.fields` (`state.py:53`); every entry is also
/// in `read_only_fields` (`state.py:54`). Defined but unused by every
/// D-19 view — ported for completeness.
pub const STATE_LITE_FIELDS: [&str; 4] = ["id", "name", "color", "group"];

/// `EstimateSerializer.Meta.read_only_fields` (`estimate.py:16`).
/// `id` is additionally read-only as the auto primary key
/// (`serializers/base.py`).
pub const ESTIMATE_READ_ONLY_FIELDS: [&str; 3] = ["workspace", "project", "deleted_at"];

/// Writable `EstimateSerializer` fields.
pub const ESTIMATE_WRITABLE_FIELDS: [&str; 4] = ["name", "description", "type", "last_used"];

/// `EstimatePointSerializer.Meta.read_only_fields` (`estimate.py:37`).
/// `id` is additionally read-only as the auto primary key.
pub const ESTIMATE_POINT_READ_ONLY_FIELDS: [&str; 3] = ["estimate", "workspace", "project"];

/// Writable `EstimatePointSerializer` fields.
pub const ESTIMATE_POINT_WRITABLE_FIELDS: [&str; 3] = ["key", "description", "value"];

/// Rejection for `group == "triage"` (`state.py:24-25`, exact match on
/// `StateGroup.TRIAGE.value`). The message says "create" but `validate`
/// also runs on update — ported as-is.
pub const TRIAGE_REJECTED_MESSAGE: &str = "Cannot create triage state";

/// Rejection for an empty validated payload (`estimate.py:27-28` — falsy
/// data dict rejected before any field check).
pub const ESTIMATE_POINTS_REQUIRED_MESSAGE: &str = "Estimate points are required";

/// Rejection for an over-long point value (`estimate.py:29-31` — only
/// when `value` is truthy AND longer than 20; an empty value passes).
pub const ESTIMATE_POINT_VALUE_TOO_LONG_MESSAGE: &str = "Value can't be more than 20 characters";

/// Maximum point-value length in code points (`estimate.py:30`).
/// Python `len(value)` counts code points, so this port uses
/// `.chars().count()`, never `.len()` (UTF-8 bytes).
pub const ESTIMATE_POINT_VALUE_MAX_CHARS: usize = 20;

/// The triage group value (`db/models/state.py:24`, `StateGroup.TRIAGE`).
pub const STATE_GROUP_TRIAGE: &str = "triage";

/// DRF renders a bare-string `ValidationError` as
/// `{"non_field_errors": ["<msg>"]}` with status 400.
pub fn non_field_errors(message: &str) -> serde_json::Value {
    serde_json::json!({ "non_field_errors": [message] })
}

/// Mirrors `data.get("default", False)` truthiness (`state.py:20`): only
/// an explicitly-true `default` arms the flip; absent or false does not.
pub fn state_default_flip_requested(default: Option<bool>) -> bool {
    default == Some(true)
}

/// The default-flip side effect (`state.py:21-22`):
/// `State.objects.filter(project_id=...).update(default=False)`.
///
/// The queryset starts from `StateManager.get_queryset`, which is the
/// soft-delete filter (`deleted_at IS NULL`, `db/mixins.py:49-51`) plus
/// the triage exclusion (`db/models/state.py:82-84`) — so triage and
/// soft-deleted siblings keep their `default`. Django quotes the
/// identifiers (`"default"` is reserved); `$1` is the project id.
pub const STATE_DEFAULT_FLIP_SQL: &str = concat!(
    "UPDATE \"states\" SET \"default\" = false ",
    "WHERE \"project_id\" = $1 ",
    "AND \"deleted_at\" IS NULL ",
    "AND NOT (\"group\" = 'triage')"
);

/// Mirrors `data.get("group", None) == StateGroup.TRIAGE.value`
/// (`state.py:24-25`): exact match only.
pub fn state_group_rejected(group: Option<&str>) -> bool {
    group == Some(STATE_GROUP_TRIAGE)
}

/// Port of `StateSerializer.validate` (`state.py:19-27`).
///
/// Returns `(default_flip_armed, validation_error_body)`. The flip flag
/// is computed first because the Python runs the UPDATE before the
/// triage check (BUG-4a): callers must execute
/// [`STATE_DEFAULT_FLIP_SQL`] even when the returned body is `Some`.
pub fn state_validate(
    default: Option<bool>,
    group: Option<&str>,
) -> (bool, Option<serde_json::Value>) {
    let flip = state_default_flip_requested(default);
    if state_group_rejected(group) {
        return (flip, Some(non_field_errors(TRIAGE_REJECTED_MESSAGE)));
    }
    (flip, None)
}

/// Port of `EstimatePointSerializer.validate` (`estimate.py:26-33`).
///
/// `is_empty` mirrors `if not data` on the validated dict; `value` is
/// `data.get("value")`. Length is counted in code points.
pub fn estimate_point_validate(is_empty: bool, value: Option<&str>) -> Option<serde_json::Value> {
    if is_empty {
        return Some(non_field_errors(ESTIMATE_POINTS_REQUIRED_MESSAGE));
    }
    match value {
        Some(v) if !v.is_empty() && v.chars().count() > ESTIMATE_POINT_VALUE_MAX_CHARS => {
            Some(non_field_errors(ESTIMATE_POINT_VALUE_TOO_LONG_MESSAGE))
        }
        _ => None,
    }
}

/// The `EstimateSerializer.create` context (`estimate.py:19-22`): the
/// view always passes both keys (`views/estimate.py:64`); Python raises
/// `KeyError` when one is absent, so both are required here too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EstimateCreateContext<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
}

impl<'a> EstimateCreateContext<'a> {
    /// Mirrors `self.context["workspace"]` / `self.context["project"]`
    /// (`estimate.py:20-21`): each missing key is an error naming it.
    pub fn require(
        workspace_id: Option<&'a str>,
        project_id: Option<&'a str>,
    ) -> Result<Self, String> {
        let workspace_id = workspace_id.ok_or_else(|| "workspace".to_string())?;
        let project_id = project_id.ok_or_else(|| "project".to_string())?;
        Ok(Self {
            workspace_id,
            project_id,
        })
    }
}

/// A database row for the state read shape. Datetimes are pre-rendered
/// DRF `iso-8601` strings; ids and FKs are rendered UUID strings with
/// `null` for absent relations.
#[derive(Debug, Clone, PartialEq)]
pub struct StateRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
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

/// `StateSerializer.to_representation` output (`state.py:11-41`,
/// `fields = "__all__"` over `db/models/state.py:93-140`), in DRF wire
/// order: declared `id`, then model definition order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
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

/// Port of `StateSerializer` read shape (`state.py:11-41`).
///
/// `slug` is always `slugify(name)` (`db/models/state.py:131-139`);
/// `workspace` is backfilled from the project on save
/// (`db/models/project.py:309-311`) — both arrive here already stored.
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

/// A database row for the lite state shape.
#[derive(Debug, Clone, PartialEq)]
pub struct StateLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// `StateLiteSerializer.to_representation` output (`state.py:44-55`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// Port of `StateLiteSerializer` read shape (`state.py:44-55`).
pub fn state_lite_to_representation<'a>(row: &'a StateLiteRow<'a>) -> StateLiteView<'a> {
    StateLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
        group: row.group,
    }
}

/// A database row for the estimate read shape.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimateRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub estimate_type: &'a str,
    pub last_used: bool,
}

/// `EstimateSerializer.to_representation` output (`estimate.py:13-16`,
/// `fields = "__all__"` over `db/models/estimate.py:18-40`), in DRF
/// wire order. The model field is `type`; it serializes under that key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EstimateView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    #[serde(rename = "type")]
    pub estimate_type: &'a str,
    pub last_used: bool,
}

/// Port of `EstimateSerializer` read shape (`estimate.py:13-22`).
pub fn estimate_to_representation<'a>(row: &'a EstimateRow<'a>) -> EstimateView<'a> {
    EstimateView {
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
        estimate_type: row.estimate_type,
        last_used: row.last_used,
    }
}

/// A database row for the estimate-point read shape.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatePointRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub estimate: &'a str,
    pub key: i32,
    pub description: &'a str,
    pub value: &'a str,
}

/// `EstimatePointSerializer.to_representation` output
/// (`estimate.py:25-37`, `fields = "__all__"` over
/// `db/models/estimate.py:43-57`), in DRF wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EstimatePointView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub estimate: &'a str,
    pub key: i32,
    pub description: &'a str,
    pub value: &'a str,
}

/// Port of `EstimatePointSerializer` read shape (`estimate.py:25-37`).
/// `estimate` / `workspace` / `project` are read-only on write
/// (`estimate.py:37`) — set by the view from the URL estimate
/// (`views/estimate.py:217-227`) — but rendered on read.
pub fn estimate_point_to_representation<'a>(
    row: &'a EstimatePointRow<'a>,
) -> EstimatePointView<'a> {
    EstimatePointView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        deleted_at: row.deleted_at,
        project: row.project,
        workspace: row.workspace,
        estimate: row.estimate,
        key: row.key,
        description: row.description,
        value: row.value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_string(v: &serde_json::Value) -> String {
        serde_json::to_string(v).expect("error body serializes")
    }

    #[test]
    fn state_meta_field_sets_match_python() {
        assert_eq!(
            STATE_READ_ONLY_FIELDS,
            [
                "id",
                "created_by",
                "updated_by",
                "created_at",
                "updated_at",
                "workspace",
                "project",
                "deleted_at",
                "slug"
            ]
        );
        assert_eq!(
            STATE_WRITABLE_FIELDS,
            [
                "name",
                "description",
                "color",
                "sequence",
                "group",
                "is_triage",
                "default",
                "external_source",
                "external_id"
            ]
        );
        // StateLiteSerializer: exactly these four, all read-only
        // (state.py:44-55).
        assert_eq!(STATE_LITE_FIELDS, ["id", "name", "color", "group"]);
    }

    #[test]
    fn estimate_meta_field_sets_match_python() {
        assert_eq!(
            ESTIMATE_READ_ONLY_FIELDS,
            ["workspace", "project", "deleted_at"]
        );
        assert_eq!(
            ESTIMATE_POINT_READ_ONLY_FIELDS,
            ["estimate", "workspace", "project"]
        );
        assert_eq!(
            ESTIMATE_POINT_WRITABLE_FIELDS,
            ["key", "description", "value"]
        );
    }

    #[test]
    fn state_triage_rejected_byte_identical() {
        // FX-WORKFLOW-SER triage_rejected + contract
        // test_create_conflicts: exact body, exact message.
        let (flip, err) = state_validate(None, Some("triage"));
        assert!(!flip);
        let err = err.expect("triage group is rejected");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Cannot create triage state"]}"#
        );
    }

    #[test]
    fn state_validate_ok_arms_nothing() {
        let (flip, err) = state_validate(None, Some("backlog"));
        assert!(!flip);
        assert!(err.is_none());
        // Explicit false does not arm the flip either.
        let (flip, err) = state_validate(Some(false), Some("backlog"));
        assert!(!flip);
        assert!(err.is_none());
    }

    #[test]
    fn state_default_flip_armed_only_on_true() {
        // data.get("default", False) truthiness (state.py:20).
        assert!(state_default_flip_requested(Some(true)));
        assert!(!state_default_flip_requested(Some(false)));
        assert!(!state_default_flip_requested(None));
        let (flip, err) = state_validate(Some(true), Some("backlog"));
        assert!(flip);
        assert!(err.is_none());
    }

    #[test]
    fn state_triage_rejection_still_arms_flip() {
        // BUG-4a: the UPDATE runs before the triage check, so a
        // rejected payload still clears its siblings' defaults.
        let (flip, err) = state_validate(Some(true), Some("triage"));
        assert!(flip);
        assert_eq!(
            body_string(&err.expect("triage group is rejected")),
            r#"{"non_field_errors":["Cannot create triage state"]}"#
        );
    }

    #[test]
    fn state_group_match_is_exact() {
        assert!(!state_group_rejected(None));
        assert!(!state_group_rejected(Some("backlog")));
        assert!(!state_group_rejected(Some("Triage")));
        assert!(!state_group_rejected(Some("triage ")));
        assert!(state_group_rejected(Some("triage")));
    }

    #[test]
    fn state_default_flip_sql_carries_manager_scoping() {
        // State.objects = StateManager: soft-delete filter plus the
        // triage exclusion, so triage and soft-deleted siblings keep
        // their default. Django quotes "default" (reserved).
        assert_eq!(
            STATE_DEFAULT_FLIP_SQL,
            "UPDATE \"states\" SET \"default\" = false \
             WHERE \"project_id\" = $1 \
             AND \"deleted_at\" IS NULL \
             AND NOT (\"group\" = 'triage')"
        );
    }

    #[test]
    fn estimate_point_empty_dict_rejected_byte_identical() {
        // FX-WORKFLOW-SER empty_item (estimate.py:27-28).
        let err = estimate_point_validate(true, None).expect("empty payload is rejected");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Estimate points are required"]}"#
        );
    }

    #[test]
    fn estimate_point_value_length_rule() {
        // FX-WORKFLOW-SER value_too_long (estimate.py:29-31).
        let over: String = "1".repeat(21);
        let err = estimate_point_validate(false, Some(&over)).expect("21 chars is rejected");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Value can't be more than 20 characters"]}"#
        );
        // Boundary: exactly 20 passes, empty passes, absent passes.
        assert!(estimate_point_validate(false, Some(&"1".repeat(20))).is_none());
        assert!(estimate_point_validate(false, Some("")).is_none());
        assert!(estimate_point_validate(false, None).is_none());
    }

    #[test]
    fn estimate_point_length_counts_code_points_not_bytes() {
        // Python len() counts code points: 21 × U+00E9 (2 bytes each
        // in UTF-8) is over the limit despite being 42 bytes.
        let over: String = "é".repeat(21);
        assert_eq!(over.len(), 42);
        assert!(estimate_point_validate(false, Some(&over)).is_some());
        // 7 emoji (28 bytes, 7 code points) pass.
        let ok = "🎯".repeat(7);
        assert_eq!(ok.len(), 28);
        assert!(estimate_point_validate(false, Some(&ok)).is_none());
    }

    #[test]
    fn estimate_create_context_requires_both_keys() {
        // estimate.py:20-21 raises KeyError on a missing key.
        let ctx = EstimateCreateContext::require(Some("ws-1"), Some("pj-1")).expect("both present");
        assert_eq!(ctx.workspace_id, "ws-1");
        assert_eq!(ctx.project_id, "pj-1");
        assert_eq!(
            EstimateCreateContext::require(None, Some("pj-1")).expect_err("workspace missing"),
            "workspace"
        );
        assert_eq!(
            EstimateCreateContext::require(Some("ws-1"), None).expect_err("project missing"),
            "project"
        );
    }

    #[test]
    fn state_create_ok_golden_byte_identical() {
        // FX-WORKFLOW-SER create_ok input shape; datetimes cross as
        // pre-rendered strings, UUIDs as strings.
        let row = StateRow {
            id: "11111111-1111-1111-1111-111111111111",
            created_at: "2026-09-29T12:00:00Z",
            updated_at: "2026-09-29T12:00:00Z",
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "22222222-2222-2222-2222-222222222222",
            workspace: "33333333-3333-3333-3333-333333333333",
            name: "CT State",
            description: "",
            color: "#00ff00",
            slug: "ct-state",
            sequence: 1000.0,
            group: "backlog",
            is_triage: false,
            default: false,
            external_source: None,
            external_id: None,
        };
        let rendered =
            serde_json::to_string(&state_to_representation(&row)).expect("view serializes");
        assert_eq!(
            rendered,
            concat!(
                r#"{"id":"11111111-1111-1111-1111-111111111111","#,
                r#""created_at":"2026-09-29T12:00:00Z","updated_at":"2026-09-29T12:00:00Z","#,
                r#""created_by":null,"updated_by":null,"deleted_at":null,"#,
                r#""project":"22222222-2222-2222-2222-222222222222","#,
                r#""workspace":"33333333-3333-3333-3333-333333333333","#,
                r##""name":"CT State","description":"","color":"#00ff00","slug":"ct-state","##,
                r#""sequence":1000.0,"group":"backlog","is_triage":false,"default":false,"#,
                r#""external_source":null,"external_id":null}"#
            )
        );
        // Float rendering keeps the `.0`: DRF emits 1000.0, not 1000.
        assert!(rendered.contains(r#""sequence":1000.0"#));
    }

    #[test]
    fn state_lite_golden_shape_and_order() {
        let row = StateLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            name: "Backlog",
            color: "#60646C",
            group: "backlog",
        };
        let rendered =
            serde_json::to_string(&state_lite_to_representation(&row)).expect("view serializes");
        assert_eq!(
            rendered,
            r##"{"id":"11111111-1111-1111-1111-111111111111","name":"Backlog","color":"#60646C","group":"backlog"}"##
        );
    }

    #[test]
    fn estimate_point_ok_golden_byte_identical() {
        // FX-WORKFLOW-SER ok input {description:"", key:1, value:"1"};
        // estimate/workspace/project read back on read.
        let row = EstimatePointRow {
            id: "44444444-4444-4444-4444-444444444444",
            created_at: "2026-09-29T12:00:00Z",
            updated_at: "2026-09-29T12:00:00Z",
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "22222222-2222-2222-2222-222222222222",
            workspace: "33333333-3333-3333-3333-333333333333",
            estimate: "55555555-5555-5555-5555-555555555555",
            key: 1,
            description: "",
            value: "1",
        };
        let rendered = serde_json::to_string(&estimate_point_to_representation(&row))
            .expect("view serializes");
        assert_eq!(
            rendered,
            concat!(
                r#"{"id":"44444444-4444-4444-4444-444444444444","#,
                r#""created_at":"2026-09-29T12:00:00Z","updated_at":"2026-09-29T12:00:00Z","#,
                r#""created_by":null,"updated_by":null,"deleted_at":null,"#,
                r#""project":"22222222-2222-2222-2222-222222222222","#,
                r#""workspace":"33333333-3333-3333-3333-333333333333","#,
                r#""estimate":"55555555-5555-5555-5555-555555555555","#,
                r#""key":1,"description":"","value":"1"}"#
            )
        );
    }

    #[test]
    fn estimate_view_renders_type_key() {
        let row = EstimateRow {
            id: "55555555-5555-5555-5555-555555555555",
            created_at: "2026-09-29T12:00:00Z",
            updated_at: "2026-09-29T12:00:00Z",
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "22222222-2222-2222-2222-222222222222",
            workspace: "33333333-3333-3333-3333-333333333333",
            name: "CT Est",
            description: "",
            estimate_type: "points",
            last_used: false,
        };
        let rendered =
            serde_json::to_string(&estimate_to_representation(&row)).expect("view serializes");
        assert!(rendered.contains(r#""type":"points""#));
        assert!(rendered.contains(r#""last_used":false"#));
    }
}
