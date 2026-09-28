#![forbid(unsafe_code)]

//! Provider-backed background tasks (D-05, jobs layer).
//!
//! [`git_sync`] ports the provider-neutral poller
//! (`apps/api/pi_dash/bgtasks/git_sync_task.py:30-323`).
//!
//! [`github_sync`] ports the legacy GitHub poller
//! (`apps/api/pi_dash/bgtasks/github_sync_task.py:48-389`), and
//! [`github_signals`] the completion comment-back hook
//! (`apps/api/pi_dash/bgtasks/github_signals.py:31-74`).

pub mod git_sync;
pub mod github_signals;
pub mod github_sync;
