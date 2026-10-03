#![forbid(unsafe_code)]

//! App workspace/users/API-tokens domain surface (D-24, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/{workspace,user,api,favorite}.py`
//! (this module) for the services layer, bottom-up:
//!
//! * [`ser_account_token`] — profile + account + API-token + favorite
//!   serializers (PIDASHCONV-604).
//! * [`ser_extras`] — user links, recent visits, home/user preferences,
//!   stickies (PIDASHCONV-602).
//!
//! Wiring note: the crate root declares `pub mod app_workspace;`.
//! Sibling issues add their own siblings to this file (`ser_workspace`
//! PIDASHCONV-600, `ser_invite` PIDASHCONV-601, `ser_extras`
//! PIDASHCONV-602, `ser_user` PIDASHCONV-603); on rebase keep both sides.
//!
//! Fixture input: F-W24-05 (`rust-api/fixtures/app_workspace/`
//! `serializers/account_token_fav.golden.json` + `TRACE.md`) and F-W24-03
//! (`rust-api/fixtures/app_workspace/` `serializers/extras.golden.json` +
//! `TRACE.md`); the goldens are the Done-when oracles for this layer.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

pub mod ser_account_token;
pub mod ser_extras;
