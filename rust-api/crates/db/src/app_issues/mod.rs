//! Issue app domain surface (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/` for the db layer, bottom-up:
//!
//! * [`models_core`] — `Issue`, `IssueAssignee`, `IssueLabel`, `Label`,
//!   `ProjectUserProperty` (PIDASHCONV-644, creates this module).
//!   Reads, serializers, guards, tasks and handlers belong to the sibling
//!   D-26 issues; the domain gate is PIDASHCONV-658.
//!
//! Sibling models issues (PIDASHCONV-645/646/647) add their own
//! `models_*.rs` files here and re-export their row structs below next
//! to [`models_core`]'s.

pub mod models_core;

pub use models_core::{
    issue::Issue, issue_assignee::IssueAssignee, issue_label::IssueLabel, label::Label,
    project_user_property::ProjectUserProperty, OnDelete,
};
