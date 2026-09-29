//! api-v1 projects/members/states/estimates domain surface (D-19, stage 5).
//!
//! Ports `apps/api/pi_dash/api/views/{project,member,invite,user,state,
//! estimate}.py` + serializers for the services layer, bottom-up:
//!
//! * [`ser_workflow`] — state / estimate serializers (PIDASHCONV-351).
//! * `ser_project` — project serializers (PIDASHCONV-348, sibling).
//! * [`ser_collab`] — member / invite / user serializers (PIDASHCONV-350,
//!   sibling).
//!
//! Wiring note: the crate root declares `pub mod v1_projects;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! issues add their own `ser_*` siblings to this `mod.rs`; on rebase keep
//! both sides.

pub mod ser_collab;
pub mod ser_workflow;
