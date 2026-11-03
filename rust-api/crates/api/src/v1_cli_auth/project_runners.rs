//! `GET /api/v1/workspaces/<slug>/projects/<project_id>/runners/` —
//! read-only cloud runner list for the CLI (`pidash project runners`).
//!
//! Ports `ProjectRunnersEndpoint.get` (`api/views/runner.py`, route
//! `api-project-runners` in `api/urls/runner.py`; PDASHOSS01-245). The
//! visibility and filter rules live in the shared
//! [`project_runners_body`](crate::runner_enroll::manage) read — the Rust
//! twin of `runner/services/runner_directory.project_runners_queryset` —
//! which the session-auth web list (`GET /api/runners/`) also calls, so
//! the two surfaces cannot drift apart.
//!
//! Handler order mirrors DRF `initial()` + the view body:
//!
//! 1. `APIKeyAuthentication` — no `X-Api-Key` → 401, bad token → 403
//!    `Given API token is not valid` (the shared `v1_schedulers` preamble).
//! 2. `_rewrite_project_kwarg` (`api/views/base.py`) — an identifier/slug
//!    resolves to its UUID or 404s `{"detail":"Project not found"}`; a
//!    UUID-looking value passes through unchecked.
//! 3. `TimezoneMixin` — the user's zone (unknown → 400 `KeyError` body,
//!    empty → 500) renders every datetime in the response.
//! 4. `Project.objects.filter(pk, workspace__slug, deleted_at__isnull)
//!    .first()` — miss → 404 `{"error":"not found"}`.
//! 5. The shared read: non-member → 403 `{"error":"forbidden"}`; a
//!    non-UUID `?pod=` raises Django's `ValidationError` inside the view,
//!    which `BaseAPIView.handle_exception` answers 400 `{"error":"Please
//!    provide valid detail"}`.
//! 6. 200 with the bare `RunnerSerializer(many=True)` array.
//!
//! Not ported (verified-not-ported, the `v1_cli_auth::runner` precedent):
//! `ApiKeyRateThrottle` — cross-cutting infra no merged v1 handler
//! enforces.
//!
//! Approximation: unhandled failures answer the JSON 500 while Django
//! renders the same `{"error": …}` body via `handle_exception` (exact).

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_services::runner_enroll::queries::manage_reads;

use crate::runner_enroll::manage::{project_runners_body, DirectoryError, DirectoryFilters};
use crate::state::AppState;
use crate::v1_schedulers::{
    activate_timezone, preamble, query_last, rewrite_project_id, Denial, QueryMap,
    SERVER_ERROR_BODY,
};

/// `{"error": "not found"}` — the view-inline 404 (project UUID unknown
/// in this workspace, or soft-deleted).
pub const NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;
/// `{"error": "forbidden"}` — the view-inline 403 (`PermissionDenied`
/// from the shared read: caller is not a workspace member).
pub const FORBIDDEN_BODY: &str = r#"{"error":"forbidden"}"#;
/// `BaseAPIView.handle_exception`'s `ValidationError` branch.
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;

/// `Project.objects.filter(pk=project_id, workspace__slug=slug,
/// deleted_at__isnull=True).first()` projected to `workspace_id` (the
/// only column the view reads; the manager scope and the explicit
/// `deleted_at__isnull` collapse to one conjunct, and `pk` is unique so
/// `.first()`'s ordering is moot). `$1` = project id, `$2` = slug.
const PROJECT_LOOKUP_SQL: &str = r#"SELECT "projects"."workspace_id" FROM "projects" INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id") WHERE ("projects"."deleted_at" IS NULL AND "projects"."id" = $1 AND "workspaces"."slug" = $2) LIMIT 1"#;

fn json_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("handler response")
}

/// The handler: see the module docs for the order.
pub async fn project_runners(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    match project_runners_inner(&state, &headers, &slug, &project_raw, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn project_runners_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let Some(workspace_id) = project_workspace(&pre.pool, project_id, slug).await? else {
        return Ok(json_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY));
    };
    let filters = DirectoryFilters {
        project: Some(project_id.to_string()),
        pod: query_last(query, "pod"),
        include_bundled: manage_reads::include_bundled(
            query_last(query, "include_bundled").as_deref(),
        ),
    };
    Ok(
        match project_runners_body(&pre.pool, pre.actor.id, workspace_id, &filters, &tz).await {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(body))
                .expect("json response"),
            Err(DirectoryError::Forbidden) => json_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY),
            Err(DirectoryError::InvalidFilter) => {
                json_response(StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY)
            }
            Err(DirectoryError::Server) => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
            }
        },
    )
}

/// Step 4: the project's workspace, or `None` for a miss.
async fn project_workspace(
    pool: &PgPool,
    project_id: Uuid,
    slug: &str,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(PROJECT_LOOKUP_SQL)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|error| crate::v1_schedulers::db_error(error, "project-runners-project"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bodies are the Django view's / `handle_exception`'s, byte for
    /// byte (`json.dumps` compact rendering via DRF's `JSONRenderer`).
    #[test]
    fn bodies_match_django() {
        assert_eq!(NOT_FOUND_BODY, r#"{"error":"not found"}"#);
        assert_eq!(FORBIDDEN_BODY, r#"{"error":"forbidden"}"#);
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
    }

    /// The lookup keeps every conjunct of the Django filter (manager
    /// scope + `pk` + `workspace__slug` + the explicit
    /// `deleted_at__isnull`).
    #[test]
    fn project_lookup_scopes_workspace_and_soft_delete() {
        assert!(PROJECT_LOOKUP_SQL.contains(r#""projects"."id" = $1"#));
        assert!(PROJECT_LOOKUP_SQL.contains(r#""workspaces"."slug" = $2"#));
        assert!(PROJECT_LOOKUP_SQL.contains(r#""projects"."deleted_at" IS NULL"#));
        assert!(PROJECT_LOOKUP_SQL.ends_with("LIMIT 1"));
    }
}
