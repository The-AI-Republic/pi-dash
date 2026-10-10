//! D-36 scheduler permission gates + feature-flag guard (stage 5, PIDASHCONV-632).
//!
//! Ports the `@allow_permission` gates on the 5 scheduler routes
//! (`apps/api/pi_dash/app/urls/scheduler.py:16-46`) and the
//! `_feature_enabled` kill switch
//! (`apps/api/pi_dash/app/views/scheduler/views.py:32-40`). Fixture:
//! `rust-api/fixtures/app_scheduler/guards/permissions.golden.json`
//! (F36-09, trace: `rust-api/fixtures/app_scheduler/TRACE.md`).
//!
//! Shape of the port, following the [`crate::app_pages`] `gate.rs`
//! precedent: the decision kernel lives in the read-only F-06
//! foundation (`pidash_auth::permissions::allow::{decide_allow, ...}`);
//! [`gate`] pins which gate each route carries, adds the async tenant
//! row fetching (handlers call into it — no handler takes an unscoped
//! database handle for these routes), and owns the byte-exact denial
//! bodies.
//!
//! The handler shells (`handlers_sched.rs`, `handlers_bind.rs`,
//! `handlers_occ.rs`, PIDASHCONV-633…635) land in this module and call
//! [`gate::resolve_gate`], then [`gate::ensure_feature_enabled`] on the
//! 4 CRUD routes (never on occurrences — ported quirk, see
//! [`gate::DISABLED_BODY`).
//!
//! Shared request plumbing (`pool_of`, [`resolve_project_id`],
//! [`session_authenticated`], [`json_response`], [`owned`], [`routes`])
//! lives here so the three handler issues reuse it; each handler file
//! owns its own view-level `Denial` + exact bodies. Sibling issues extend
//! [`routes`] with their own paths (merges keep every side's routes) and
//! add their `pub mod` lines below (merges keep every side's lines).

pub mod gate;
pub mod handlers_bind;
pub mod handlers_occ;
pub mod handlers_sched;

use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::{Extension, Router};
use sqlx::PgPool;

use crate::middleware::SessionHandle;
use crate::state::AppState;

/// `Project.resolve` miss (`db/models/project.py:213-217`): verbatim
/// `Http404` args through DRF's handler (no trailing period). Shared by
/// the project-level handler files (bindings + occurrences).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

fn json_body(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("static scheduler response")
}

/// Render a JSON success/error body with the DRF content type.
pub fn json_response(status: StatusCode, body: String) -> Response {
    json_body(status, &body)
}

/// The request pool, or the `handle_exception` 500 when the server runs
/// pool-less (unreachable in `serve`, which fail-fasts at boot).
#[allow(clippy::result_large_err)]
pub fn pool_of(state: &AppState) -> Result<&PgPool, Response> {
    match state.pools().map(|pools| pools.primary()) {
        Some(pool) => Ok(pool),
        None => Err(json_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            gate::SERVER_ERROR_BODY,
        )),
    }
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// `_rewrite_project_kwarg` + `Project.resolve` (`app/views/base.py:48-81`,
/// `db/models/project.py:192-220`): UUIDs pass through unchecked (the
/// gate 403s unknown ones — the row check happens in the view body, the
/// `app_issues`/`app_cycles` precedent); other identifiers match
/// `UPPER(identifier)` after trimming; misses answer 404
/// [`PROJECT_NOT_FOUND_BODY`]. Callers run this for authenticated
/// requests only (anonymous callers 401 inside [`gate::resolve_gate`],
/// never 404 here) — see [`session_authenticated`].
#[allow(clippy::result_large_err)]
pub async fn resolve_project_id(
    pool: &PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Response> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| json_body(StatusCode::INTERNAL_SERVER_ERROR, gate::SERVER_ERROR_BODY))?;
    row.map(|row| row.0)
        .ok_or_else(|| json_body(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY))
}

/// Whether the request looks authenticated: the session carries a UUID
/// `_auth_user_id` for the model backend. These checks are a strict
/// subset of [`crate::license::resolve_actor`]'s success conditions (which
/// additionally needs the user row, `is_active`, and the session hash),
/// so a `false` here always 401s inside [`gate::resolve_gate`] — the peek
/// can never send an authenticated caller down the anonymous path. It
/// exists only to order the project rewrite after auth, like Django's
/// `initial` (rewrite skipped for anonymous callers, who 401 in the
/// gate instead of 404ing on the project id).
pub fn session_authenticated(extension: &Option<Extension<SessionHandle>>) -> bool {
    let Some(Extension(handle)) = extension else {
        return false;
    };
    let mut session = handle.snapshot();
    let user_id = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();
    let backend = session
        .get("_auth_user_backend")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();
    backend == crate::license::MODEL_BACKEND && user_id.parse::<uuid::Uuid>().is_ok()
}

/// An owned path: the owned methods serve from Rust, every other method
/// falls through to Django (its 405-after-auth, metadata, and CSRF-failure
/// responses live there — answering 405 in Rust would mistranslate the
/// body). The [`crate::app_pages`] precedent.
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the scheduler routes (`app/urls/scheduler.py`): each handler
/// file exposes its own `routes()` (sibling D-36 handler issues extend
/// this merge; merges keep both sides), merged here for the F-10 overlay
/// seam.
pub fn routes() -> Router<AppState> {
    handlers_occ::routes()
        .merge(handlers_sched::routes())
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
            owned(
                axum::routing::get(handlers_bind::binding_list)
                    .post(handlers_bind::binding_install),
                &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{binding_id}/",
            owned(
                axum::routing::get(handlers_bind::binding_detail)
                    .patch(handlers_bind::binding_patch)
                    .delete(handlers_bind::binding_uninstall),
                &["PUT", "POST", "HEAD", "OPTIONS"],
            ),
        )
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
