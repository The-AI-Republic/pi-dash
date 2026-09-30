//! User-server-asset handlers (D-21, stage 5, PIDASHCONV-419).
//!
//! Ports `UserServerAssetEndpoint`
//! (`apps/api/pi_dash/api/views/asset.py:246-400`) with routes from
//! `apps/api/pi_dash/api/urls/asset.py:24-33`:
//!
//! * `POST assets/user-assets/server/` (`asset.py:282-347`)
//! * `PATCH assets/user-assets/<uuid:asset_id>/server/` (`asset.py:359-376`)
//! * `DELETE assets/user-assets/<uuid:asset_id>/server/` (`asset.py:387-400`)
//! * helpers twin (`asset.py:249-271` — shared with unit 4, one
//!   implementation; see below)
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-h-asset-server.json`
//! (`fx-h-asset-server`; trace: `rust-api/fixtures/v1_assets/TRACE.md`).
//!
//! The server trio duplicates the user trio line-for-line except
//! `S3Storage(request=request, is_server=True)` (`asset.py:336` vs
//! `:163`), so this module is thin by design: the POST calls the shared
//! [`super::asset_user`] core with `is_server=true`, and PATCH/DELETE
//! (whose bodies never touch storage) call the identical shared core.
//! Error bodies, guards, lookups, saves, the metadata enqueue and both
//! helpers are the one user-trio implementation — the helpers twin is
//! shared, not copied.
//!
//! Route registration is the cutover granularity (Porting guide cutover
//! row): [`routes`] serves the three owned methods; every other method
//! falls through to [`crate::edge::proxy`] so Django answers the 405s,
//! OPTIONS metadata and resolver 404s exactly as before. Mount wiring
//! (merging [`routes`] into the app router) belongs to PIDASHCONV-426 —
//! this module must not touch `overlay.rs` / `routes.rs`.
//!
//! Ported bugs: identical to the user trio (see [`super::asset_user`];
//! also listed in the PR) — the guard literals, the `int(size)`-before-
//! guards 500, the `<hex>-None` key, the auth-only surface and the
//! `is_deleted`-free lookups all come from the shared core untouched.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Uri};
use axum::response::Response;
use axum::routing::post;
use axum::Router;

use crate::state::AppState;

/// Register the user-server-asset routes. PIDASHCONV-426 merges this
/// router into the app router; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/assets/user-assets/server/",
            post(server_post).fallback(crate::edge::proxy),
        )
        .route(
            "/api/v1/assets/user-assets/{asset_id}/server/",
            axum::routing::patch(server_patch)
                .delete(server_delete)
                .fallback(crate::edge::proxy),
        )
}

/// Mint a user asset row with server credentials and answer the
/// presigned upload POST (`asset.py:282-347`).
async fn server_post(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    super::asset_user::handle_post(state, method, uri, headers, body, true).await
}

/// Mark the asset uploaded (`asset.py:359-376`) — identical to the user
/// PATCH; the twin shares the core verbatim.
async fn server_patch(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    super::asset_user::handle_patch(state, method, uri, headers, &asset_raw, body).await
}

/// Soft-delete the asset (`asset.py:387-400`) — identical to the user
/// DELETE; the twin shares the core verbatim.
async fn server_delete(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    super::asset_user::handle_delete(state, headers, &asset_raw).await
}

#[cfg(test)]
mod tests {
    use super::super::asset_user;

    // fx-h-asset-server `error_bodies`: IDENTICAL to the user trio — the
    // server module reuses the shared literals, so a drift in either
    // breaks both suites at the same line.
    #[test]
    fn server_error_bodies_are_the_shared_literals() {
        assert_eq!(
            asset_user::INVALID_ENTITY_BODY,
            r#"{"error":"Invalid entity type.","status":false}"#
        );
        assert_eq!(
            asset_user::INVALID_TYPE_BODY,
            r#"{"error":"Invalid file type. Only JPEG and PNG files are allowed.","status":false}"#
        );
        assert_eq!(
            asset_user::NOT_FOUND_BODY,
            r#"{"error":"The requested resource does not exist."}"#
        );
    }

    // The server trio admits the same entity + MIME sets (the guards at
    // `asset.py:299,306-312` match `:126,133-139` line for line).
    #[test]
    fn server_guards_match_user_trio() {
        assert_eq!(
            asset_user::USER_ENTITY_TYPES,
            &["USER_AVATAR", "USER_COVER"]
        );
        assert_eq!(asset_user::USER_MIME_TYPES.len(), 5);
    }
}
