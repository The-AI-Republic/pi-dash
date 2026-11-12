#![forbid(unsafe_code)]

//! App notification serializers (D-34, stage 5).
//!
//! Ports the read shapes of `apps/api/pi_dash/app/serializers/notification.py`
//! (all 28 lines); see [`shape`] for the two serializers and [`queries`]
//! for the four query units (`base.py:36-45,:57-137,:202-221,:239-287`).
//! The handlers (PIDASHCONV-301…303) own routing and writes. The shape
//! layer owns only the wire shapes those layers render through.

pub mod queries;
pub mod shape;

pub use queries::{
    archived_clause, base_where, batch_ids, created_member_guard_sql, intake_exists_sql,
    list_assigned_issue_ids_sql, list_created_issue_ids_sql, list_select_sql,
    list_subscribed_issue_ids_sql, list_type_filter, mark_all_read_body, mark_all_read_list_sql,
    mark_all_read_update_sql, mark_archived_clause, mark_assigned_issue_ids_sql,
    mark_snoozed_clause, mark_type_filter, mark_watching_issue_ids_sql, mentioned_annotation_sql,
    mentioned_clause, read_clause, read_stamps, select_list, snoozed_clause,
    unread_mention_count_sql, unread_response_body, unread_watching_count_sql, NotificationScope,
    ParamError, TypeFilter, BULK_UPDATE_BATCH_SIZE, ISSUE_ASSIGNEE_TABLE, ISSUE_INTAKE_TABLE,
    ISSUE_SUBSCRIBER_TABLE, ISSUE_TABLE, NOTIFICATION_SELECT_COLUMNS, NOTIFICATION_TABLE,
    SELECT_RELATED_TABLES, WORKSPACE_MEMBER_TABLE, WORKSPACE_TABLE,
};
pub use shape::{
    notification_to_representation, preference_to_representation, NotificationRow,
    NotificationView, PreferenceRow, PreferenceView, NOTIFICATION_ALL_FIELDS,
    NOTIFICATION_DECLARED_FIELDS, NOTIFICATION_WIRE_FIELDS, PREFERENCE_ALL_FIELDS,
};
