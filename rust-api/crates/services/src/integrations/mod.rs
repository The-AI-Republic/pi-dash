//! Git provider integrations (D-05, stage 5).
//!
//! Ports the HTTP-touching adapter closure of
//! `apps/api/pi_dash/integrations/git/`:
//!
//! * [`adapters_github`] — `adapters/github.py:41-257` (`GitHubAdapter`,
//!   key `"github"`).
//! * [`adapters_gitlab`] — `adapters/gitlab.py:37-412` (helpers,
//!   `GitLabClient`, `GitLabAdapter`, key `"gitlab"`).
//! * [`accounts`] — `services.py:29-202` (errors, host normalization,
//!   credential merge, account queryset/select/create/resolve;
//!   PIDASHCONV-145).
//! * [`repositories`] — `services.py:203-231,297-388` (repo upsert,
//!   clone URL, bind/get/set-sync/unbind/list; PIDASHCONV-145).
//! * [`serializers`] — `services.py:232-296` (the four serializers;
//!   PIDASHCONV-145).
//! * [`code_reviews`] — `code_reviews.py:1-262` (link-lifecycle closure:
//!   errors, legacy/path lookups, legacy sync, detach, account fallback,
//!   attach; PIDASHCONV-146).
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/integrations/adapters/github.golden.json`,
//! `rust-api/fixtures/integrations/adapters/gitlab.golden.json`,
//! `rust-api/fixtures/integrations/services/*.golden.json`,
//! `rust-api/fixtures/integrations/code_reviews.golden.json`.

pub mod accounts;
pub mod adapters_github;
pub mod adapters_gitlab;
pub mod code_reviews;
pub mod repositories;
pub mod serializers;

pub use adapters_github::{ClientAuth, GitHubAdapter, GithubClient, GithubError, GITHUB_HOST};
