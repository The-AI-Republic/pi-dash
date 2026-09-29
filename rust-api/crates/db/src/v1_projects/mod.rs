#![forbid(unsafe_code)]

//! api-v1 projects/members/states/estimates domain surface (D-19, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/{project,state,estimate}.py`,
//! `WorkspaceMemberInvite` (`db/models/workspace.py:234-258`), and the
//! project-relevant columns of `UserFavorite` (`db/models/favorite.py:14-63`)
//! for the db layer.
//!
//! * [`models`] — column lists, row structs, enums, defaults, constraints,
//!   manager scopes, save-rule helpers, and the project-identifier routing
//!   rule (PIDASHCONV-352).
//! * [`queries_stateest`] — state / estimate queryset reads (S1/S2/E1/E2/E3,
//!   PIDASHCONV-366, fixture FX-Q-STATEEST).
//!
//! Wiring note: the crate root declares `pub mod v1_projects;` (seam added
//! by PIDASHCONV-352); every file under this module is new. Sibling issues
//! add their own layers under sibling modules (e.g. PIDASHCONV-364
//! `queries_projmem`); on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod models;
pub mod queries_stateest;
