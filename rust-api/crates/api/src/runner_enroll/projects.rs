//! Runner project list (D-13 handlers-E, PIDASHCONV-595).
//!
//! Ports `runner/views/projects.py:80-124` (`ProjectListEndpoint`), served on
//! both `GET /api/runners/projects/` (`runner/web_urls.py:111`) and `GET
//! /api/v1/runner/projects/` (`runner/urls.py:82-86`):
//!
//! * Three auth modes, in class order: Bearer [REDACTED] (runner —
//!   scoped to `runner.workspace_id`, `?workspace` ignored), `X-Api-Key`
//!   (CLI/user), session (web UI). Any `AuthenticationFailed` answers 401
//!   immediately; anonymous through all three answers the view's 401.
//! * `_serialize_projects`: one row per project with the default-first
//!   embedded pod list, `default_pod_id`, and `pod_count`.
//!
//! # Execution model
//!
//! * Mode 1 goes through the merged [`auth::authenticate_access_token`]
//!   (no URL runner id on this route, so the `mt_` branch reads
//!   `X-Runner-Id`); mode 2 through [`auth::authenticate_api_key`]
//!   (rendered with the view's first `authenticate_header`, `Bearer`);
//!   mode 3 through [`crate::license::resolve_actor`]. Denials propagate
//!   exactly as the extractors render them.
//! * SQL text comes from the merged queries builders
//!   ([`catalog_reads`](pidash_services::runner_enroll::queries::catalog_reads)
//!   J-series: pod values, projects, the `?workspace=` probe, the
//!   membership id list); this module binds the documented `$N` params
//!   positionally and owns the grouping (`default_pod_id` is first-
//!   default-wins) and the multi-workspace concatenation.
//! * `?workspace=` garbage is Django's `ValidationError` → 500; the
//!   membership probe and the id list both omit `is_active`
//!   (`BUG-membership-no-active`, ported as-is).
//!
//! # Ported bugs and quirks (translate, don't redesign; also in the PR)
//!
//! * BUG-membership-no-active (`projects.py:109-117`): the `?workspace=`
//!   probe and the no-filter id list are raw `WorkspaceMember` filters with
//!   NO `is_active` conjunct — every other D-13 membership check has one.
//! * QUIRK-workspace-concat (`projects.py:116-124`): with no filter, EVERY
//!   membership workspace is serialized and concatenated (in `-created_at`
//!   id order), even ones whose projects the caller cannot otherwise see.
//!
//! # Documented approximations
//!
//! * Unhandled failures answer the JSON 500 (`SERVER_ERROR_BODY`): Django
//!   renders its HTML error page there, so only the status is
//!   contract-pinned (the `runner_runs` precedent).
//! * Auth-failure bodies (`{"detail": code}`) propagate from the merged
//!   [`auth`] extractors verbatim; Django lowercases that key (verified
//!   against DRF 3.15.2) — tracked as a `runner_enroll::auth` fix, outside
//!   this issue's paths.
//! * Anonymous floods are not throttled: the view inherits the default
//!   `AnonRateThrottle` (30/min/IP → 429), whose Redis wiring belongs to
//!   the handler layer and has no D-13 precedent yet (first use: the
//!   handlers-A enroll endpoint). No fixture or contract test covers it.
//! * `description` decodes nullable (Django renders `None` as `null`);
//!   the column is `NOT NULL` in practice, so this only matters for
//!   legacy rows.

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use std::collections::HashMap;

use axum::extract::Extension;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Router;
use serde::Serialize;
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use pidash_services::runner_enroll::queries::catalog_reads;

use super::auth;
use crate::middleware::SessionHandle;
use crate::runner_runs::json_response;
use crate::runner_runs::pool_of;
use crate::runner_runs::server_error;
use crate::state::AppState;

use super::parse_uuid;
use super::query_param;

/// `projects/` under `/api/runners/` (`runner/web_urls.py:111`).
pub const WEB_PROJECTS_PATH: &str = "/api/runners/projects/";
/// `projects/` under `/api/v1/runner/` (`runner/urls.py:82-86`).
pub const DAEMON_PROJECTS_PATH: &str = "/api/v1/runner/projects/";

// ---------------------------------------------------------------------------
// Error bodies (D13-F7 `handlers/endpoints.golden.json`, key order verbatim)
// ---------------------------------------------------------------------------

/// `{"error": "authentication required"}` — 401, anonymous through all
/// three auth classes (`projects.py:102-106`). A view-inline body, not
/// DRF's `NotAuthenticated` rendering.
pub const AUTHENTICATION_REQUIRED_BODY: &str = r#"{"error":"authentication required"}"#;
/// `{"error": "forbidden"}` — 403, `?workspace=` without a membership row
/// (`projects.py:112-114`).
pub const FORBIDDEN_BODY: &str = r#"{"error":"forbidden"}"#;

/// An owned path: the listed methods serve from Rust, every other method
/// falls through to Django (the `app_scheduler` precedent). List `"HEAD"`
/// with the other unowned methods on GET-owned paths: axum would
/// auto-serve it from `get`, but Django 405s after auth.
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

/// Register the web project list (GET owned). Merged under
/// `RouteGroup::RunnerWeb` at the F-10 seam; sibling handler issues
/// extend the merge, keeping both sides.
pub fn web_routes() -> Router<AppState> {
    use axum::routing::get;
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    Router::new().route(WEB_PROJECTS_PATH, owned(get(get_projects), GET_ONLY))
}

/// Register the daemon project list (GET owned). Merged under
/// `RouteGroup::Runner` at the F-10 seam; sibling handler issues extend
/// the merge, keeping both sides.
pub fn daemon_routes() -> Router<AppState> {
    use axum::routing::get;
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    Router::new().route(DAEMON_PROJECTS_PATH, owned(get(get_projects), GET_ONLY))
}

// ---------------------------------------------------------------------------
// Shapes (`projects.py:31-77`, D13-F2; key order verbatim)
// ---------------------------------------------------------------------------

/// One embedded pod (`projects.py:52-58`): `{id, name, is_default}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PodEntry {
    id: String,
    name: String,
    is_default: bool,
}

/// One project row (`projects.py:61-73`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProjectRow {
    id: String,
    identifier: String,
    name: String,
    description: Option<String>,
    is_default: bool,
    default_pod_id: Option<String>,
    pod_count: usize,
    pods: Vec<PodEntry>,
}

/// Group pod value-rows by project, preserving row order (J1 assembly
/// rule): each entry is `{id: str, name, is_default}`; `default_pod_id`
/// is the FIRST row with `is_default` per project (a second default row
/// does NOT overwrite); `pod_count` is the group length.
fn group_pods(
    rows: Vec<(Uuid, bool, Uuid, String)>,
) -> HashMap<Uuid, (Vec<PodEntry>, Option<Uuid>)> {
    let mut groups: HashMap<Uuid, (Vec<PodEntry>, Option<Uuid>)> = HashMap::new();
    for (project_id, is_default, id, name) in rows {
        let entry = PodEntry {
            id: id.to_string(),
            name,
            is_default,
        };
        let group = groups
            .entry(project_id)
            .or_insert_with(|| (Vec::new(), None));
        group.0.push(entry);
        if is_default && group.1.is_none() {
            group.1 = Some(id);
        }
    }
    groups
}

/// `_serialize_projects` (`projects.py:31-77`): the pod-values query
/// (default-first, name order) grouped above, then one row per project
/// (identifier order). Projects without pods get `default_pod_id: None`,
/// `pod_count: 0`, `pods: []`.
async fn serialize_projects(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<ProjectRow>, Response> {
    let pod_rows: Vec<(Uuid, bool, Uuid, String)> =
        sqlx::query_as(&catalog_reads::workspace_pods_values_sql())
            .bind(workspace_id)
            .fetch_all(pool)
            .await
            .map_err(|_| server_error())?;
    let groups = group_pods(pod_rows);
    // Full-row select in `project` column order; only id(5), name(6),
    // description(7), identifier(12), and is_default(24) are read.
    let project_rows = sqlx::query(&catalog_reads::workspace_projects_sql())
        .bind(workspace_id)
        .fetch_all(pool)
        .await
        .map_err(|_| server_error())?;
    let mut out = Vec::with_capacity(project_rows.len());
    for row in project_rows {
        let id: Uuid = row.try_get(5).map_err(|_| server_error())?;
        let name: String = row.try_get(6).map_err(|_| server_error())?;
        let description: Option<String> = row.try_get(7).map_err(|_| server_error())?;
        let identifier: String = row.try_get(12).map_err(|_| server_error())?;
        let is_default: bool = row.try_get(24).map_err(|_| server_error())?;
        let (pods, default_pod_id) = groups.get(&id).cloned().unwrap_or_default();
        out.push(ProjectRow {
            id: id.to_string(),
            identifier,
            name,
            description,
            is_default,
            default_pod_id: default_pod_id.map(|pod| pod.to_string()),
            pod_count: pods.len(),
            pods,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// GET (`projects.py:97-124`)
// ---------------------------------------------------------------------------

async fn get_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    extension: Option<Extension<SessionHandle>>,
    Query(params): Query<crate::license::QueryMap>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(denial) => return denial,
    };
    let secret = state.settings().secret_key.clone();

    // Mode 1: runner access token (`:98-100`). A presented-but-invalid
    // credential 401s immediately (DRF never falls through a raise);
    // `None` (no Bearer [REDACTED] to the next class. There is no URL
    // runner id on this route, so the `mt_` branch reads `X-Runner-Id`.
    // Stock settings carry no `RUNNER_ACCESS_TOKEN_KEYS` (`[]`), so the
    // ring is the derived `"default"` key (`tokens.py:108-110`).
    let secret_str = String::from_utf8_lossy(secret.as_bytes()).into_owned();
    let ring = pidash_services::runner_enroll::tokens::build_key_ring(&[], &secret_str);
    match auth::authenticate_access_token(
        &pool,
        secret.as_bytes(),
        &ring,
        &headers,
        None,
        auth::ALLOW_GET,
    )
    .await
    {
        Err(denial) => return denial,
        Ok(Some(authenticated)) => {
            let rows = match serialize_projects(&pool, authenticated.runner.workspace_id).await {
                Ok(rows) => rows,
                Err(failure) => return failure,
            };
            return json_response(
                StatusCode::OK,
                serde_json::to_string(&rows).expect("json body"),
            );
        }
        Ok(None) => {}
    }

    // Modes 2/3 share the membership path (`:102-124`).
    let user_id = match auth::authenticate_api_key(&pool, secret.as_bytes(), &headers).await {
        Err(failure) => return failure,
        Ok(auth::ApiKeyOutcome::Authenticated(authenticated)) => authenticated.user_id,
        Ok(auth::ApiKeyOutcome::Invalid) => {
            return auth::auth_failure_response(
                auth::CODE_GIVEN_API_TOKEN_NOT_VALID,
                Some(auth::AUTHENTICATE_HEADER_BEARER),
                auth::ALLOW_GET,
            );
        }
        Ok(auth::ApiKeyOutcome::Missing) => {
            match crate::license::resolve_actor(&pool, secret.as_bytes(), extension).await {
                Err(_) => return server_error(),
                Ok(Some(actor)) => actor.id,
                Ok(None) => {
                    return json_response(
                        StatusCode::UNAUTHORIZED,
                        AUTHENTICATION_REQUIRED_BODY.to_owned(),
                    );
                }
            }
        }
    };

    if let Some(ws_filter) = query_param(&params, "workspace") {
        // `?workspace=` given: membership probe WITHOUT `is_active`
        // (`BUG-membership-no-active`); garbage UUIDs 500.
        let workspace_id = match parse_uuid(&ws_filter) {
            Ok(id) => id,
            Err(failure) => return failure,
        };
        // `$1` member, `$2` workspace.
        let member: Option<i32> =
            match sqlx::query_scalar(&catalog_reads::workspace_membership_probe_sql())
                .bind(user_id)
                .bind(workspace_id)
                .fetch_optional(&pool)
                .await
            {
                Ok(row) => row,
                Err(_) => return server_error(),
            };
        if member.is_none() {
            return json_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned());
        }
        let rows = match serialize_projects(&pool, workspace_id).await {
            Ok(rows) => rows,
            Err(failure) => return failure,
        };
        return json_response(
            StatusCode::OK,
            serde_json::to_string(&rows).expect("json body"),
        );
    }

    // No filter: serialize EVERY membership workspace and concatenate
    // (`QUIRK-workspace-concat`; no `is_active` filter, id-list order).
    let workspace_ids: Vec<Uuid> =
        match sqlx::query_scalar(&catalog_reads::member_workspace_ids_sql())
            .bind(user_id)
            .fetch_all(&pool)
            .await
        {
            Ok(ids) => ids,
            Err(_) => return server_error(),
        };
    let mut out = Vec::new();
    for workspace_id in workspace_ids {
        match serialize_projects(&pool, workspace_id).await {
            Ok(rows) => out.extend(rows),
            Err(failure) => return failure,
        }
    }
    json_response(
        StatusCode::OK,
        serde_json::to_string(&out).expect("json body"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SHAPES: &str =
        include_str!("../../../../fixtures/runner_enroll/serializers/shapes.golden.json");
    const FIXTURE_ENDPOINTS: &str =
        include_str!("../../../../fixtures/runner_enroll/handlers/endpoints.golden.json");

    /// J1 grouping: row order preserved, first-default-wins, groups
    /// keyed by project.
    #[test]
    fn group_pods_first_default_wins() {
        let project_a = Uuid::new_v4();
        let project_b = Uuid::new_v4();
        let pod_default = Uuid::new_v4();
        let pod_second_default = Uuid::new_v4();
        let pod_plain = Uuid::new_v4();
        let pod_other = Uuid::new_v4();
        let groups = group_pods(vec![
            (project_a, true, pod_default, "a-default".to_owned()),
            (project_a, true, pod_second_default, "a-second".to_owned()),
            (project_a, false, pod_plain, "a-plain".to_owned()),
            (project_b, false, pod_other, "b-plain".to_owned()),
        ]);
        assert_eq!(groups.len(), 2);
        let (pods_a, default_a) = groups.get(&project_a).expect("group a").clone();
        // Row order preserved (default-first comes from the SQL order).
        assert_eq!(pods_a.len(), 3);
        assert_eq!(pods_a[0].id, pod_default.to_string());
        assert_eq!(pods_a[0].name, "a-default");
        assert!(pods_a[0].is_default);
        assert_eq!(pods_a[1].id, pod_second_default.to_string());
        assert_eq!(pods_a[2].id, pod_plain.to_string());
        assert!(!pods_a[2].is_default);
        // First default wins; the second does NOT overwrite.
        assert_eq!(default_a, Some(pod_default));
        let (pods_b, default_b) = groups.get(&project_b).expect("group b").clone();
        assert_eq!(pods_b.len(), 1);
        assert_eq!(default_b, None);
    }

    /// The project row keeps the source key order, with `null` for a
    /// missing default pod.
    #[test]
    fn project_row_key_order() {
        let row = ProjectRow {
            id: "00000000-0000-0000-0000-000000000001".to_owned(),
            identifier: "WEB".to_owned(),
            name: "Web".to_owned(),
            description: Some("desc".to_owned()),
            is_default: true,
            default_pod_id: None,
            pod_count: 0,
            pods: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&row).expect("json"),
            "{\"id\":\"00000000-0000-0000-0000-000000000001\",\"identifier\":\"WEB\",\"name\":\"Web\",\"description\":\"desc\",\"is_default\":true,\"default_pod_id\":null,\"pod_count\":0,\"pods\":[]}"
        );
        let pod = PodEntry {
            id: "00000000-0000-0000-0000-000000000002".to_owned(),
            name: "WEB_pod_1".to_owned(),
            is_default: true,
        };
        assert_eq!(
            serde_json::to_string(&pod).expect("json"),
            "{\"id\":\"00000000-0000-0000-0000-000000000002\",\"name\":\"WEB_pod_1\",\"is_default\":true}"
        );
    }

    /// A `None` description renders `null` (Django renders `None` as
    /// `null`; the column is `NOT NULL` in practice).
    #[test]
    fn project_row_null_description() {
        let row = ProjectRow {
            id: "id".to_owned(),
            identifier: "W".to_owned(),
            name: "n".to_owned(),
            description: None,
            is_default: false,
            default_pod_id: Some("p".to_owned()),
            pod_count: 1,
            pods: vec![PodEntry {
                id: "p".to_owned(),
                name: "n".to_owned(),
                is_default: false,
            }],
        };
        let body = serde_json::to_string(&row).expect("json");
        assert!(body.contains("\"description\":null"), "{body}");
    }

    /// D13-F2 tie-in: the embedded pod entries are the first three keys
    /// of the `PodMiniSerializer` shape (`id`, `name`, `is_default`) —
    /// the view builds its own dict rather than the serializer.
    #[test]
    fn pod_entry_matches_f2_mini_prefix() {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_SHAPES).expect("shapes fixture parses");
        let key_order = fixture["pod_mini"]["key_order"]
            .as_array()
            .expect("pod_mini key_order");
        let keys: Vec<&str> = key_order
            .iter()
            .map(|key| key.as_str().expect("key str"))
            .collect();
        assert_eq!(keys[..3], ["id", "name", "is_default"]);
        // And the port renders exactly those three, in that order.
        let pod = PodEntry {
            id: "id".to_owned(),
            name: "name".to_owned(),
            is_default: false,
        };
        assert_eq!(
            serde_json::to_string(&pod).expect("json"),
            "{\"id\":\"id\",\"name\":\"name\",\"is_default\":false}"
        );
    }

    /// D13-F7 projects errors: anonymous 401 and `?workspace=` 403 match
    /// the golden bodies on both routes (same view).
    #[test]
    fn f7_projects_errors_match_consts() {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_ENDPOINTS).expect("endpoints fixture parses");
        assert_eq!(
            fixture["daemon"]["GET_projects"]["route"],
            "GET /api/v1/runner/projects/"
        );
        assert_eq!(
            fixture["web"]["GET_projects"]["route"],
            "GET /api/runners/projects/"
        );
        let errors = fixture["web"]["GET_projects"]["errors"]
            .as_array()
            .expect("errors array");
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0]["status"], 401);
        assert_eq!(
            serde_json::to_string(&errors[0]["body"]).expect("json"),
            AUTHENTICATION_REQUIRED_BODY
        );
        assert_eq!(errors[1]["status"], 403);
        assert_eq!(
            serde_json::to_string(&errors[1]["body"]).expect("json"),
            FORBIDDEN_BODY
        );
    }

    /// Both routes are registered at the exact Django mount paths.
    #[test]
    fn route_paths_match_django_mounts() {
        assert_eq!(WEB_PROJECTS_PATH, "/api/runners/projects/");
        assert_eq!(DAEMON_PROJECTS_PATH, "/api/v1/runner/projects/");
        assert_eq!(
            super::super::desktop::DESKTOP_ENROLL_PATH,
            "/api/v1/runner/dev-machines/desktop-enroll/"
        );
    }
}
