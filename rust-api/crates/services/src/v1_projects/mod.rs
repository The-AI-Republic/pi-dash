#![forbid(unsafe_code)]

//! api-v1 projects/members/states/estimates domain surface (D-19, stage 5).
//!
//! Ports `apps/api/pi_dash/api/views/{project,member,invite,user,state,
//! estimate}.py` + serializers for the services layer, bottom-up:
//!
//! * [`ser_project`] — project serializers (PIDASHCONV-348).
//! * [`ser_workflow`] — state / estimate serializers (PIDASHCONV-351).
//! * [`ser_collab`] — member / invite / user serializers (PIDASHCONV-350).
//! * [`tasks`] — project task publishers: `model_activity` x2 +
//!   `webhook_activity` x1 as transactional Celery-v2 enqueues
//!   (PIDASHCONV-368).
//!
//! Wiring note: the crate root declares `pub mod v1_projects;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! issues add their own `ser_*` siblings to this `mod.rs`; on rebase keep
//! both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod ser_collab;
pub mod ser_project;
pub mod ser_workflow;
pub mod tasks;
