//! Webhook + activity + logging background tasks (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry points of `apps/api/pi_dash/bgtasks/`.
//! Each module owns the Celery wire surface (task names, payload parsing)
//! and registers [`Registry`] handlers that run the pipeline in
//! `pidash-services` (`tasks_webhooks/`). Unregistered names stay
//! Python-owned and forward over AMQP until their port registers here.
//!
//! * [`link_crawl`] — `bgtasks/work_item_link_task.py` (PIDASHCONV-200).
//! * [`sinks`] — `bgtasks/logger_task.py` + `bgtasks/event_tracking_task.py`
//!   (PIDASHCONV-202): Celery task names, `(args, kwargs)` extraction, the
//!   mongo-else-postgres routing, the PostHog `/batch/` payload, and the
//!   [`Registry`][crate::worker::Registry] wiring.
//! * [`visit_page`] — `bgtasks/recent_visited_task.py` +
//!   `bgtasks/page_transaction_task.py` (PIDASHCONV-201): recent-visit +
//!   page-transaction entry points. The pure component extraction lives in
//!   `pidash-services` (`tasks_webhooks::page_extract`).
//! * [`activity_dispatch`] — `bgtasks/issue_activities_task.py`
//!   `issue_activity` dispatcher (PIDASHCONV-198): Celery task name,
//!   `(args, kwargs)` binding, uuid guard, redis origin, issue touch,
//!   27-type mapper, `bulk_create`, notifications enqueue, and the
//!   [`Registry`][crate::worker::Registry] wiring. The pure row builders
//!   live in `pidash-services` (`tasks_webhooks::activity_misc`).

pub mod activity_dispatch;
pub mod link_crawl;
pub mod sinks;
pub mod visit_page;

pub use activity_dispatch::{
    activity_json, bind_issue_activity, build_notifications_job, django_dumps, drf_datetime,
    is_activity_task, is_known_activity_type, is_valid_uuid, issue_flat_json,
    register_activity_task, run_activity, serialize_notification_rows, DispatchError,
    IssueActivityCall, IssueFlatRow, RedisOrigin, StoredActivityRow, ACTIVITY_TYPES,
    FIND_COMMENT_ISSUE_SQL, FIND_COMMENT_REACTION_SQL, FIND_CREATED_ISSUE_SQL, FIND_CYCLE_SQL,
    FIND_ESTIMATE_SQL, FIND_ISSUE_FLAT_SQL, FIND_ISSUE_REACTION_SQL, FIND_ISSUE_REF_SQL,
    FIND_ISSUE_SQL, FIND_LABEL_SQL, FIND_LATEST_ACTIVITY_SQL, FIND_MODULE_SQL, FIND_PARENT_SQL,
    FIND_PROJECT_WORKSPACE_SQL, FIND_STATE_SQL, FIND_USER_SQL, INSERT_SUBSCRIBERS_SQL,
    ISSUE_ACTIVITY_TASK, NOTIFICATIONS_TASK, ORIGIN_TTL_SECS, TOUCH_ACTIVITY_SQL, TOUCH_ISSUES_SQL,
    TOUCH_ISSUE_SQL,
};

pub use link_crawl::{register_link_crawl_handler, TASK_NAMES};
pub use sinks::{
    build_capture_request, capture_url, determine_server_host, insert_api_activity_log, iso_now,
    lookup_workspace_owner, mongo_log_to_document, parse_activity_log_row, parse_process_logs_call,
    parse_track_event_call, post_capture, posthog_configuration_from_env, process_logs_handler,
    python_str, register_sink_tasks, resolve_posthog_config, route_for_logs, track_event_handler,
    ActivityLogRow, CaptureRequest, LogSink, ProcessLogsCall, SinkError, TrackEventCall,
    API_ACTIVITY_LOG_INSERT, MONGO_COLLECTION, NOT_CONFIGURED_WARNING, POSTHOG_API_KEY_VAR,
    POSTHOG_BATCH_PATH, POSTHOG_DEFAULT_HOST, POSTHOG_HOST_VAR, POSTHOG_LIB, POSTHOG_LIB_VERSION,
    POSTHOG_TIMEOUT_SECS, POSTHOG_USER_AGENT, PROCESS_LOGS_TASK, TRACK_EVENT_TASK,
    WORKSPACE_OWNER_SQL,
};
pub use visit_page::{
    is_visit_page_task, page_transaction_message, recent_visited_task_message,
    register_visit_page_handlers, PAGE_TRANSACTION_TASK_NAME, RECENT_VISITED_TASK_NAME,
};
