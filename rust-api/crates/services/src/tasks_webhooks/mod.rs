//! D-08 logging decode + event role preprocessing (services layer).
//!
//! Port of the pure helpers in `apps/api/pi_dash/bgtasks/logger_task.py`
//! (`:38-:60`) and `apps/api/pi_dash/bgtasks/event_tracking_task.py`
//! (`:43-:59`). The DB-backed halves live in `pidash-jobs`
//! (`tasks_webhooks::sinks`).

pub mod log_decode;

pub use log_decode::{
    is_role_event, preprocess_data_properties, resolve_role, safe_decode_body, Role,
    USER_INVITED_TO_WORKSPACE, WORKSPACE_DELETED,
};
