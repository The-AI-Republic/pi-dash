//! D-08 logging + event-tracking sinks (jobs layer).
//!
//! Port of the task entry points of
//! `apps/api/pi_dash/bgtasks/logger_task.py:22-100` and
//! `apps/api/pi_dash/bgtasks/event_tracking_task.py:24-81`:
//! Celery task names, `(args, kwargs)` extraction, the
//! mongo-else-postgres routing, the PostHog `/batch/` payload, and the
//! [`Registry`][crate::worker::Registry] wiring.

pub mod sinks;

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
