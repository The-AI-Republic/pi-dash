//! Project route registration (D-19 handlers A, PIDASHCONV-369).
//!
//! Ports `apps/api/pi_dash/api/urls/project.py:15-33` (4 entries) onto the
//! merged D-19 foundation. Cutover granularity is the route + method (the
//! pilot `owned()` pattern): the owned methods serve from Rust, every other
//! method on these paths proxies to Django so its 405-after-auth and
//! metadata responses are preserved byte for byte.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::Router;

use crate::state::AppState;

use super::handlers_project::{
    archive_project, create_project, delete_project, list_projects, patch_project,
    retrieve_project, summary, unarchive_project,
};

/// Register the four project paths (`urls/project.py:15-33`).
///
/// Sibling `v1_projects` paths (members, states, estimates) have no Rust
/// route yet and keep proxying to Django through the fallback; on rebase
/// keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{slug}/projects/",
            super::handlers_project::owned_list(
                axum::routing::get(list_projects).post(create_project),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{pk}/",
            super::handlers_project::owned_detail(
                axum::routing::get(retrieve_project)
                    .patch(patch_project)
                    .delete(delete_project),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/archive/",
            super::handlers_project::owned_archive(
                axum::routing::post(archive_project).delete(unarchive_project),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/summary/",
            super::handlers_project::owned_summary(axum::routing::get(summary)),
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
        let pid = "11111111-1111-1111-1111-111111111111";
        for (method, uri) in [
            ("GET", "/api/v1/workspaces/acme/projects/".to_owned()),
            ("POST", "/api/v1/workspaces/acme/projects/".to_owned()),
            ("GET", format!("/api/v1/workspaces/acme/projects/{pid}/")),
            ("PATCH", format!("/api/v1/workspaces/acme/projects/{pid}/")),
            ("DELETE", format!("/api/v1/workspaces/acme/projects/{pid}/")),
            (
                "POST",
                format!("/api/v1/workspaces/acme/projects/{pid}/archive/"),
            ),
            (
                "DELETE",
                format!("/api/v1/workspaces/acme/projects/{pid}/archive/"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/acme/projects/{pid}/summary/"),
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
        assert_eq!(
            status("PUT", "/api/v1/workspaces/acme/projects/").await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status(
                "PUT",
                "/api/v1/workspaces/acme/projects/11111111-1111-1111-1111-111111111111/"
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
    }
}
