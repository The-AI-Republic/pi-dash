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
//! Fixture id replayed by the live suite: FX-ORCH-06
//! (`rust-api/fixtures/orchestration/fx06_creation/`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod creation_jobs;

pub use creation_jobs::{
    complete_project_move_handoff, create_and_dispatch_run, create_continuation_run,
    create_project_move_handoff_run, CreationJobsError, CreationOutcome, LiveCreationStore,
    StoreDeps,
};
