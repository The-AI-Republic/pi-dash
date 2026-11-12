//! api-v1 CLI runner delete + list (D-22, stage 5, PIDASHCONV-538; list NEWPIDASH-146; project runners PDASHOSS01-245).
//!
//! Ports the one unit this domain still needs after D-17 merged the auth
//! half (PIDASHCONV-342 #808, PIDASHCONV-343 #817):
//! `RunnerDeleteEndpoint` (`api/views/runner.py:29-57`,
//! `DELETE /api/v1/runners/<runner_id>/`; route `api/urls/runner.py:9-14`)
//! and the dedicated-runner CLI list (`RunnerListAPIEndpoint`,
//! `GET /api/v1/workspaces/<slug>/runners/`; route `api/urls/runner.py:16-20`,
//! f92fe3c0 NEWPIDASH-146) and `ProjectRunnersEndpoint` (`GET
//! /api/v1/workspaces/<slug>/projects/<project_id>/runners/`,
//! PDASHOSS01-245): the CLI's cloud runner list, sharing the web list's
//! visibility read.
//!
//! * [`runner`] — the DELETE handler (auth → lookup → guards →
//!   purge-flag parse → `delete_runner` service → 204) and the workspace
//!   runner list handler.
//! * [`project_runners`] — `ProjectRunnersEndpoint` (`GET
//!   /api/v1/workspaces/<slug>/projects/<project_id>/runners/`,
//!   PDASHOSS01-245): the CLI's cloud runner list, sharing the web list's
//!   visibility read.
//!
//! Wiring note: the crate root declares `pub mod v1_cli_auth;` (seam for
//! this issue's new files); every file under this module is new. The
//! router merges into `RouteGroup::ApiV1`; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod project_runners;
pub mod runner;

use axum::Router;

use crate::state::AppState;

/// `api/urls/runner.py:10-14` under the `api/v1/` include.
pub const RUNNER_DELETE_PATH: &str = "/api/v1/runners/{runner_id}/";
/// `api/urls/runner.py:16-20` under the `api/v1/` include (f92fe3c0).
pub const WORKSPACE_RUNNERS_LIST_PATH: &str = "/api/v1/workspaces/{slug}/runners/";
/// `api/urls/runner.py` `api-project-runners` under the `api/v1/` include
/// (`<str:slug>` / `<str:project_id>`: any non-empty segment).
pub const PROJECT_RUNNERS_PATH: &str = "/api/v1/workspaces/{slug}/projects/{project_id}/runners/";

/// Domain router: the DELETE-owned + GET-owned runner paths. Registration is the
/// cutover granularity — sibling `api/v1/` paths have no Rust route and
/// keep proxying to Django through the fallback.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            RUNNER_DELETE_PATH,
            owned_delete(axum::routing::delete(runner::delete_runner)),
        )
        .route(
            WORKSPACE_RUNNERS_LIST_PATH,
            owned_list(axum::routing::get(runner::list_runners)),
        )
        .route(
            PROJECT_RUNNERS_PATH,
            // `http_method_names=["get"]`: GET serves Rust, every other
            // method (HEAD included) proxies for Django's own 405.
            crate::v1_schedulers::owned(axum::routing::get(project_runners::project_runners)),
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

/// A GET-owned path: GET serves from Rust, every other method proxies to Django
/// (`http_method_names=["get"]`, `api/urls/runner.py:18`). HEAD is served by
/// Django via `View.setup head=get` and by Rust via axum's `get` handling;
/// the inventory records HEAD as proxied to Django so the 405/200 pair stays
/// byte-exact.
fn owned_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    router
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
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
            status("DELETE", "/api/v1/runners/12345678-1234-1234-1234-123456789012/").await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// GET serves from Rust: anonymous callers 401 at the auth layer
    /// (before any pool access or proxy dispatch).
    #[tokio::test]
    async fn owned_list_answers_401_anonymous() {
        assert_eq!(
            status("GET", "/api/v1/workspaces/ws/runners/").await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// GET serves from Rust: anonymous callers 401 at the auth layer
    /// (before any pool access or proxy dispatch).
    #[tokio::test]
    async fn owned_project_runners_answers_401_anonymous() {
        assert_eq!(
            status("GET", "/api/v1/workspaces/ws/projects/proj/runners/").await,
            StatusCode::UNAUTHORIZED
        );
    }
}
