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

/// `PIDASH_GITHUB_API_BASE` override for the two job GitHub transports
/// (PIDASHCONV-1472): contract tests point a redirect stub here. `None`
/// when unset or empty, which is the production state — Django has no
/// equivalent knob (its contract leg reaches the stub through an HTTPS
/// MITM proxy instead), so behavior with the variable unset is exactly
/// `GithubClient(token=…)` against `https://api.github.com`.
pub(crate) fn github_api_base_override() -> Option<String> {
    std::env::var("PIDASH_GITHUB_API_BASE")
        .ok()
        .map(|value| value.trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
}
