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
//! Re-exported through `pidash-sdk::read` (the facade's `read`
//! module carries this root API item for item).

pub mod comment;
pub mod identity;
pub mod issues;
pub mod render;
#[cfg(test)]
pub(crate) mod test_support;
pub mod work_items;
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

/// Render an aware datetime through a zone: ISO-8601 with `+00:00`
/// rewritten to `Z` (the `pidash-api` serializer kernel, moved with the
/// comment render; shared by the read renderers).
pub(crate) fn render_datetime_in<TzFrom: chrono::TimeZone, TzTo: chrono::TimeZone>(
    dt: &chrono::DateTime<TzFrom>,
    tz: &TzTo,
) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    let local = dt.with_timezone(tz);
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &local.format("%Y-%m-%dT%H:%M:%S").to_string(),
        local.timestamp_subsec_nanos(),
        &suffix,
    )
}

pub(crate) fn render_utc_parts(base: &str, nanos: u32, suffix: &str) -> String {
    // DRF `DateTimeField` renders Python datetimes, which carry only
    // microseconds: no fraction when micros == 0, exactly 6 digits
    // otherwise. Truncate sub-microsecond nanos (e.g. `Utc::now()`
    // stamps) so HTTP output never carries 9 digits (PIDASHCONV-867).
    let micros = nanos / 1000;
    if micros == 0 {
        format!("{base}{suffix}")
    } else {
        format!("{base}.{:06}{suffix}", micros)
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
pub use work_items::{
    decode_comment, decode_issue, expand_estimate_point, expand_parent, expand_project,
    expand_state, expand_user, expand_workspace, fetch_assignee_ids, fetch_assignee_rows,
    fetch_blocker_rows, fetch_comment_direct, fetch_expanded_labels, fetch_grouped_rows,
    fetch_issue_for_write, fetch_label_ids, fetch_project_for_write, render_decoded,
    render_write_response, requested_names, row_datetime, row_datetime_opt, row_string,
    row_string_opt, row_uuid, row_uuid_opt, BlockerRows, DecodedComment, DecodedIssue,
    OwnedAssigneeRow, OwnedRelationItem, RenderRequest, WriteProject, ISSUE_EXTRA_COLS,
    ISSUE_SELECT_COLS,
};
pub use workspace::{workspace_of, workspace_row, workspace_slug};

#[cfg(test)]
mod surface {
    // Survey names that were already public before OVL-05: each `use`
    // fails to compile if its home ever goes private again.
    use crate::app_issues::issue_move::MoveStore;
    use crate::app_issues::pr_links::PrLinksStore;
    use crate::assistant::seams::{agent_model_profile_for_user, has_usable_llm_config};
    use crate::integrations::adapters_github::GithubClient;

    #[test]
    fn already_public_names_stay_public() {
        // Generic bounds over the homes resolve (the survey methods
        // live on these traits); the bounds are never instantiated.
        fn _needs_pr_links<T: PrLinksStore>() {}
        fn _needs_move<T: MoveStore>() {}
        fn _needs_github<T: GithubClient>() {}
        // The assistant seams are callable pure decisions.
        assert!(has_usable_llm_config(true));
        assert!(!has_usable_llm_config(false));
        let _ = agent_model_profile_for_user(true);
    }
}
