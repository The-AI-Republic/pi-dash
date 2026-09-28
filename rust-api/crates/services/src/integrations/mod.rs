//! Git provider integrations (D-05, stage 5).
//!
//! Ports the HTTP-touching adapter closure of
//! `apps/api/pi_dash/integrations/git/`:
//!
//! * [`adapters_github`] — `adapters/github.py:41-257` (`GitHubAdapter`,
//!   key `"github"`).
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/integrations/adapters/github.golden.json`.

pub mod adapters_github;

pub use adapters_github::{ClientAuth, GitHubAdapter, GithubClient, GithubError, GITHUB_HOST};
