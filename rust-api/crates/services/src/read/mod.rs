//! Public read/render services (OVL-05, PIDASHCONV-1543).
//!
//! The overlay re-implemented these reads because they lived as private
//! helpers in `pidash-api` handlers. Each function here is a verbatim
//! move of the OSS handler helper: same SQL, same row shapes, same
//! miss semantics. OSS handlers call these and map [`Error`] onto
//! their own denial type, so response bytes are unchanged.
//!
//! Every fallible query names its `site`: the log tag the handler's
//! `db_error` logged before the move. Handlers keep logging through
//! their own `db_error`, so operator-visible logs are unchanged too.
//!
//! Re-exported through `pidash-sdk::read` once OVL-02
//! (PIDASHCONV-1540) lands the facade crate.

pub mod comment;
pub mod identity;
pub mod issues;
pub mod render;
pub mod workspace;

use std::fmt;

/// A database/driver failure at a named query site.
///
/// [`Display`](fmt::Display) renders only the driver error, exactly
/// what the handler logged before the move; the [`site`](DbError::site)
/// travels alongside so the handler logs the same tag.
#[derive(Debug)]
pub struct DbError {
    site: &'static str,
    source: sqlx::Error,
}

impl DbError {
    /// Attach the query-site tag to a driver failure.
    pub fn new(site: &'static str, source: sqlx::Error) -> Self {
        Self { site, source }
    }

    /// The log tag the owning handler passes to its `db_error`.
    pub fn site(&self) -> &'static str {
        self.site
    }

    /// The underlying driver error, for the handler's `db_error`.
    pub fn into_source(self) -> sqlx::Error {
        self.source
    }

    /// Split into the site tag and the driver error, for one-line
    /// `db_error` mappings.
    pub fn split(self) -> (&'static str, sqlx::Error) {
        (self.site, self.source)
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Failure modes of the public read surface.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A database/driver failure (the handler's 500 branch).
    #[error(transparent)]
    Db(#[from] DbError),
    /// The row is missing (the handler's 404 branch, whatever its body).
    #[error("object not found")]
    NotFound,
    /// A 500-class row failure: undecodable shape, unparseable id, or a
    /// row that must exist but does not (the tag names the site).
    #[error("read failed: {0}")]
    Failed(&'static str),
}

pub use comment::{
    comment_is_synced, render_actor_detail, render_comment, render_comment_reaction_row,
    ActorDetailView, CommentReactionView, CommentRow, CommentView, IssueFlatView, ProjectLiteView,
    WorkspaceLiteView,
};
pub use identity::{fetch_asset, fetch_profile, fetch_user, AssetRow, ProfileRow, UserShape};
pub use issues::{comment_url, fetch_blocker_summary, resolve_workpad_state, workpad_stripped};
pub use render::{render_state, state_row_from, StateRow, StateRowTuple};
pub use workspace::{workspace_of, workspace_row, workspace_slug};
