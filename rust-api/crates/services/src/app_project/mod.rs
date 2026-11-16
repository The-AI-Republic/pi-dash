#![forbid(unsafe_code)]

//! App project/states/estimates domain surface (D-25, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/{project,state,estimate}.py`
//! (this module) for the services layer, bottom-up:
//!
//! * [`ser_member`] — member / invite serializers (PIDASHCONV-564).
//! * [`ser_project`] — `ProjectSerializer` (`:30-117`),
//!   `ProjectLiteSerializer` (`:120-133`), `ProjectListSerializer`
//!   (`:136-168`), `ProjectDetailSerializer` (`:171-190`, unreferenced
//!   except `__init__` — ported as-is) and `DeployBoardSerializer`
//!   (`:259-266`) (PIDASHCONV-563).
//! * [`ser_workflow`] — state / estimate serializers (PIDASHCONV-565).
//! * [`ser_shared`] — shared + nested serializers (PIDASHCONV-566).
//! * [`tasks`] — task call-sites + invite tokens + `handle_exception`
//!   table (PIDASHCONV-570).
//! * [`queries`] — read querysets + identifier routing (PIDASHCONV-568).
//!
//! Wiring note: the crate root declares `pub mod app_project;`. Sibling
//! issues have all landed (`ser_project` PIDASHCONV-563, `ser_member`
//! PIDASHCONV-564); on rebase keep both sides.
//!
//! Fixture input: FX-APROJ-03 (`rust-api/fixtures/app_project/`
//! `FX-APROJ-03.serializers_state_estimate.json` + `TRACE.md`); the golden
//! is the Done-when oracle for this layer. Shared goldens: FX-APROJ-04
//! (`FX-APROJ-04.serializers_shared.json`). Member/invite goldens:
//! FX-APROJ-02 (`FX-APROJ-02.serializers_member_invite.json`). Project
//! goldens: FX-APROJ-01 (`FX-APROJ-01.serializers_project.json`). Queries
//! goldens: FX-APROJ-06 (`FX-APROJ-06.queries.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); Dead Python Code
//! `05399703-0404-49f6-a680-5924d6df7640` (updated_at
//! 2026-09-23T08:30:31.434101Z, no rows for this domain); PIDASHCONV-1
//! rulebook (updated 2026-10-02T22:09:36Z).

pub mod queries;
pub mod ser_member;
pub mod ser_project;
pub mod ser_shared;
pub mod ser_workflow;
pub mod tasks;

pub use tasks::{
    detail_body, estimate_point_dumps, handle_exception, invite_create_failure, invite_token,
    model_activity_on_create, model_activity_on_partial_update, point_destroy_plan,
    IssueActivityEmit, IssuePointRef, ModelActivityEmit, PointDestroyStep, ProjectAddUserEmailEmit,
    RecentVisitedEmit, ViewError, WebhookActivityEmit, INTEGRITY_ERROR_BODY,
    INVITE_JWT_HEADER_JSON, ISSUE_ACTIVITY_KWARG_ORDER, ISSUE_ACTIVITY_TASK,
    ISSUE_ACTIVITY_UPDATED, KEY_ERROR_BODY, MODEL_ACTIVITY_KWARG_ORDER, MODEL_ACTIVITY_MODEL,
    MODEL_ACTIVITY_TASK, OBJECT_NOT_FOUND_BODY, PROJECT_ADD_USER_EMAIL_TASK, RECENT_VISITED_ENTITY,
    RECENT_VISITED_KWARG_ORDER, RECENT_VISITED_TASK, SERVER_ERROR_BODY, VALIDATION_ERROR_BODY,
    WEBHOOK_ACTIVITY_KWARG_ORDER, WEBHOOK_ACTIVITY_TASK, WEBHOOK_EVENT_PROJECT,
    WEBHOOK_VERB_DELETED,
};
