//! Git provider integrations (D-05, stage 5).
//!
//! Ports the HTTP-touching adapter closure of
//! `apps/api/pi_dash/integrations/git/`:
//!
//! * [`adapters_github`] — `adapters/github.py:41-257` (`GitHubAdapter`,
//!   key `"github"`).
//! * [`adapters_gitlab`] — `adapters/gitlab.py:37-412` (helpers,
//!   `GitLabClient`, `GitLabAdapter`, key `"gitlab"`).
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/integrations/adapters/github.golden.json`,
//! `rust-api/fixtures/integrations/adapters/gitlab.golden.json`.

pub mod adapters_github;
pub mod adapters_gitlab;

pub use adapters_github::{ClientAuth, GitHubAdapter, GithubClient, GithubError, GITHUB_HOST};
