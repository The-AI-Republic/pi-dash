//! api-v1 work-item guards (D-18, stage 5).
//!
//! Ports the `apps/api/pi_dash/api/views/issue.py` guard closure and the
//! per-endpoint permission wiring for the api layer:
//!
//! * [`perms`] — `user_has_issue_permission`, the `X-Pi-Dash-Run-Id`
//!   agent-guard closure (`run_belongs_to`, `resolve_moved_by_run`,
//!   `_active_run_of_caller`, `_refuse_agent_action` /
//!   `_refuse_agent_retick`), the route→gate table, the work-item delete
//!   guard, and the page archive guard (PIDASHCONV-671).
//!
//! Wiring note: the crate root declares `pub mod v1_work_items;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! D-18 handler issues (PIDASHCONV-673…680) add their own modules here;
//! on rebase keep both sides, never fork this file.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod perms;
