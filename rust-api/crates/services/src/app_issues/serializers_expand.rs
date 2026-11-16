#![forbid(unsafe_code)]

//! Nested-expansion serializer kernels for app:issues `?expand=` (FX-ISS-14).
//!
//! Ports the lite/nested classes the `DynamicBaseSerializer` expansion
//! mapper renders that are not already ported elsewhere:
//!
//! * app `IssueRelationSerializer` (`app/serializers/issue.py:590-628`)
//! * app `RelatedIssueSerializer` (`app/serializers/issue.py:630-665`)
//! * app `IntakeIssueLiteSerializer` (`app/serializers/intake.py:120-125`)
//!
//! `LabelSerializer`, `UserLiteSerializer`, `ProjectLiteSerializer`,
//! `WorkspaceLiteSerializer`, `StateLiteSerializer`, `IssueLiteSerializer`,
//! `IssueLinkLiteSerializer`, `IssueReactionLiteSerializer` and
//! `IssueAttachmentLiteSerializer` already have kernels in this crate and
//! are reused by the caller, not re-ported here.

use serde::Serialize;

/// App `IssueRelationSerializer` wire keys (`issue.py:590-628`), in
/// `Meta.fields` order minus the write-only `assignee_ids`.
///
/// `state_id` reads the *related* issue's `state.id` through a declared
/// `UUIDField(source="related_issue.state.id", read_only=True)`; a null
/// state raises `AttributeError` inside DRF's `get_attribute`, and because
/// the field is `required=False` that becomes `SkipField` — the key is
/// omitted, not nulled.
pub const ISSUE_RELATION_FIELDS: [&str; 11] = [
    "id",
    "project_id",
    "sequence_id",
    "relation_type",
    "name",
    "state_id",
    "priority",
    "created_by",
    "created_at",
    "updated_at",
    "updated_by",
];

/// App `RelatedIssueSerializer` wire keys (`issue.py:630-665`): same shape as
/// [`ISSUE_RELATION_FIELDS`] but sourced from the *related* row's `issue`
/// side, and with `updated_by` / `updated_at` swapped in `Meta.fields`.
pub const RELATED_ISSUE_FIELDS: [&str; 11] = [
    "id",
    "project_id",
    "sequence_id",
    "relation_type",
    "name",
    "state_id",
    "priority",
    "created_by",
    "created_at",
    "updated_by",
    "updated_at",
];

/// One `IssueRelation` row joined to the counterpart issue
/// (`db/models/issue.py:396-402`). The `*_id` columns are pre-resolved
/// string UUIDs; datetimes are pre-rendered DRF strings (the request user's
/// zone); `counterpart_state_id` is `None` when the counterpart issue has no
/// state (the DRF `SkipField` arm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRelationExpandRow<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub sequence_id: i32,
    pub relation_type: &'a str,
    pub name: &'a str,
    pub state_id: Option<&'a str>,
    pub priority: &'a str,
    pub created_by: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub updated_by: Option<&'a str>,
}

/// `IssueRelationSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueRelationExpandView<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub sequence_id: i32,
    pub relation_type: &'a str,
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<&'a str>,
    pub priority: &'a str,
    pub created_by: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub updated_by: Option<&'a str>,
}

/// Port of app `IssueRelationSerializer` (`issue.py:590-628`).
pub fn issue_relation_expand_to_representation<'a>(
    row: &'a IssueRelationExpandRow<'a>,
) -> IssueRelationExpandView<'a> {
    IssueRelationExpandView {
        id: row.id,
        project_id: row.project_id,
        sequence_id: row.sequence_id,
        relation_type: row.relation_type,
        name: row.name,
        state_id: row.state_id,
        priority: row.priority,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
    }
}

/// `RelatedIssueSerializer.to_representation` output, in wire order
/// (`updated_by` before `updated_at`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedIssueExpandView<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub sequence_id: i32,
    pub relation_type: &'a str,
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<&'a str>,
    pub priority: &'a str,
    pub created_by: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_by: Option<&'a str>,
    pub updated_at: &'a str,
}

/// Port of app `RelatedIssueSerializer` (`issue.py:630-665`). Shares
/// [`IssueRelationExpandRow`] with the relation kernel — both read a
/// relation row plus its counterpart issue.
pub fn related_issue_expand_to_representation<'a>(
    row: &'a IssueRelationExpandRow<'a>,
) -> RelatedIssueExpandView<'a> {
    RelatedIssueExpandView {
        id: row.id,
        project_id: row.project_id,
        sequence_id: row.sequence_id,
        relation_type: row.relation_type,
        name: row.name,
        state_id: row.state_id,
        priority: row.priority,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_by: row.updated_by,
        updated_at: row.updated_at,
    }
}

/// App `IntakeIssueLiteSerializer` wire keys (`intake.py:120-125`), in
/// `Meta.fields` order. Every field is read-only.
pub const INTAKE_ISSUE_LITE_FIELDS: [&str; 5] =
    ["id", "status", "duplicate_to", "snoozed_till", "source"];

/// An `IntakeIssue` row for lite rendering (`db/models/intake.py:50-70`):
/// `status` is a non-null integer choice; `duplicate_to` / `source` are
/// nullable; `snoozed_till` is pre-rendered in the request user's zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeIssueLiteRow<'a> {
    pub id: &'a str,
    pub status: i32,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// `IntakeIssueLiteSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntakeIssueLiteView<'a> {
    pub id: &'a str,
    pub status: i32,
    pub duplicate_to: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub source: Option<&'a str>,
}

/// Port of app `IntakeIssueLiteSerializer` (`intake.py:120-125`).
pub fn intake_issue_lite_to_representation<'a>(
    row: &'a IntakeIssueLiteRow<'a>,
) -> IntakeIssueLiteView<'a> {
    IntakeIssueLiteView {
        id: row.id,
        status: row.status,
        duplicate_to: row.duplicate_to,
        snoozed_till: row.snoozed_till,
        source: row.source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialized_keys<T: Serialize>(value: &T) -> Vec<String> {
        let text = serde_json::to_string(value).expect("serializes");
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&text).expect("object");
        map.keys().cloned().collect()
    }

    #[test]
    fn relation_keys_match_meta_order() {
        let row = IssueRelationExpandRow {
            id: "r",
            project_id: "p",
            sequence_id: 2,
            relation_type: "relates_to",
            name: "I2",
            state_id: Some("s"),
            priority: "urgent",
            created_by: Some("u"),
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            updated_by: Some("u"),
        };
        assert_eq!(
            serialized_keys(&issue_relation_expand_to_representation(&row)),
            ISSUE_RELATION_FIELDS
        );
        assert_eq!(
            serialized_keys(&related_issue_expand_to_representation(&row)),
            RELATED_ISSUE_FIELDS
        );
    }

    #[test]
    fn relation_null_state_omits_state_id() {
        let row = IssueRelationExpandRow {
            id: "r",
            project_id: "p",
            sequence_id: 2,
            relation_type: "relates_to",
            name: "I2",
            state_id: None,
            priority: "urgent",
            created_by: None,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            updated_by: None,
        };
        let keys = serialized_keys(&issue_relation_expand_to_representation(&row));
        assert!(!keys.iter().any(|key| key == "state_id"));
    }

    #[test]
    fn intake_lite_keys_match_meta_order() {
        let row = IntakeIssueLiteRow {
            id: "i",
            status: -2,
            duplicate_to: None,
            snoozed_till: None,
            source: None,
        };
        assert_eq!(
            serialized_keys(&intake_issue_lite_to_representation(&row)),
            INTAKE_ISSUE_LITE_FIELDS
        );
    }
}
