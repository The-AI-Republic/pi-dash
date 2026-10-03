#![forbid(unsafe_code)]

//! App workspace/users/tokens domain surface (D-24, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/{workspace,user,api,favorite}.py`
//! (this module) for the services layer, bottom-up:
//!
//! * [`ser_workspace`] — workspace + member serializers (PIDASHCONV-600).
//! * [`ser_user`] — user serializers: shapes, name guards, me-settings
//!   (PIDASHCONV-603, F-W24-04).
//! * [`ser_invite`] — invite + join-request + theme/props serializers
//!   (`workspace.py:110-194`, PIDASHCONV-601).
//! * [`ser_account_token`] — profile + account + API-token + favorite
//!   serializers (PIDASHCONV-604).
//! * [`ser_extras`] — user links, recent visits, home/user preferences,
//!   stickies (PIDASHCONV-602).
//!
//! * [`models_prefs`] — per-user prefs/links + home/sidebar prefs +
//!   recent visits models (PIDASHCONV-606).
//! * [`models_user`] — user / profile / account / API-token / favorite
//!   table models (PIDASHCONV-607).
//!
//! * [`queries_membership`] — workspace member / invite / join-request
//!   query builders (PIDASHCONV-609).
//!
//! * [`queries_core`] — workspace core querysets: lists, dashboard
//!   bundle, themes, export-CSV (PIDASHCONV-608).
//!
//! * [`queries_user`] — user / account / profile / API-token query
//!   builders: settings + session reads, email-change + cache,
//!   deactivate bundle, account/profile/token CRUD (PIDASHCONV-612).
//!
//! * [`tasks`] — Celery publishers: `workspace_seed` + `track_event` (6
//!   sites) + `workspace_invitation` + `issue_activity` (3 draft shapes)
//!   + user mail (PIDASHCONV-614, F-W24-14).
//!
//! Wiring note: the crate root declares `pub mod app_workspace;`.
//! Sibling issues add their own siblings to this file (`ser_invite`
//! PIDASHCONV-601, `ser_extras` PIDASHCONV-602, `ser_user`
//! PIDASHCONV-603, `models_workspace` PIDASHCONV-605, `models_prefs`
//! PIDASHCONV-606, `models_user` PIDASHCONV-607, `queries_membership`
//! PIDASHCONV-609, `queries_profile` PIDASHCONV-610, `tasks`
//! PIDASHCONV-614); on rebase keep both sides.
//!
//! Fixture input: F-W24-01 (`rust-api/fixtures/app_workspace/`
//! `serializers/workspace_core.golden.json` + `TRACE.md`), F-W24-02
//! (`rust-api/fixtures/app_workspace/serializers/invites.golden.json` +
//! `TRACE.md`), F-W24-05 (`rust-api/fixtures/app_workspace/
//! `serializers/account_token_fav.golden.json` + `TRACE.md`), F-W24-03
//! (`rust-api/fixtures/app_workspace/` `serializers/extras.golden.json` +
//! `TRACE.md`), F-W24-07 (`models/workspace_prefs.columns.json`, incl. the
//! `UserFavorite` part), F-W24-08 (`models/user_token.columns.json`),
//! F-W24-09 (`queries/core.sql` + `queries/core.rows.json`), and
//! F-W24-10 (`queries/membership.sql` + `.rows.json`), and F-W24-04
//! (`rust-api/fixtures/app_workspace/` `serializers/user.golden.json`),
//! and F-W24-14 (`tasks/enqueue.golden.json`), and F-W24-12, user
//! part (`queries/extras_user.sql` R13-R14 +
//! `queries/extras_user.rows.json` `user_account` / `api_tokens`
//! cases); the goldens are the Done-when oracles for this layer.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

pub mod models_prefs;
pub mod models_user;
pub mod queries_core;
pub mod queries_membership;
pub mod queries_user;
pub mod ser_account_token;
pub mod ser_extras;
pub mod ser_invite;
pub mod ser_user;
pub mod ser_workspace;
pub mod tasks;
