//! D-33 app-integrations permission layer (stage 5, PIDASHCONV-436).
//!
//! [`gates`] ports the `allow_permission` role matrix, the two `AllowAny`
//! endpoints, and the manual admin checks; [`hmac`] ports the
//! `verify_webhook_signature` HMAC-SHA256 guard. Both modules are pure:
//! handlers fetch membership rows through the workspace-scoped handle and
//! decide here.
//!
//! [`handlers_external`] owns the external-integration HTTP shell
//! (PIDASHCONV-454): the two AI-assistant POSTs and the Unsplash GET.
//! [`handlers_github_proj`] owns the project-level GitHub HTTP shell
//! (PIDASHCONV-450): bind POST plus status GET/PATCH/DELETE. Sibling
//! handler issues merge their own routers into [`routes`]; merges keep
//! both sides.

pub mod gates;
pub mod handlers_external;
pub mod handlers_git_repo;
pub mod handlers_github_proj;
pub mod handlers_github_ws;
pub mod hmac;

use axum::Router;

use crate::state::AppState;

/// Owned D-33 app-integration routes (cutover granularity: registered
/// paths serve from Rust, everything else keeps proxying). Sibling
/// handler issues extend this merge; merges keep both sides.
pub fn routes() -> Router<AppState> {
    handlers_external::routes()
        .merge(handlers_github_proj::routes())
        .merge(handlers_github_ws::routes())
        .merge(handlers_git_repo::routes())
}
