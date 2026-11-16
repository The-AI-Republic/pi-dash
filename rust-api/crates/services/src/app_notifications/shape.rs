#![forbid(unsafe_code)]

//! Response shapes for app notifications (`app/serializers/notification.py`).
//!
//! Ports, as shape/serial views in the pilot `app_issues/shape.rs` style:
//!
//! * `NotificationSerializer` (`serializers/notification.py:14-22`,
//!   `fields = "__all__"`): every `Notification` column
//!   (`db/models/notification.py:14-33` over `AuditModel`,
//!   `db/mixins.py:86-90`) plus the four declared read-only fields — the
//!   nested `triggered_by_details` (`UserLiteSerializer`, `source =
//!   "triggered_by"`, `serializers/user.py:141-153`, keys `id, first_name,
//!   last_name, avatar, avatar_url, is_bot, display_name`) and the three
//!   annotation booleans `is_inbox_issue`, `is_intake_issue`,
//!   `is_mentioned_notification` (`serializers/notification.py:16-18`).
//!   Annotation values are evaluated by the list queryset
//!   (`app/views/notification/base.py:66-74`) and arrive here as plain
//!   booleans; the query layer (PIDASHCONV-299) owns their SQL.
//! * `UserNotificationPreferenceSerializer`
//!   (`serializers/notification.py:25-28`, `fields = "__all__"`): every
//!   `UserNotificationPreference` column (`db/models/notification.py:83-108`
//!   over `AuditModel`).
//!
//! Wire order is live DRF's real `__all__` order
//! (`ModelSerializer.get_default_field_names`: `[pk] + declared +
//! concrete columns + forward relations`, DRF 3.15.2 source, probed
//! per serializer below — never hand-derived): `id` first, then the
//! declared fields in declaration order, then every concrete column,
//! then every FK trailing in definition order (`created_by`,
//! `updated_by` first — they come from `UserAuditModel`, `db/mixins.py`
//! — then the model's own FKs). The contract tests pin the key *sets*
//! (`test_list_item_shape`, `test_preferences_get_shape`); the tests
//! below pin the probed order literally and assert the serialized key
//! order off the struct (serde struct order, byte-identical under
//! `preserve_order`).
//!
//! Value rendering is byte-exact passthrough: FK primary keys render as
//! UUID strings (`PrimaryKeyRelatedField`, read-only) with null FKs as
//! `null`; `data` / `message` (`JSONField(null=True)`) splice raw;
//! datetimes cross this boundary already rendered as DRF `iso-8601`
//! strings (`+00:00` rewritten to `Z`, microseconds only when nonzero) —
//! formatting owns to the DB edge (`pidash_api::serializer::render_datetime`),
//! exactly like the space lite serializers. These models have no `Decimal`
//! columns, so no decimal rendering applies.
//!
//! Null rules: `triggered_by` is `SET_NULL` (`models/notification.py:24-29`),
//! so a null FK renders `"triggered_by": null` with `"triggered_by_details":
//! null` (nested `source=` serializes the null object, never `{}`).
//! `project` (`notification.py:15`, null) and `message_stripped`
//! (`notification.py:22`, null) likewise render `null`. Writes are out of
//! scope here: the declared fields are `read_only`, so DRF silently ignores
//! them on input (`partial_update` honours only `snoozed_till`, recorded in
//! FX-NOTIF-09 for the handlers).
//!
//! Carried quirk (translate, don't redesign): `is_inbox_issue` and
//! `is_intake_issue` annotate the *same* intake `Exists` subquery
//! (`views/base.py:66-67`) — the name says inbox, the subquery is the intake
//! one. Both flags always agree; ported as-is, listed in the PR.

use serde::Serialize;

use crate::space::serializers::lite::{user_lite_to_representation, UserLiteRow, UserLiteView};

/// The `Notification` `fields = "__all__"` model body
/// (`db/models/notification.py:14-33` over `AuditModel`,
/// `db/mixins.py:86-90`), in live-DRF order (probed
/// `NotificationSerializer().fields`): `id`, the concrete columns
/// (`created_at`, `updated_at`, `deleted_at`, then the model's own
/// columns in definition order), then the forward relations trailing
/// (`created_by`, `updated_by`, `workspace`, `project`,
/// `triggered_by`, `receiver`).
pub const NOTIFICATION_ALL_FIELDS: [&str; 21] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "data",
    "entity_identifier",
    "entity_name",
    "title",
    "message",
    "message_html",
    "message_stripped",
    "sender",
    "read_at",
    "snoozed_till",
    "archived_at",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "triggered_by",
    "receiver",
];

/// The four declared read-only fields of `NotificationSerializer`
/// (`serializers/notification.py:15-18`), in declaration order: the nested
/// `triggered_by_details` first, then the three annotation booleans.
pub const NOTIFICATION_DECLARED_FIELDS: [&str; 4] = [
    "triggered_by_details",
    "is_inbox_issue",
    "is_intake_issue",
    "is_mentioned_notification",
];

/// Full `NotificationSerializer` wire order, in live-DRF order (probed
/// `NotificationSerializer().fields`): `id`, the declared fields in
/// declaration order, then the [`NOTIFICATION_ALL_FIELDS`] body. 25
/// keys, the same set as FX-NOTIF-04 `output_keys`.
pub const NOTIFICATION_WIRE_FIELDS: [&str; 25] = [
    "id",
    "triggered_by_details",
    "is_inbox_issue",
    "is_intake_issue",
    "is_mentioned_notification",
    "created_at",
    "updated_at",
    "deleted_at",
    "data",
    "entity_identifier",
    "entity_name",
    "title",
    "message",
    "message_html",
    "message_stripped",
    "sender",
    "read_at",
    "snoozed_till",
    "archived_at",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "triggered_by",
    "receiver",
];

/// The `UserNotificationPreference` `fields = "__all__"` wire keys
/// (`db/models/notification.py:83-108` over `AuditModel`), in live-DRF
/// order (probed `UserNotificationPreferenceSerializer().fields`):
/// `id`, the concrete columns (`created_at`, `updated_at`,
/// `deleted_at`, then the five boolean preference columns), then the
/// forward relations trailing (`created_by`, `updated_by`, `user`,
/// nullable `workspace` / `project`). 14 keys, the FX-NOTIF-05
/// `output_keys` set.
pub const PREFERENCE_ALL_FIELDS: [&str; 14] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "property_change",
    "state_change",
    "comment",
    "mention",
    "issue_completed",
    "created_by",
    "updated_by",
    "user",
    "workspace",
    "project",
];

/// A database row for `Notification` rendering. Datetimes are pre-rendered
/// DRF `iso-8601` strings; FK columns are UUID strings, `None` when null;
/// `data` / `message` splice raw JSON. Nullability mirrors the model:
/// `workspace` and `receiver` are non-null (`CASCADE`);
/// `project`, `triggered_by` (`SET_NULL`), `created_by`, `updated_by` are
/// nullable. The annotation booleans arrive evaluated from the queryset.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationRow<'a> {
    pub id: &'a str,
    pub triggered_by_details: Option<UserLiteRow<'a>>,
    pub is_inbox_issue: bool,
    pub is_intake_issue: bool,
    pub is_mentioned_notification: bool,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub data: Option<&'a serde_json::Value>,
    pub entity_identifier: Option<&'a str>,
    pub entity_name: &'a str,
    pub title: &'a str,
    pub message: Option<&'a serde_json::Value>,
    pub message_html: &'a str,
    pub message_stripped: Option<&'a str>,
    pub sender: &'a str,
    pub read_at: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub triggered_by: Option<&'a str>,
    pub receiver: &'a str,
}

/// `NotificationSerializer.to_representation` output
/// (`serializers/notification.py:14-22`), in [`NOTIFICATION_WIRE_FIELDS`]
/// order: `id`, the declared nests/annotations, the concrete columns,
/// then the trailing FKs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotificationView<'a> {
    pub id: &'a str,
    pub triggered_by_details: Option<UserLiteView<'a>>,
    pub is_inbox_issue: bool,
    pub is_intake_issue: bool,
    pub is_mentioned_notification: bool,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub data: Option<&'a serde_json::Value>,
    pub entity_identifier: Option<&'a str>,
    pub entity_name: &'a str,
    pub title: &'a str,
    pub message: Option<&'a serde_json::Value>,
    pub message_html: &'a str,
    pub message_stripped: Option<&'a str>,
    pub sender: &'a str,
    pub read_at: Option<&'a str>,
    pub snoozed_till: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub triggered_by: Option<&'a str>,
    pub receiver: &'a str,
}

/// Port of `NotificationSerializer` (`serializers/notification.py:14-22`).
/// A null `triggered_by` (deleted user via `SET_NULL`) renders both
/// `"triggered_by": null` and `"triggered_by_details": null`.
pub fn notification_to_representation<'a>(row: &'a NotificationRow<'a>) -> NotificationView<'a> {
    NotificationView {
        id: row.id,
        triggered_by_details: row
            .triggered_by_details
            .as_ref()
            .map(user_lite_to_representation),
        is_inbox_issue: row.is_inbox_issue,
        is_intake_issue: row.is_intake_issue,
        is_mentioned_notification: row.is_mentioned_notification,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        data: row.data,
        entity_identifier: row.entity_identifier,
        entity_name: row.entity_name,
        title: row.title,
        message: row.message,
        message_html: row.message_html,
        message_stripped: row.message_stripped,
        sender: row.sender,
        read_at: row.read_at,
        snoozed_till: row.snoozed_till,
        archived_at: row.archived_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        project: row.project,
        triggered_by: row.triggered_by,
        receiver: row.receiver,
    }
}

/// A database row for `UserNotificationPreference` rendering.
/// `user` is non-null (`CASCADE`); `workspace` / `project` are nullable;
/// the five preference columns default `True` (`models:104-108`).
#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub property_change: bool,
    pub state_change: bool,
    pub comment: bool,
    pub mention: bool,
    pub issue_completed: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
    pub project: Option<&'a str>,
}

/// `UserNotificationPreferenceSerializer.to_representation` output
/// (`serializers/notification.py:25-28`, `fields = "__all__"`), in
/// [`PREFERENCE_ALL_FIELDS`] order: `id`, the concrete columns, then
/// the trailing FKs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreferenceView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub property_change: bool,
    pub state_change: bool,
    pub comment: bool,
    pub mention: bool,
    pub issue_completed: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
    pub project: Option<&'a str>,
}

/// Port of `UserNotificationPreferenceSerializer`
/// (`serializers/notification.py:25-28`). Field-for-field copy.
pub fn preference_to_representation<'a>(row: &'a PreferenceRow<'a>) -> PreferenceView<'a> {
    PreferenceView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        property_change: row.property_change,
        state_change: row.state_change,
        comment: row.comment,
        mention: row.mention,
        issue_completed: row.issue_completed,
        created_by: row.created_by,
        updated_by: row.updated_by,
        user: row.user,
        workspace: row.workspace,
        project: row.project,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// FX-NOTIF-04 golden, embedded from the fixture sub-issue's output.
    const NOTIFICATION_GOLDEN: &str =
        include_str!("../../../../fixtures/app_notifications/serializers/notification.golden.json");

    /// FX-NOTIF-05 golden, embedded from the fixture sub-issue's output.
    const PREFERENCE_GOLDEN: &str =
        include_str!("../../../../fixtures/app_notifications/serializers/preference.golden.json");

    /// Top-level JSON key order of a view's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order, while `Value` objects may re-sort keys.
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

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|key| key.to_string()).collect()
    }

    /// A row matching the FX-NOTIF-04 `output_sample` (plus synthetic audit
    /// datetimes, which the sample omits).
    fn sample_notification_row() -> NotificationRow<'static> {
        NotificationRow {
            id: "99999999-9999-9999-9999-999999999999",
            created_at: Some("2026-09-01T00:00:00Z"),
            updated_at: Some("2026-09-02T00:00:00Z"),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            workspace: "33333333-3333-3333-3333-333333333333",
            project: Some("11111111-1111-1111-1111-111111111111"),
            data: None,
            entity_identifier: Some("3fa85f64-5717-4562-b3f3-6d2b9b4f2c11"),
            entity_name: "issue",
            title: "Shape check",
            message: None,
            message_html: "<p></p>",
            message_stripped: None,
            sender: "issue.created",
            triggered_by: None,
            triggered_by_details: None,
            receiver: "22222222-2222-2222-2222-222222222222",
            read_at: None,
            snoozed_till: None,
            archived_at: None,
            is_inbox_issue: false,
            is_intake_issue: false,
            is_mentioned_notification: false,
        }
    }

    #[test]
    fn notification_keys_match_fx_notif_04() {
        let golden: serde_json::Value =
            serde_json::from_str(NOTIFICATION_GOLDEN).expect("fixture parses");
        let pinned: BTreeSet<&str> = golden["output_keys"]
            .as_array()
            .expect("output_keys is an array")
            .iter()
            .map(|key| key.as_str().expect("key is a string"))
            .collect();
        let wired: BTreeSet<&str> = NOTIFICATION_WIRE_FIELDS.into_iter().collect();
        assert_eq!(wired, pinned);
        let body: BTreeSet<&str> = NOTIFICATION_ALL_FIELDS.into_iter().collect();
        let declared: BTreeSet<&str> = NOTIFICATION_DECLARED_FIELDS.into_iter().collect();
        assert_eq!(wired, body.union(&declared).copied().collect());
        // Live-DRF order (probed `NotificationSerializer().fields` on
        // Django 4.2.30 / DRF 3.15.2): `[pk] + declared + concrete
        // columns + forward relations`. Pinned literally so a wrong
        // order fails even when const and struct agree with each other.
        assert_eq!(
            NOTIFICATION_WIRE_FIELDS,
            [
                "id",
                "triggered_by_details",
                "is_inbox_issue",
                "is_intake_issue",
                "is_mentioned_notification",
                "created_at",
                "updated_at",
                "deleted_at",
                "data",
                "entity_identifier",
                "entity_name",
                "title",
                "message",
                "message_html",
                "message_stripped",
                "sender",
                "read_at",
                "snoozed_till",
                "archived_at",
                "created_by",
                "updated_by",
                "workspace",
                "project",
                "triggered_by",
                "receiver",
            ]
        );
        assert_eq!(
            NOTIFICATION_ALL_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "data",
                "entity_identifier",
                "entity_name",
                "title",
                "message",
                "message_html",
                "message_stripped",
                "sender",
                "read_at",
                "snoozed_till",
                "archived_at",
                "created_by",
                "updated_by",
                "workspace",
                "project",
                "triggered_by",
                "receiver",
            ]
        );
    }

    #[test]
    fn notification_sample_renders_fx_notif_04_values() {
        let golden: serde_json::Value =
            serde_json::from_str(NOTIFICATION_GOLDEN).expect("fixture parses");
        let sample = &golden["output_sample"];
        let row = sample_notification_row();
        let view = notification_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&NOTIFICATION_WIRE_FIELDS)
        );
        let body = serde_json::to_value(&view).expect("view serializes");
        for (key, value) in sample.as_object().expect("sample is an object") {
            assert_eq!(&body[key], value, "golden sample key {key}");
        }
        // SET_NULL rule (models/notification.py:24-29): null FK renders a
        // null object, and the contract test asserts exactly this.
        assert!(body["triggered_by"].is_null());
        assert!(body["triggered_by_details"].is_null());
    }

    #[test]
    fn notification_renders_triggered_by_details() {
        let lite = UserLiteRow {
            id: "22222222-2222-2222-2222-222222222222",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "Ada Lovelace",
        };
        let row = NotificationRow {
            triggered_by: Some("22222222-2222-2222-2222-222222222222"),
            triggered_by_details: Some(lite),
            is_inbox_issue: true,
            is_intake_issue: true,
            is_mentioned_notification: true,
            ..sample_notification_row()
        };
        let body =
            serde_json::to_value(notification_to_representation(&row)).expect("view serializes");
        let details = &body["triggered_by_details"];
        assert_eq!(details["id"], "22222222-2222-2222-2222-222222222222");
        assert_eq!(details["first_name"], "Ada");
        assert_eq!(details["display_name"], "Ada Lovelace");
        assert_eq!(
            details.as_object().expect("details is an object").len(),
            7,
            "UserLite key set (serializers/user.py:141-153)"
        );
        assert_eq!(body["triggered_by"], "22222222-2222-2222-2222-222222222222");
        assert!(body["is_inbox_issue"].as_bool().unwrap());
    }

    #[test]
    fn notification_datetimes_pass_through_byte_exact() {
        // DRF rendering owns to the DB edge; the shape must not touch it:
        // offsets, microseconds and the Z rewrite survive verbatim.
        let row = NotificationRow {
            read_at: Some("2026-01-01T00:00:00.123456Z"),
            snoozed_till: Some("2026-06-01T12:30:00+05:30"),
            ..sample_notification_row()
        };
        let body =
            serde_json::to_value(notification_to_representation(&row)).expect("view serializes");
        assert_eq!(body["read_at"], "2026-01-01T00:00:00.123456Z");
        assert_eq!(body["snoozed_till"], "2026-06-01T12:30:00+05:30");
    }

    #[test]
    fn preference_keys_match_fx_notif_05() {
        let golden: serde_json::Value =
            serde_json::from_str(PREFERENCE_GOLDEN).expect("fixture parses");
        let pinned: BTreeSet<&str> = golden["output_keys"]
            .as_array()
            .expect("output_keys is an array")
            .iter()
            .map(|key| key.as_str().expect("key is a string"))
            .collect();
        let wired: BTreeSet<&str> = PREFERENCE_ALL_FIELDS.into_iter().collect();
        assert_eq!(wired, pinned);
        // Live-DRF order (probed
        // `UserNotificationPreferenceSerializer().fields` on Django
        // 4.2.30 / DRF 3.15.2): `[pk] + concrete columns + forward
        // relations`. Pinned literally so a wrong order fails even when
        // const and struct agree with each other.
        assert_eq!(
            PREFERENCE_ALL_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "property_change",
                "state_change",
                "comment",
                "mention",
                "issue_completed",
                "created_by",
                "updated_by",
                "user",
                "workspace",
                "project",
            ]
        );
    }

    #[test]
    fn preference_sample_renders_fx_notif_05_values() {
        let row = PreferenceRow {
            id: "88888888-8888-8888-8888-888888888888",
            created_at: Some("2026-09-01T00:00:00Z"),
            updated_at: Some("2026-09-02T00:00:00Z"),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            user: "22222222-2222-2222-2222-222222222222",
            workspace: None,
            project: None,
            property_change: true,
            state_change: true,
            comment: true,
            mention: true,
            issue_completed: true,
        };
        let view = preference_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&PREFERENCE_ALL_FIELDS));
        let body = serde_json::to_value(&view).expect("view serializes");
        // FX-NOTIF-05 output_sample excerpt (defaults golden).
        assert_eq!(body["user"], "22222222-2222-2222-2222-222222222222");
        assert!(body["workspace"].is_null());
        for key in [
            "property_change",
            "state_change",
            "comment",
            "mention",
            "issue_completed",
        ] {
            assert_eq!(body[key], true, "default {key}");
        }
        // PATCH round-trip half (contract test_preferences_patch_roundtrip):
        // any subset of booleans flips while the rest hold.
        let patched = PreferenceRow {
            comment: false,
            mention: false,
            ..row
        };
        let patched_body =
            serde_json::to_value(preference_to_representation(&patched)).expect("view serializes");
        assert_eq!(patched_body["mention"], false);
        assert_eq!(patched_body["comment"], false);
        assert_eq!(patched_body["property_change"], true);
    }
}
