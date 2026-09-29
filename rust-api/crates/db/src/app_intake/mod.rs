//! Intake app domain surface (D-32, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/intake.py` for the db layer:
//!
//! * [`models`] — `SourceType`, `IntakeIssueStatus`, `Intake`,
//!   `IntakeIssue` (PIDASHCONV-284, struct + column/constraint mapping
//!   only). Reads, serializers, guards, tasks and handlers belong to
//!   PIDASHCONV-281/282/314/329/345/360/385/395; the domain gate is
//!   PIDASHCONV-402.

pub mod models;

pub use models::{
    intake::Intake, intake_issue::IntakeIssue, IntakeIssueStatus, OnDelete, SourceType,
};
