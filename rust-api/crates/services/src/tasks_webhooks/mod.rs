//! Webhook + activity + logging background tasks (D-08, services layer).
//!
//! Pure pipelines over injected snapshots: no database, no network, no SMTP.
//! The Celery entry points live in `pidash-jobs` (`tasks_webhooks/`); each
//! module here owns the decision logic the Python `bgtasks/` module owns.
//!
//! * [`activity_tracks`] — field trackers from
//!   `bgtasks/issue_activities_task.py` (`:41-:556`) (PIDASHCONV-196).
//! * [`link_crawl`] — `bgtasks/work_item_link_task.py` (PIDASHCONV-200).
//! * [`log_decode`] — pure helpers from `bgtasks/logger_task.py` (`:38-:60`)
//!   and `bgtasks/event_tracking_task.py` (`:43-:59`) (PIDASHCONV-202).

pub mod activity_tracks;
pub mod link_crawl;
pub mod log_decode;

pub use log_decode::{
    is_role_event, preprocess_data_properties, resolve_role, safe_decode_body, Role,
    USER_INVITED_TO_WORKSPACE, WORKSPACE_DELETED,
};
