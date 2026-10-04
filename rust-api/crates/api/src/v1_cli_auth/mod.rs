//! api-v1 CLI runner delete (D-22, stage 5, PIDASHCONV-538).
//!
//! Ports the one unit this domain still needs after D-17 merged the auth
//! half (PIDASHCONV-342 #808, PIDASHCONV-343 #817):
//! `RunnerDeleteEndpoint` (`api/views/runner.py:29-57`,
//! `DELETE /api/v1/runners/<runner_id>/`; route `api/urls/runner.py:9-14`).
//!
//! * [`runner`] — the DELETE handler (auth → lookup → guards →
//!   purge-flag parse → `delete_runner` service → 204).
//!
//! Wiring note: the crate root declares `pub mod v1_cli_auth;` (seam for
//! this issue's new files); every file under this module is new. The
//! router merges into `RouteGroup::ApiV1`; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod runner;

use axum::Router;

use crate::state::AppState;

/// `api/urls/runner.py:10-14` under the `api/v1/` include.
pub const RUNNER_DELETE_PATH: &str = "/api/v1/runners/{runner_id}/";

/// Domain router: the DELETE-owned runner path. Registration is the
/// cutover granularity — sibling `api/v1/` paths have no Rust route and
/// keep proxying to Django through the fallback.
pub fn routes() -> Router<AppState> {
    Router::new().route(
        RUNNER_DELETE_PATH,
        owned_delete(axum::routing::delete(runner::delete_runner)),
    )
}

/// A DELETE-owned path (the pilot `owned()` pattern): DELETE serves from
/// Rust, every other method proxies to Django so its own 405 answers byte
/// for byte (`http_method_names=["delete"]`, `api/urls/runner.py:12`).
fn owned_delete(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    router
        .get(crate::edge::proxy)
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// DELETE serves from Rust: anonymous callers 401 at the auth layer
    /// (never a proxy 502), before any pool access.
    #[tokio::test]
    async fn owned_delete_answers_401_anonymous() {
        assert_eq!(
            status(
                "DELETE",
                "/api/v1/runners/11111111-1111-1111-1111-111111111111/"
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// A non-UUID segment 404s at the resolver level, before auth — the
    /// `<uuid:>` converter rejects it in Django before any view code runs.
    #[tokio::test]
    async fn non_uuid_segment_404s_before_auth() {
        assert_eq!(
            status("DELETE", "/api/v1/runners/not-a-uuid/").await,
            StatusCode::NOT_FOUND
        );
    }

    /// Unowned methods proxy to Django (502 fail-closed with the test
    /// edge), preserving its 405-after-auth byte for byte.
    #[tokio::test]
    async fn unowned_methods_proxy() {
        let uri = "/api/v1/runners/11111111-1111-1111-1111-111111111111/";
        for method in ["GET", "POST", "PUT", "PATCH"] {
            assert_eq!(
                status(method, uri).await,
                StatusCode::BAD_GATEWAY,
                "{method}"
            );
        }
    }
}
