//! Orchestration engine jobs (D-12, stage 5).
//!
//! The jobs-side half of the orchestration engine:
//!
//! * [`creation_jobs`] — the [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//!   / [`FinalizeAgentRunSeam`][pidash_services::orchestration::creation::FinalizeAgentRunSeam]
//!   implementation over live SQL (`LiveCreationStore`), plus the
//!   transaction drivers that commit and drain post-commit dispatch.
//!   The three `cloud_agent.creation` calls delegate to the merged
//!   [`crate::dispatch`] functions; nothing here calls back into
//!   services-side logic beyond the seam traits (no new crate edges).
//!
//! Fixture ids replayed by the live suites: FX-ORCH-06
//! (`rust-api/fixtures/orchestration/fx06_creation/`) alongside
//! [`creation_jobs`], FX-ORCH-08
//! (`rust-api/fixtures/orchestration/fx08_dispatch/`) alongside
//! [`fire_tick_seam`].
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod creation_jobs;
pub mod fire_tick_seam;

pub use creation_jobs::{
    complete_project_move_handoff, create_and_dispatch_run, create_continuation_run,
    create_project_move_handoff_run, CreationJobsError, CreationOutcome, LiveCreationStore,
    StoreDeps,
};
pub use fire_tick_seam::{
    bounce_issue_no_eligible_runner, dispatch_continuation_run, dispatch_run_ai_run,
    dispatch_run_ai_run_with_reason, maybe_apply_deferred_pause, preflight_eligibility_or_bounce,
    re_tick_ticker, run_ai_for_human, shim_is_ticking_state, shim_tick_interval_seconds,
    wait_ticker, DispatchJobsError, DispatchOutcome, FireTickShim, StateView,
};
