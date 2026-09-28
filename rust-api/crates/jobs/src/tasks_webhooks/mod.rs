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

pub mod link_crawl;
pub mod sinks;

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
