//! api-v1 projects/members/states/estimates permission guards (D-19, stage 5).
//!
//! Ports `apps/api/pi_dash/api/views/{project,member,invite,user,state,
//! estimate}.py` permission wiring for the api layer:
//!
//! * [`perms`] — the 7 guard classes as a route→gate table plus thin
//!   decision wrappers over the read-only F-06 kernel (PIDASHCONV-367).
//!
//! Wiring note: the crate root declares `pub mod v1_projects;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! issues add their own handler modules here; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod perms;
