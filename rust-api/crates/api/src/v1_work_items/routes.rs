//! D-18 work-item route registration: action routes (handlers F,
//! PIDASHCONV-678), link + comment routes (handlers B, PIDASHCONV-674),
//! PR / code-review link routes (handlers H, PIDASHCONV-680), plus
//! activity + attachment routes (handlers C, PIDASHCONV-675).
//!
//! Ports `apps/api/pi_dash/api/urls/work_item.py:133-151` (the four action
//! paths), `urls/work_item.py:60-77,154-171` (the eight link/comment paths:
//! four `work-items/` routes plus their deprecated `issues/` twins, which
//! share the view classes and therefore the handlers),
//! `urls/work_item.py:218-236` (the four PR/review-link paths) and
//! `urls/work_item.py:79-98,173-192` (the eight activity/attachment paths
//! plus their deprecated twins) onto the
//! merged D-18 foundation. Cutover granularity is the route + method (the
//! pilot `owned()` pattern): the owned methods serve from Rust, every other
//! method on these paths proxies to Django so its 405-after-auth and
//! metadata responses are preserved byte for byte.
//!
//! Sibling D-18 handler issues register their own routes here; on rebase
//! keep both sides, never fork this file.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::Router;

use crate::state::AppState;

use super::handlers_actions::{owned_action, post_retick, post_run_ai, post_wait, post_yield};
use super::handlers_activity::{
    delete_attachment, get_activity_detail, get_activity_list, get_attachment, get_attachment_list,
    owned_activity, owned_attachment_detail, owned_attachment_list, patch_attachment,
    post_attachment,
};

use super::handlers_pr_links::{
    pr_create, pr_destroy, pr_list, review_create, review_destroy, review_list,
};

use super::handlers_social::{
    delete_comment, delete_link, get_comment_detail, get_comment_list, get_link_detail,
    get_link_list, owned_comment_detail, owned_comment_list, owned_link_detail, owned_link_list,
    patch_comment, patch_link, post_comment, post_link,
};

/// Register the four action paths (`urls/work_item.py:133-151`,
/// PIDASHCONV-678), the D-18 link/comment paths
/// (`urls/work_item.py:60-77,154-171`, PIDASHCONV-674) and the
/// PR/review-link paths (`urls/work_item.py:218-236`, PIDASHCONV-680), and
/// the activity/attachment paths (`urls/work_item.py:79-98,173-192`,
/// PIDASHCONV-675).
///
/// Sibling `v1_work_items` paths have no Rust route yet and keep proxying
/// to Django through the fallback; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/",
            super::handlers_pr_links::owned_list(
                axum::routing::get(pr_list).post(pr_create),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/{pk}/",
            super::handlers_pr_links::owned_detail(axum::routing::delete(pr_destroy)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/",
            super::handlers_pr_links::owned_list(
                axum::routing::get(review_list).post(review_create),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/{pk}/",
            super::handlers_pr_links::owned_detail(axum::routing::delete(review_destroy)),
        )
        // `work_item.py:60-63` — link list (get + post).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/",
            owned_link_list(axum::routing::get(get_link_list).post(post_link)),
        )
        // `work_item.py:154-157` — deprecated twin.
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/",
            owned_link_list(axum::routing::get(get_link_list).post(post_link)),
        )
        // `work_item.py:64-67` — link detail (get + patch + delete).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/",
            owned_link_detail(
                axum::routing::get(get_link_detail)
                    .patch(patch_link)
                    .delete(delete_link),
            ),
        )
        // `work_item.py:158-162` — deprecated twin.
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/",
            owned_link_detail(
                axum::routing::get(get_link_detail)
                    .patch(patch_link)
                    .delete(delete_link),
            ),
        )
        // `work_item.py:68-72` — comment list (get + post).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/",
            owned_comment_list(axum::routing::get(get_comment_list).post(post_comment)),
        )
        // `work_item.py:163-167` — deprecated twin.
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/",
            owned_comment_list(axum::routing::get(get_comment_list).post(post_comment)),
        )
        // `work_item.py:73-77` — comment detail (get + patch + delete).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/",
            owned_comment_detail(
                axum::routing::get(get_comment_detail)
                    .patch(patch_comment)
                    .delete(delete_comment),
            ),
        )
        // `work_item.py:168-172` — deprecated twin.
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
            owned_comment_detail(
                axum::routing::get(get_comment_detail)
                    .patch(patch_comment)
                    .delete(delete_comment),
            ),
        )
        // `work_item.py:133-136` — re-tick (post).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/",
            owned_action(axum::routing::post(post_retick)),
        )
        // `work_item.py:137-140` — wait (post).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/",
            owned_action(axum::routing::post(post_wait)),
        )
        // `work_item.py:141-144` — run-ai (post).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/",
            owned_action(axum::routing::post(post_run_ai)),
        )
        // `work_item.py:145-151` — agent-run yield (post).
        .route(
            "/api/v1/workspaces/{slug}/agent-runs/{run_id}/yield/",
            owned_action(axum::routing::post(post_yield)),
        )
        // Activities (IssueActivityListAPIEndpoint `views/issue.py:2124-2175`).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/",
            owned_activity(axum::routing::get(get_activity_list)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/",
            owned_activity(axum::routing::get(get_activity_list)),
        )
        // Activity detail (IssueActivityDetailAPIEndpoint `views/issue.py:2176-2234`).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/",
            owned_activity(axum::routing::get(get_activity_detail)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/",
            owned_activity(axum::routing::get(get_activity_detail)),
        )
        // Attachment list + upload (IssueAttachmentListCreateAPIEndpoint
        // `views/issue.py:2235-2449`).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/",
            owned_attachment_list(
                axum::routing::get(get_attachment_list).post(post_attachment),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/",
            owned_attachment_list(
                axum::routing::get(get_attachment_list).post(post_attachment),
            ),
        )
        // Attachment detail (IssueAttachmentDetailAPIEndpoint
        // `views/issue.py:2450-2653`).
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/",
            owned_attachment_detail(
                axum::routing::get(get_attachment)
                    .patch(patch_attachment)
                    .delete(delete_attachment),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
            owned_attachment_detail(
                axum::routing::get(get_attachment)
                    .patch(patch_attachment)
                    .delete(delete_attachment),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        // Port 1 is never bound, so proxied (unowned) methods fail closed
        // with 502 instead of reaching a dev server.
        crate::routes::with_routes(
            AppState::with_edge(
                "0.1.0",
                crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
            ),
            routes(),
        )
    }

    async fn status(method: &str, uri: &str) -> StatusCode {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("request");
        app().oneshot(request).await.expect("serve").status()
    }

    /// Owned methods serve from Rust: anonymous callers 401 at the auth
    /// layer (never a proxy 502).
    #[tokio::test]
    async fn owned_methods_answer_401_anonymous() {
        let issue = "11111111-1111-1111-1111-111111111111";
        let pk = "22222222-2222-2222-2222-222222222222";
        let pid = "11111111-1111-1111-1111-111111111111";
        let iid = "22222222-2222-2222-2222-222222222222";
        let rid = "33333333-3333-3333-3333-333333333333";
        for (method, uri) in [
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/p1/work-items/{issue}/github/pull-requests/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/p1/work-items/{issue}/github/pull-requests/"),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/acme/projects/p1/work-items/{issue}/github/pull-requests/{pk}/"
                ),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/p1/work-items/{issue}/code-reviews/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/p1/work-items/{issue}/code-reviews/"),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/acme/projects/p1/work-items/{issue}/code-reviews/{pk}/"
                ),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/re-tick/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/wait/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/run-ai/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/agent-runs/{rid}/yield/"),
            ),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
        // PIDASHCONV-675: all eight activity/attachment paths.
        let pid = "44444444-4444-4444-4444-444444444444";
        let iid = "55555555-5555-5555-5555-555555555555";
        let aid = "66666666-6666-6666-6666-666666666666";
        for (method, uri) in [
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/activities/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/activities/"),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/activities/{aid}/"
                ),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/activities/{aid}/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/issue-attachments/"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/issue-attachments/"),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/{aid}/"
                ),
            ),
            (
                "PATCH",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/{aid}/"
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/{aid}/"
                ),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/issue-attachments/{aid}/"
                ),
            ),
            (
                "PATCH",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/issue-attachments/{aid}/"
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/acme/projects/{pid}/issues/{iid}/issue-attachments/{aid}/"
                ),
            ),
        ] {
            assert_eq!(
                status(method, &uri).await,
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }

    /// Unowned methods proxy to Django (502 fail-closed with the test
    /// edge), preserving its 405-after-auth and metadata responses.
    #[tokio::test]
    async fn unowned_methods_proxy() {
        let issue = "11111111-1111-1111-1111-111111111111";
        let pk = "22222222-2222-2222-2222-222222222222";
        let pid = "11111111-1111-1111-1111-111111111111";
        let iid = "22222222-2222-2222-2222-222222222222";
        assert_eq!(
            status(
                "PUT",
                &format!(
                    "/api/v1/workspaces/acme/projects/p1/work-items/{issue}/github/pull-requests/"
                )
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "GET",
                &format!(
                    "/api/v1/workspaces/acme/projects/p1/work-items/{issue}/github/pull-requests/{pk}/"
                )
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "PATCH",
                &format!("/api/v1/workspaces/acme/projects/p1/work-items/{issue}/code-reviews/")
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "POST",
                &format!(
                    "/api/v1/workspaces/acme/projects/p1/work-items/{issue}/code-reviews/{pk}/"
                )
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "GET",
                &format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/re-tick/"),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "PUT",
                &format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/wait/"),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        // PIDASHCONV-675: unowned verbs on the activity/attachment paths.
        let pid = "44444444-4444-4444-4444-444444444444";
        let iid = "55555555-5555-5555-5555-555555555555";
        let aid = "66666666-6666-6666-6666-666666666666";
        assert_eq!(
            status(
                "POST",
                &format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/activities/"),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "PUT",
                &format!("/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/"),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "PUT",
                &format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/{aid}/"
                ),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
    }

    /// Non-UUID path segments proxy to Django (its `<uuid:>` converter
    /// would not match), before auth runs.
    #[tokio::test]
    async fn non_uuid_segments_proxy() {
        let pid = "11111111-1111-1111-1111-111111111111";
        assert_eq!(
            status(
                "POST",
                &format!("/api/v1/workspaces/acme/projects/{pid}/work-items/not-a-uuid/re-tick/"),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "POST",
                "/api/v1/workspaces/acme/agent-runs/not-a-uuid/yield/",
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        // PIDASHCONV-675: activity/attachment paths.
        let pid = "44444444-4444-4444-4444-444444444444";
        let iid = "55555555-5555-5555-5555-555555555555";
        assert_eq!(
            status(
                "GET",
                &format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/not-a-uuid/activities/"
                ),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "GET",
                &format!(
                    "/api/v1/workspaces/acme/projects/{pid}/work-items/{iid}/attachments/not-a-uuid/"
                ),
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
    }
}
