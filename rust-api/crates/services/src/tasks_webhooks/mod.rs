//! Webhook + activity + logging background tasks (D-08, services layer).
//!
//! Pure pipelines over injected snapshots: no database, no network, no SMTP.
//! The Celery entry points live in `pidash-jobs` (`tasks_webhooks/`); each
//! module here owns the decision logic the Python `bgtasks/` module owns.
//!
//! * [`activity_issue`] — entity builders (issue, comment, cycle, module)
//!   from `bgtasks/issue_activities_task.py` (`:557-:927`) (PIDASHCONV-197).
//! * [`activity_misc`] — remaining builders (link, attachment, reactions,
//!   vote, relation, draft, intake) from
//!   `bgtasks/issue_activities_task.py` (`:928-:1501`) (PIDASHCONV-198).
//! * [`activity_tracks`] — field trackers from
//!   `bgtasks/issue_activities_task.py` (`:41-:556`) (PIDASHCONV-196).
//! * [`link_crawl`] — `bgtasks/work_item_link_task.py` (PIDASHCONV-200).
//! * [`log_decode`] — pure helpers from `bgtasks/logger_task.py` (`:38-:60`)
//!   and `bgtasks/event_tracking_task.py` (`:43-:59`) (PIDASHCONV-202).
//! * [`page_extract`] — `COMPONENT_MAP` / `extract_all_components` /
//!   `get_entity_details` from `bgtasks/page_transaction_task.py:21-82`
//!   (PIDASHCONV-201). The task entry point itself lives in `pidash-jobs`
//!   (`tasks_webhooks::visit_page`).
//! * [`webhook_data`] — `SERIALIZER_MAPPER` / `MODEL_MAPPER`,
//!   `get_issue_prefetches` and `get_model_data` from
//!   `bgtasks/webhook_task.py` (PIDASHCONV-194).

pub mod activity_issue;
pub mod activity_misc;
pub mod activity_tracks;
pub mod link_crawl;
pub mod log_decode;
pub mod page_extract;
pub mod webhook_data;

pub use log_decode::{
    is_role_event, preprocess_data_properties, resolve_role, safe_decode_body, Role,
    USER_INVITED_TO_WORKSPACE, WORKSPACE_DELETED,
};
