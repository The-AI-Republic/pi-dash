//! api-v1 work items (D-18, stage 5).
//!
//! Ports the `apps/api/pi_dash/api/views/issue.py` guard closure, the
//! per-endpoint permission wiring, and the endpoint handlers for the api
//! layer:
//!
//! * [`perms`] — `user_has_issue_permission`, the `X-Pi-Dash-Run-Id`
//!   agent-guard closure (`run_belongs_to`, `resolve_moved_by_run`,
//!   `_active_run_of_caller`, `_refuse_agent_action` /
//!   `_refuse_agent_retick`), the route→gate table, the work-item delete
//!   guard, and the page archive guard (PIDASHCONV-671).
//! * [`handlers_actions`] — the re-tick / wait / run-ai / yield handlers
//!   (PIDASHCONV-678), registered by [`routes`].
//! * [`handlers_activity`] — the activity + attachment endpoints
//!   (`IssueActivityListAPIEndpoint`, `IssueActivityDetailAPIEndpoint`,
//!   `IssueAttachmentListCreateAPIEndpoint`,
//!   `IssueAttachmentDetailAPIEndpoint`, PIDASHCONV-675).
//! * [`handlers_pr_links`] — the four PR/review-link endpoints
//!   (`views/github_pr.py:29-100`, `views/git_code_review.py:24-100`)
//!   plus the `pr_links`/`code_reviews` sqlx stores and the production
//!   GitHub/GitLab transports (PIDASHCONV-680).
//! * [`handlers_social`] — the link + comment endpoints
//!   (`IssueLinkListCreateAPIEndpoint`, `IssueLinkDetailAPIEndpoint`,
//!   `IssueCommentListCreateAPIEndpoint`,
//!   `IssueCommentDetailAPIEndpoint`, PIDASHCONV-674).
//! * [`routes`] — the `urls/work_item.py` route registration with
//!   owned-method cutover (created by PIDASHCONV-680; extended by
//!   PIDASHCONV-674, PIDASHCONV-675 and PIDASHCONV-678; sibling handler
//!   issues extend it,
//!   never fork it).
//!
//! Wiring note: the crate root declares `pub mod v1_work_items;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! D-18 handler issues (PIDASHCONV-673…680) add their own modules here
//! and merge their routers into [`routes()`]; on rebase keep both sides,
//! never fork this file.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod handlers_actions;
pub mod handlers_activity;
pub mod handlers_pr_links;
pub mod handlers_social;
pub mod perms;
pub mod routes;

use axum::Router;

use crate::state::AppState;

/// Domain router: action routes (PIDASHCONV-678), link/comment routes
/// (PIDASHCONV-674), activity/attachment routes (PIDASHCONV-675), and
/// PR/review-link routes (PIDASHCONV-680) in
/// [`routes::routes`]. Owned D-18 api-v1 routes serve from Rust (cutover
/// granularity); everything else keeps proxying to Django through the
/// edge fallback. Sibling D-18 handler issues (PIDASHCONV-673…679) merge
/// their routers here; merges keep both sides, never fork this file.
pub fn routes() -> Router<AppState> {
    routes::routes()
}
