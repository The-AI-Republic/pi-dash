//! PR / code-review link route registration (D-18 handlers H, PIDASHCONV-680).
//!
//! Ports `apps/api/pi_dash/api/urls/work_item.py:218-236` (4 entries) onto
//! the merged D-18 foundation. Cutover granularity is the route + method
//! (the pilot `owned()` pattern): the owned methods serve from Rust, every
//! other method on these paths proxies to Django so its 405-after-auth and
//! metadata responses are preserved byte for byte.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::Router;

use crate::state::AppState;

use super::handlers_pr_links::{
    pr_create, pr_destroy, pr_list, review_create, review_destroy, review_list,
};

/// Register the four PR/review-link paths (`urls/work_item.py:218-236`).
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
    }
}
