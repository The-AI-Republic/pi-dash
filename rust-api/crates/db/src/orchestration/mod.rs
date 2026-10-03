#![forbid(unsafe_code)]

//! Orchestration L2 models: run lookups + ingest + workpad + querysets (D-12, stage 5).
//!
//! Ports the model layer of the orchestration engine:
//!
//! * [`runs`] — `orchestration/service.py:84-107` (`_active_run_for`,
//!   `_latest_prior_run`) and `orchestration/done_signal.py:141-181`
//!   (`ingest_into_run`, over the L1 parser).
//! * [`workpad`] — `orchestration/workpad.py` whole (workpad
//!   accessors, agent system user).
//! * [`querysets`] — `core/querysets.py` whole plus the role-fact SQL
//!   of `core/permissions.py` (decisions stay in the F-06 kernel).
//!
//! Reads reuse the D-11 [`dispatch`](crate::dispatch) `AgentRun`
//! shape + `AgentRunStatus` and the L1
//! [`pidash_types::orchestration`] parser — never re-ported. All
//! queries are executor-generic (`sqlx::query`, no build-time
//! database); multi-statement flows take `&mut PgConnection`.
//!
//! Fixture id replayed by the unit tests alongside each module:
//! FX-ORCH-02 (`rust-api/fixtures/orchestration/fx02_reads/`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod querysets;
pub mod runs;
pub mod workpad;

pub use querysets::{
    fetch_is_workspace_member, fetch_member_project_issue_ids, fetch_project_has_allowed_role,
    fetch_project_is_member, fetch_user_issue_ids, fetch_workspace_admin_by_slug,
    fetch_workspace_role, fetch_workspace_role_by_slug, user_issues_sql, ROLE_ADMIN, ROLE_GUEST,
    ROLE_MEMBER,
};
pub use runs::{
    active_run_for, ingest_into_run, is_terminal_status, latest_prior_run, plan_ingest,
};
pub use workpad::{
    fetch_workpad, get_agent_system_user, set_workpad, AgentSystemUser, AgentUserCollisionError,
    GetAgentUserError,
};
