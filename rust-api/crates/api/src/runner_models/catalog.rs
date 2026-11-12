#![forbid(unsafe_code)]

//! The signed-in catalog read (NEWPIDASH-169).
//!
//! `GET /api/runners/model-catalog/` — any signed-in user. Returns the
//! enabled models grouped by agent (`{"agents": {<agent>: [rows…]}}`),
//! in sort order, with the preselected flag. The add-runner form uses
//! this and falls back to its built-in list when the call fails, so a
//! missing table or a transient error never breaks enrolment.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::{all_rows, authed_actor, json_response, pools, Denial};

/// `GET /api/runners/model-catalog/` joins the RunnerWeb group.
/// Register the public catalog path. Sibling methods proxy to Django
/// (which owns no such path, so it answers its own 404) — the cutover
/// pattern, so a future Django import of this feature stays byte-exact.
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/runners/model-catalog/",
        get(get_catalog)
            .post(crate::edge::proxy)
            .put(crate::edge::proxy)
            .patch(crate::edge::proxy)
            .delete(crate::edge::proxy)
            .head(crate::edge::proxy)
            .options(crate::edge::proxy),
    )
}

async fn get_catalog(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pools(&state)?;
    authed_actor(&state, &pool, extension).await?;
    let rows = all_rows(&pool, true).await?;
    Ok(json_response(StatusCode::OK, &super::render_grouped(&rows)))
}
