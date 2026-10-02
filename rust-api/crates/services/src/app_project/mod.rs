#![forbid(unsafe_code)]

//! App project/states/estimates domain surface (D-25, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/{state,estimate}.py` (this
//! module) for the services layer, bottom-up:
//!
//! * [`ser_workflow`] — state / estimate serializers (PIDASHCONV-565).
//! * [`ser_shared`] — shared + nested serializers (PIDASHCONV-566).
//!
//! Wiring note: the crate root declares `pub mod app_project;`. Sibling
//! issues add their own `ser_*` siblings to this file (`ser_project`
//! PIDASHCONV-563, `ser_member` PIDASHCONV-564); on rebase keep both sides.
//!
//! Fixture input: FX-APROJ-03 (`rust-api/fixtures/app_project/`
//! `FX-APROJ-03.serializers_state_estimate.json` + `TRACE.md`); the golden
//! is the Done-when oracle for this layer. Shared goldens: FX-APROJ-04
//! (`FX-APROJ-04.serializers_shared.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); Dead Python Code
//! `05399703-0404-49f6-a680-5924d6df7640` (updated_at
//! 2026-09-23T08:30:31.434101Z, no rows for this domain); PIDASHCONV-1
//! rulebook (updated 2026-10-02T22:09:36Z).

pub mod ser_shared;
pub mod ser_workflow;
