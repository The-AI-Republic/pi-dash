#![forbid(unsafe_code)]

//! Provider-backed background tasks (D-05, jobs layer).
//!
//! [`git_sync`] ports the provider-neutral poller
//! (`apps/api/pi_dash/bgtasks/git_sync_task.py:30-323`).

pub mod git_sync;
