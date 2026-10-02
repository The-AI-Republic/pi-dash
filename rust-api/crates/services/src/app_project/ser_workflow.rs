#![forbid(unsafe_code)]

//! State / estimate serializers: read shapes plus the two validators.
//!
//! Port of `apps/api/pi_dash/app/serializers/state.py:12-41` and
//! `apps/api/pi_dash/app/serializers/estimate.py:13-41`:
//!
//! * `state.py:12-30` (`StateSerializer` declared `order` + `Meta`)
//! * `state.py:31-34` (`StateSerializer.validate` — triage veto only)
//! * `state.py:37-41` (`StateLiteSerializer`, all four fields read-only)
//! * `estimate.py:13-17` (`EstimateSerializer.Meta`)
//! * `estimate.py:20-32` (`EstimatePointSerializer.validate` + `Meta`)
//! * `estimate.py:35-41` (`EstimateReadSerializer` — nested read-only points)
//!
//! These are pure shapes plus validators: each `*_to_representation`
//! takes a row borrowed from the caller and returns a `serde::Serialize`
//! view whose fields are the live DRF wire fields in DRF order. UUID and
//! FK primary keys render as strings; a null FK renders `null`. Datetimes
//! cross this boundary already rendered as DRF `iso-8601` strings —
//! formatting owns to the DB edge, so rendering here is a byte-exact
//! passthrough.
//!
//! Key order follows the live `__all__` recording (FX-APROJ-03): declared
//! fields first (`id`, then `points` on the read serializer), then concrete
//! fields in definition order, then forward relations in definition order.
//!
//! Ported quirks (translate as-is, do not fix):
//! * The declared `order` field (`state.py:13`) has no model column: it is
//!   `SkipField`'d on read, so the read shape has 9 keys. The list view
//!   injects `order` per row instead (`app/views/state/base.py:92-96`).
//! * `StateSerializer.Meta.read_only_fields` (`state.py:29`) names
//!   `workspace`/`project`, which are not in `Meta.fields` (it carries
//!   `project_id`/`workspace_id`) — the setting is inert, pinned as
//!   declared.
//! * The empty-payload branch of `EstimatePointSerializer.validate`
//!   (`estimate.py:22-23`) is unreachable via `is_valid()`: the field-level
//!   value-required check fails first (fixture note). It is ported as-is;
//!   [`ESTIMATE_POINT_VALUE_REQUIRED_MESSAGE`] pins the field-level body
//!   the goldens record.
//!
//! Out of scope for this module (owned elsewhere): `State.save` sequence
//! overwrite + slug (`db/models/state.py:131-139`, models layer,
//! PIDASHCONV-567); `WorkspaceEstimateSerializer` (`estimate.py:44-50`,
//! shared serializers, PIDASHCONV-566); the `order` list injection and all
//! view error/status bodies (handlers, PIDASHCONV-574).

use serde::Serialize;

/// `StateSerializer.Meta.fields` (`state.py:17-28`), in declared order.
pub const STATE_FIELDS: [&str; 10] = [
    "id",
    "project_id",
    "workspace_id",
    "name",
    "color",
    "group",
    "default",
    "description",
    "sequence",
    "order",
];

/// `StateSerializer` read shape: [`STATE_FIELDS`] minus the declared
/// `order`, which is `SkipField`'d (no model column, fixture state_note).
pub const STATE_READ_KEYS: [&str; 9] = [
    "id",
    "project_id",
    "workspace_id",
    "name",
    "color",
    "group",
    "default",
    "description",
    "sequence",
];

/// `StateSerializer.Meta.read_only_fields` (`state.py:29`), as declared.
/// Inert: neither name is in `Meta.fields` (it carries `project_id` /
/// `workspace_id` instead).
pub const STATE_READ_ONLY_FIELDS: [&str; 2] = ["workspace", "project"];

/// `StateLiteSerializer.Meta.fields` (`state.py:40`); every entry is also
/// in `read_only_fields` (`state.py:41`). Consumed by D-26/D-32 serializers.
pub const STATE_LITE_FIELDS: [&str; 4] = ["id", "name", "color", "group"];

/// `EstimateSerializer.Meta.read_only_fields` (`estimate.py:17`).
/// `id` is additionally read-only as the declared primary key
/// (`serializers/base.py`).
pub const ESTIMATE_READ_ONLY_FIELDS: [&str; 2] = ["workspace", "project"];

/// `EstimatePointSerializer.Meta.read_only_fields` (`estimate.py:32`).
/// `id` is additionally read-only as the declared primary key.
pub const ESTIMATE_POINT_READ_ONLY_FIELDS: [&str; 3] = ["estimate", "workspace", "project"];

/// `EstimateReadSerializer.Meta.read_only_fields` (`estimate.py:41`).
/// `points` is additionally declared `read_only=True` (`estimate.py:36`).
pub const ESTIMATE_READ_READ_ONLY_FIELDS: [&str; 3] = ["points", "name", "description"];

/// Rejection for `group == "triage"` (`state.py:32-33`, exact match on
/// `StateGroup.TRIAGE.value`). The message says "create" but `validate`
/// also runs on update — ported as-is.
pub const TRIAGE_REJECTED_MESSAGE: &str = "Cannot create triage state";

/// Rejection for an empty validated payload (`estimate.py:22-23` — falsy
/// data dict rejected before any field check).
pub const ESTIMATE_POINTS_REQUIRED_MESSAGE: &str = "Estimate points are required";

/// Rejection for an over-long point value (`estimate.py:25-26` — only
/// when `value` is truthy AND longer than 20; an empty value passes).
pub const ESTIMATE_POINT_VALUE_TOO_LONG_MESSAGE: &str = "Value can't be more than 20 characters";

/// Field-level body for a missing point value (DRF `CharField.required`,
/// not app code): `is_valid()` on `{}` or on a dict without `value`
/// fails here before `validate()` ever runs (fixture point_validate note).
pub const ESTIMATE_POINT_VALUE_REQUIRED_MESSAGE: &str = "This field is required.";

/// Maximum point-value length in code points (`estimate.py:25`).
/// Python `len(value)` counts code points, so this port uses
/// `.chars().count()`, never `.len()` (UTF-8 bytes).
pub const ESTIMATE_POINT_VALUE_MAX_CHARS: usize = 20;

/// The triage group value (`db/models/state.py:22`, `StateGroup.TRIAGE`).
pub const STATE_GROUP_TRIAGE: &str = "triage";

/// DRF renders a bare-string `ValidationError` as
/// `{"non_field_errors": ["<msg>"]}` with status 400.
pub fn non_field_errors(message: &str) -> serde_json::Value {
    serde_json::json!({ "non_field_errors": [message] })
}

/// DRF renders a field-level `ValidationError` as
/// `{"<field>": ["<msg>"]}` with status 400.
pub fn field_errors(field: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ field: [message] })
}

/// Mirrors `attrs.get("group") == StateGroup.TRIAGE.value`
/// (`state.py:32`): exact match only.
pub fn state_group_rejected(group: Option<&str>) -> bool {
    group == Some(STATE_GROUP_TRIAGE)
}

/// Port of `StateSerializer.validate` (`state.py:31-34`): the triage veto
/// only. Unlike the v1 API serializer, the app serializer performs no
/// default-flip here — defaults flip in the `mark-default` view action
/// (`app/views/state/base.py:113-119`).
pub fn state_validate(group: Option<&str>) -> Option<serde_json::Value> {
    if state_group_rejected(group) {
        return Some(non_field_errors(TRIAGE_REJECTED_MESSAGE));
    }
    None
}

/// Port of `EstimatePointSerializer.validate` (`estimate.py:21-27`).
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

/// The field-level body `is_valid()` returns for `{}` or a dict without
/// `value` (`{"value": ["This field is required."]}`, fixture
/// point_validate empty_dict / missing_value).
pub fn estimate_point_value_required_errors() -> serde_json::Value {
    field_errors("value", ESTIMATE_POINT_VALUE_REQUIRED_MESSAGE)
}

/// A database row for the state read shape. `sequence` is a Django
/// `FloatField` (`db/models/state.py:98`); DRF renders it with a `.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct StateRow<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
    pub default: bool,
    pub description: &'a str,
    pub sequence: f64,
}

/// `StateSerializer.to_representation` output (`state.py:12-30`), in DRF
/// wire order: the 9 [`STATE_READ_KEYS`] — the declared `order` is
/// `SkipField`'d and never renders here.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateView<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
    pub default: bool,
    pub description: &'a str,
    pub sequence: f64,
}

/// Port of the `StateSerializer` read shape (`state.py:12-30`).
pub fn state_to_representation<'a>(row: &'a StateRow<'a>) -> StateView<'a> {
    StateView {
        id: row.id,
        project_id: row.project_id,
        workspace_id: row.workspace_id,
        name: row.name,
        color: row.color,
        group: row.group,
        default: row.default,
        description: row.description,
        sequence: row.sequence,
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

/// `StateLiteSerializer.to_representation` output (`state.py:37-41`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
    pub group: &'a str,
}

/// Port of the `StateLiteSerializer` read shape (`state.py:37-41`).
pub fn state_lite_to_representation<'a>(row: &'a StateLiteRow<'a>) -> StateLiteView<'a> {
    StateLiteView {
        id: row.id,
        name: row.name,
        color: row.color,
        group: row.group,
    }
}

/// A database row for the estimate read shape. Datetimes are pre-rendered
/// DRF `iso-8601` strings; ids and FKs are rendered UUID strings with
/// `null` for absent relations.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimateRow<'a> {
    pub id: &'a str,
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

/// `EstimateSerializer.to_representation` output (`estimate.py:13-17`,
/// `fields = "__all__"` over `db/models/estimate.py:18-22`), in DRF wire
/// order: declared `id`, then concrete fields, then forward relations.
/// The model field is `type`; it serializes under that key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EstimateView<'a> {
    pub id: &'a str,
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

/// Port of the `EstimateSerializer` read shape (`estimate.py:13-17`).
/// `workspace` / `project` are read-only on write (`estimate.py:17`) —
/// set by the view from the URL — but rendered on read.
pub fn estimate_to_representation<'a>(row: &'a EstimateRow<'a>) -> EstimateView<'a> {
    EstimateView {
        id: row.id,
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

/// A database row for the estimate-point read shape.
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

/// `EstimatePointSerializer.to_representation` output
/// (`estimate.py:20-32`, `fields = "__all__"` over
/// `db/models/estimate.py:43-47`), in DRF wire order.
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

/// Port of the `EstimatePointSerializer` read shape (`estimate.py:20-32`).
/// `estimate` / `workspace` / `project` are read-only on write
/// (`estimate.py:32`) — set by the view from the URL estimate (bulk
/// create writes them via the ORM) — but rendered on read.
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

/// `EstimateReadSerializer.to_representation` output (`estimate.py:35-41`):
/// the estimate shape with the nested read-only `points` second, right
/// after the declared `id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EstimateReadView<'a> {
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

/// Port of the `EstimateReadSerializer` read shape (`estimate.py:35-41`).
/// Each point renders through [`estimate_point_to_representation`],
/// mirroring `points = EstimatePointSerializer(read_only=True, many=True)`.
pub fn estimate_read_to_representation<'a>(
    row: &'a EstimateRow<'a>,
    point_rows: &'a [EstimatePointRow<'a>],
) -> EstimateReadView<'a> {
    EstimateReadView {
        id: row.id,
        points: point_rows
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

#[cfg(test)]
mod tests {
    use super::*;

    fn body_string(v: &serde_json::Value) -> String {
        serde_json::to_string(v).expect("error body serializes")
    }

    #[test]
    fn state_meta_field_sets_match_python() {
        assert_eq!(
            STATE_FIELDS,
            [
                "id",
                "project_id",
                "workspace_id",
                "name",
                "color",
                "group",
                "default",
                "description",
                "sequence",
                "order"
            ]
        );
        // Read shape drops the declared `order` (SkipField, no column).
        assert_eq!(
            STATE_READ_KEYS,
            [
                "id",
                "project_id",
                "workspace_id",
                "name",
                "color",
                "group",
                "default",
                "description",
                "sequence"
            ]
        );
        // Pinned as declared (state.py:29); inert — neither name is in
        // Meta.fields.
        assert_eq!(STATE_READ_ONLY_FIELDS, ["workspace", "project"]);
        // StateLiteSerializer: exactly these four, all read-only
        // (state.py:37-41).
        assert_eq!(STATE_LITE_FIELDS, ["id", "name", "color", "group"]);
    }

    #[test]
    fn estimate_meta_field_sets_match_python() {
        assert_eq!(ESTIMATE_READ_ONLY_FIELDS, ["workspace", "project"]);
        assert_eq!(
            ESTIMATE_POINT_READ_ONLY_FIELDS,
            ["estimate", "workspace", "project"]
        );
        assert_eq!(
            ESTIMATE_READ_READ_ONLY_FIELDS,
            ["points", "name", "description"]
        );
    }

    #[test]
    fn state_golden_byte_identical() {
        // FX-APROJ-03 state.data; 9 keys, declared order, no `order`.
        let row = StateRow {
            id: "df077c5d-d6d2-4265-a706-1255887b90bb",
            project_id: "8ddb0885-215e-4ae3-a772-bfa2b42d4295",
            workspace_id: "eec47df5-8eb2-4aae-9093-320975d58cd3",
            name: "S4e33a3a3",
            color: "#111111",
            group: "unstarted",
            default: false,
            description: "",
            sequence: 45000.0,
        };
        let rendered =
            serde_json::to_string(&state_to_representation(&row)).expect("view serializes");
        assert_eq!(
            rendered,
            r##"{"id":"df077c5d-d6d2-4265-a706-1255887b90bb","project_id":"8ddb0885-215e-4ae3-a772-bfa2b42d4295","workspace_id":"eec47df5-8eb2-4aae-9093-320975d58cd3","name":"S4e33a3a3","color":"#111111","group":"unstarted","default":false,"description":"","sequence":45000.0}"##
        );
        // Float rendering keeps the `.0`: DRF emits 45000.0, not 45000.
        assert!(rendered.contains(r#""sequence":45000.0"#));
        assert!(!rendered.contains("order"));
    }

    #[test]
    fn state_lite_golden_byte_identical() {
        // FX-APROJ-03 state_lite.data.
        let row = StateLiteRow {
            id: "df077c5d-d6d2-4265-a706-1255887b90bb",
            name: "S4e33a3a3",
            color: "#111111",
            group: "unstarted",
        };
        let rendered =
            serde_json::to_string(&state_lite_to_representation(&row)).expect("view serializes");
        assert_eq!(
            rendered,
            r##"{"id":"df077c5d-d6d2-4265-a706-1255887b90bb","name":"S4e33a3a3","color":"#111111","group":"unstarted"}"##
        );
    }

    #[test]
    fn state_triage_veto_byte_identical() {
        // FX-APROJ-03 state_validate_triage_veto: exact body, exact message.
        let err = state_validate(Some("triage")).expect("triage group is rejected");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Cannot create triage state"]}"#
        );
    }

    #[test]
    fn state_validate_ok_vectors() {
        // FX-APROJ-03 state_validate_ok.
        assert!(state_validate(Some("unstarted")).is_none());
        assert!(state_validate(None).is_none());
        // Exact match only (state.py:32): near-misses pass.
        assert!(state_validate(Some("Triage")).is_none());
        assert!(state_validate(Some("triage ")).is_none());
        assert!(!state_group_rejected(None));
        assert!(state_group_rejected(Some("triage")));
    }

    #[test]
    fn estimate_golden_byte_identical() {
        // FX-APROJ-03 estimate.data; 12 keys in live `__all__` order.
        let row = EstimateRow {
            id: "8abc7daa-8714-49c0-a429-638f15a2c214",
            created_at: "2026-10-02T22:13:45.023126Z",
            updated_at: "2026-10-02T22:13:45.023134Z",
            deleted_at: None,
            name: "E4e33a3a3",
            description: "",
            estimate_type: "points",
            last_used: false,
            created_by: None,
            updated_by: None,
            project: "8ddb0885-215e-4ae3-a772-bfa2b42d4295",
            workspace: "eec47df5-8eb2-4aae-9093-320975d58cd3",
        };
        let rendered =
            serde_json::to_string(&estimate_to_representation(&row)).expect("view serializes");
        assert_eq!(
            rendered,
            concat!(
                r#"{"id":"8abc7daa-8714-49c0-a429-638f15a2c214","#,
                r#""created_at":"2026-10-02T22:13:45.023126Z","#,
                r#""updated_at":"2026-10-02T22:13:45.023134Z","deleted_at":null,"#,
                r#""name":"E4e33a3a3","description":"","type":"points","last_used":false,"#,
                r#""created_by":null,"updated_by":null,"#,
                r#""project":"8ddb0885-215e-4ae3-a772-bfa2b42d4295","#,
                r#""workspace":"eec47df5-8eb2-4aae-9093-320975d58cd3"}"#
            )
        );
    }

    #[test]
    fn estimate_point_golden_byte_identical() {
        // FX-APROJ-03 estimate_point.data; 12 keys in live `__all__` order.
        let row = EstimatePointRow {
            id: "67b19688-40c5-47e0-b470-d5b25fbb467c",
            created_at: "2026-10-02T22:13:45.025804Z",
            updated_at: "2026-10-02T22:13:45.025810Z",
            deleted_at: None,
            key: 1,
            description: "one",
            value: "1",
            created_by: None,
            updated_by: None,
            project: "8ddb0885-215e-4ae3-a772-bfa2b42d4295",
            workspace: "eec47df5-8eb2-4aae-9093-320975d58cd3",
            estimate: "8abc7daa-8714-49c0-a429-638f15a2c214",
        };
        let rendered = serde_json::to_string(&estimate_point_to_representation(&row))
            .expect("view serializes");
        assert_eq!(
            rendered,
            concat!(
                r#"{"id":"67b19688-40c5-47e0-b470-d5b25fbb467c","#,
                r#""created_at":"2026-10-02T22:13:45.025804Z","#,
                r#""updated_at":"2026-10-02T22:13:45.025810Z","deleted_at":null,"#,
                r#""key":1,"description":"one","value":"1","#,
                r#""created_by":null,"updated_by":null,"#,
                r#""project":"8ddb0885-215e-4ae3-a772-bfa2b42d4295","#,
                r#""workspace":"eec47df5-8eb2-4aae-9093-320975d58cd3","#,
                r#""estimate":"8abc7daa-8714-49c0-a429-638f15a2c214"}"#
            )
        );
    }

    fn fixture_estimate_row() -> EstimateRow<'static> {
        EstimateRow {
            id: "8abc7daa-8714-49c0-a429-638f15a2c214",
            created_at: "2026-10-02T22:13:45.023126Z",
            updated_at: "2026-10-02T22:13:45.023134Z",
            deleted_at: None,
            name: "E4e33a3a3",
            description: "",
            estimate_type: "points",
            last_used: false,
            created_by: None,
            updated_by: None,
            project: "8ddb0885-215e-4ae3-a772-bfa2b42d4295",
            workspace: "eec47df5-8eb2-4aae-9093-320975d58cd3",
        }
    }

    fn fixture_point_row() -> EstimatePointRow<'static> {
        EstimatePointRow {
            id: "67b19688-40c5-47e0-b470-d5b25fbb467c",
            created_at: "2026-10-02T22:13:45.025804Z",
            updated_at: "2026-10-02T22:13:45.025810Z",
            deleted_at: None,
            key: 1,
            description: "one",
            value: "1",
            created_by: None,
            updated_by: None,
            project: "8ddb0885-215e-4ae3-a772-bfa2b42d4295",
            workspace: "eec47df5-8eb2-4aae-9093-320975d58cd3",
            estimate: "8abc7daa-8714-49c0-a429-638f15a2c214",
        }
    }

    #[test]
    fn estimate_read_golden_byte_identical() {
        // FX-APROJ-03 estimate_read.data: estimate shape with nested
        // `points` second, right after `id`.
        let row = fixture_estimate_row();
        let points = [fixture_point_row()];
        let rendered = serde_json::to_string(&estimate_read_to_representation(&row, &points))
            .expect("view serializes");
        let point = concat!(
            r#"{"id":"67b19688-40c5-47e0-b470-d5b25fbb467c","#,
            r#""created_at":"2026-10-02T22:13:45.025804Z","#,
            r#""updated_at":"2026-10-02T22:13:45.025810Z","deleted_at":null,"#,
            r#""key":1,"description":"one","value":"1","#,
            r#""created_by":null,"updated_by":null,"#,
            r#""project":"8ddb0885-215e-4ae3-a772-bfa2b42d4295","#,
            r#""workspace":"eec47df5-8eb2-4aae-9093-320975d58cd3","#,
            r#""estimate":"8abc7daa-8714-49c0-a429-638f15a2c214"}"#
        );
        let expected = format!(
            concat!(
                r#"{{"id":"8abc7daa-8714-49c0-a429-638f15a2c214","points":[{}],"#,
                r#""created_at":"2026-10-02T22:13:45.023126Z","#,
                r#""updated_at":"2026-10-02T22:13:45.023134Z","deleted_at":null,"#,
                r#""name":"E4e33a3a3","description":"","type":"points","last_used":false,"#,
                r#""created_by":null,"updated_by":null,"#,
                r#""project":"8ddb0885-215e-4ae3-a772-bfa2b42d4295","#,
                r#""workspace":"eec47df5-8eb2-4aae-9093-320975d58cd3"}}"#
            ),
            point
        );
        assert_eq!(rendered, expected);
    }

    #[test]
    fn estimate_read_empty_points_shape() {
        // Bulk create with no points still renders `points: []`
        // (contract test_bulk_create_empty_points_200).
        let row = fixture_estimate_row();
        let rendered = serde_json::to_string(&estimate_read_to_representation(&row, &[]))
            .expect("view serializes");
        assert!(rendered.starts_with(
            r#"{"id":"8abc7daa-8714-49c0-a429-638f15a2c214","points":[],"created_at":"#
        ));
    }

    #[test]
    fn estimate_point_empty_dict_rejected_byte_identical() {
        // validate() level (estimate.py:22-23): ported as-is even though
        // is_valid() never reaches it (see below).
        let err =
            estimate_point_validate(true, None).expect("empty payload is rejected at validate()");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Estimate points are required"]}"#
        );
    }

    #[test]
    fn estimate_point_field_required_byte_identical() {
        // FX-APROJ-03 point_validate empty_dict + missing_value: what
        // is_valid() actually returns — the field-level check fails first.
        assert_eq!(
            body_string(&estimate_point_value_required_errors()),
            r#"{"value":["This field is required."]}"#
        );
    }

    #[test]
    fn estimate_point_value_length_rule() {
        // FX-APROJ-03 point_validate value_21_chars (estimate.py:25-26).
        let over: String = "1".repeat(21);
        let err = estimate_point_validate(false, Some(&over)).expect("21 chars is rejected");
        assert_eq!(
            body_string(&err),
            r#"{"non_field_errors":["Value can't be more than 20 characters"]}"#
        );
        // Boundary: exactly 20 passes (value_20_chars), empty passes
        // (falsy skips the check), absent passes, valid passes (ok).
        assert!(estimate_point_validate(false, Some(&"1".repeat(20))).is_none());
        assert!(estimate_point_validate(false, Some("")).is_none());
        assert!(estimate_point_validate(false, None).is_none());
        assert!(estimate_point_validate(false, Some("1")).is_none());
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
}
