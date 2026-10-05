//! api-v1 work-item guards (D-18, stage 5).
//!
//! Ports the `apps/api/pi_dash/api/views/issue.py` guard closure and the
//! per-endpoint permission wiring for the api layer:
//!
//! * [`perms`] — `user_has_issue_permission`, the `X-Pi-Dash-Run-Id`
//!   agent-guard closure (`run_belongs_to`, `resolve_moved_by_run`,
//!   `_active_run_of_caller`, `_refuse_agent_action` /
//!   `_refuse_agent_retick`), the route→gate table, the work-item delete
//!   guard, and the page archive guard (PIDASHCONV-671).
//! * [`handlers_pr_links`] — the four PR/review-link endpoints
//!   (`views/github_pr.py:29-100`, `views/git_code_review.py:24-100`)
//!   plus the `pr_links`/`code_reviews` sqlx stores and the production
//!   GitHub/GitLab transports (PIDASHCONV-680).
//! * [`routes`] — the four `urls/work_item.py:218-236` paths with
//!   owned-method cutover (PIDASHCONV-680; this issue creates it —
//!   sibling handler issues extend it, never fork it).
//!
//! Wiring note: the crate root declares `pub mod v1_work_items;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! D-18 handler issues (PIDASHCONV-673…680) add their own modules here;
//! on rebase keep both sides, never fork this file.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod handlers_pr_links;
pub mod perms;
pub mod routes;

use axum::Router;

use crate::state::AppState;

/// Domain router: PR/review-link routes in [`routes::routes`]
/// (PIDASHCONV-680). Owned D-18 api-v1 routes serve from Rust (cutover
/// granularity); everything else keeps proxying to Django through the
/// edge fallback. Sibling D-18 handler issues (PIDASHCONV-673…679) merge
/// their routers here; merges keep both sides, never fork this file.
pub fn routes() -> Router<AppState> {
    routes::routes()
}
