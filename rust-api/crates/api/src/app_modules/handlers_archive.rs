#![forbid(unsafe_code)]

//! Module archive handlers (D-28, stage 5, PIDASHCONV-407).
//!
//! Ports `ModuleArchiveUnarchiveEndpoint` from
//! `apps/api/pi_dash/app/views/module/archive.py` (all five route slots,
//! `permission_classes = [ProjectEntityPermission]`):
//!
//! - `GET modules/<module_id>/archive/`: the ported 500 — `get()`
//!   takes no `module_id` kwarg (`:258`), so dispatch raises `TypeError`
//!   after the gate.
//! - `GET archived-modules/`: archived rows as a bare array,
//!   `-is_favorite, -created_at` (`:20-115`).
//! - `GET archived-modules/<pk>/`: the `ModuleDetailSerializer` shell plus
//!   the estimate/distribution blocks (`:117-263`) — with no 404: a miss
//!   renders the `None`-instance shell.
//! - `POST modules/<module_id>/archive/`: archive (completed/cancelled
//!   only), stamp `archived_at`, soft-delete the module's favorites,
//!   answer `{"archived_at": str(...)}` (`:544-559`).
//! - `DELETE modules/<module_id>/archive/`: unarchive, 204 (`:561-565`).
//!
//! Every other method on those paths proxies to Django.
//!
//! Fixture ids: FX-MOD-06
//! (`rust-api/fixtures/app_modules/handlers/favorites_userprops_archive.golden.json`);
//! gates via `super::gates` (FX-MOD-04, PIDASHCONV-379).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `GET modules/<module_id>/archive/` answers 500, not the module.
//! * The archive queryset's `member_ids` keeps soft-deleted members
//!   (`archive.py:235-241` lacks the `~Q(members__id__isnull)` deleted-at
//!   guard `base.py:217-222` has).
//! * The label estimate-distribution omits the
//!   `issue_module__deleted_at__isnull` filter (`archive.py:386-391`)
//!   every sibling distribution has.
//! * The distribution `completion_chart` renders with zero issues
//!   (`archive.py:533` lacks the `total_issues > 0` check `base.py:632`
//!   has).
//! * The archived detail has no 404: a miss answers 200 with the
//!   `None`-instance shell `{"member_ids": []}` plus the (empty)
//!   distribution blocks.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Router;
use serde_json::{Map, Value};

use pidash_services::app_modules::queries;

use super::gates;
use super::handlers_module_links::{
    actor_or_401, check_class_gate, fetch_class_facts, resolve_project_or_404,
};
use super::handlers_modules::{
    annotation_selects, empty_response, fetch_assignee_distribution, fetch_burndown_chart,
    fetch_estimate_type, fetch_json_maps, fetch_label_distribution, fetch_module_links,
    fetch_sub_issues, json_response, parse_uuid_or_invalid, pool_of, render_distribution_array,
    render_float, shape_link, shape_value, shift_datetime, HandlerResult, ASSIGNEE_COUNT_ORDER,
    ASSIGNEE_ESTIMATE_ORDER, DETAIL_ROW_ORDER, MODULE_BASE_COLUMNS,
};
use crate::app_issues::Denial;
use crate::state::AppState;

/// Archive path in `app/urls/module.py:90-94` form.
pub const ARCHIVE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/";
/// Archived collection path in `app/urls/module.py:95-99` form.
pub const ARCHIVED_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/archived-modules/";
/// Archived detail path in `app/urls/module.py:100-104` form.
pub const ARCHIVED_DETAIL_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/";

/// Owned methods per path (mirroring `urls/module.py`); every other
/// method proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            ARCHIVE_PATH,
            axum::routing::get(archive_get)
                .post(archive_post)
                .delete(archive_delete)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            ARCHIVED_PATH,
            axum::routing::get(archived_list)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            ARCHIVED_DETAIL_PATH,
            axum::routing::get(archived_detail)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

fn gate_for(method: &str, path: &str) -> &'static gates::Gate {
    &gates::gate_for(method, path)
        .unwrap_or_else(|| panic!("D-28 gate for {method} {path}"))
        .gate
}

// ---------------------------------------------------------------------------
// Archived rows (`archive.py:45-256`, `get_queryset`)
// ---------------------------------------------------------------------------

/// Archived-list wire order (verified live): the `.values()` call
/// (`archive.py:244-272`) lists `member_ids` mid-call, but Django renders
/// concrete model fields first (in call order) and the requested
/// annotations after (in queryset annotation order).
const ARCHIVED_ROW_ORDER: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "created_at",
    "updated_at",
    "archived_at",
    "is_favorite",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_issues",
    "member_ids",
];

/// Render one archived row: `created_at`/`updated_at` convert to the
/// request zone (`user_timezone_converter`, `:275-279`) but `archived_at`
/// is outside its field list, so it renders in UTC — while the detail
/// shell (serializer path) renders all three in the request zone
/// (verified live with a non-UTC user).
fn shape_archived_row(row: &Map<String, Value>, timezone: &chrono_tz::Tz) -> String {
    let mut out = String::from("{");
    for (index, field) in ARCHIVED_ROW_ORDER.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        let value = row.get(*field).unwrap_or(&Value::Null);
        if matches!(*field, "created_at" | "updated_at") {
            out.push_str(&shift_datetime(value, timezone));
        } else if *field == "archived_at" {
            out.push_str(&shift_datetime_utc(value));
        } else if *field == "sort_order" {
            out.push_str(&render_float(value));
        } else {
            out.push_str(&serde_json::to_string(value).unwrap_or("null".to_owned()));
        }
    }
    out.push('}');
    out
}

/// DRF `JSONEncoder` rendering for an unconverted aware datetime: plain
/// `isoformat` with `+00:00` → `Z` (no zone shift).
fn shift_datetime_utc(value: &Value) -> String {
    let Value::String(text) = value else {
        return serde_json::to_string(value).unwrap_or("null".to_owned());
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => {
            let rendered = crate::serializer::render_datetime_in(&aware, &chrono_tz::UTC);
            serde_json::to_string(&rendered).unwrap_or("null".to_owned())
        }
        Err(_) => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
}

/// Fetch archived annotated rows: the archive queryset (`archive.py:45-256`
/// — same annotations as the main queryset but `archived_at IS NOT NULL`
/// and `member_ids` without the deleted-at guard) over the tenant scope,
/// `-is_favorite, -created_at`, optional module filter.
async fn fetch_archived_rows(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    slug: &str,
    user_id: &uuid::Uuid,
    module_id: Option<&uuid::Uuid>,
) -> Result<Vec<Map<String, Value>>, Denial> {
    use sqlx::Row;
    let mut columns = MODULE_BASE_COLUMNS.join(", ");
    columns.push_str(", ");
    columns.push_str(&annotation_selects(false));
    let mut sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT {columns} \
         FROM modules m JOIN workspaces w ON w.id = m.workspace_id \
         LEFT JOIN module_members ON module_members.module_id = m.id \
         WHERE m.project_id = $1 AND w.slug = $2 AND m.deleted_at IS NULL \
         AND m.archived_at IS NOT NULL"
    );
    if module_id.is_some() {
        sql.push_str(" AND m.id = $4");
    }
    sql.push_str(" GROUP BY m.id ORDER BY ");
    sql.push_str(&queries::MODULE_ORDER_SQL.replace("modules.", "m."));
    sql.push_str(") AS __r");
    let mut query = sqlx::query(&sql).bind(project_id).bind(slug).bind(user_id);
    if let Some(id) = module_id {
        query = query.bind(id);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

/// Shared archive preamble: auth → rewrite → the `ProjectEntityPermission`
/// class gate → zone.
struct ArchiveContext {
    pool: sqlx::PgPool,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
    timezone: chrono_tz::Tz,
}

#[allow(clippy::result_large_err)]
async fn archive_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: &str,
    gate_path: &str,
) -> Result<ArchiveContext, axum::response::Response> {
    let pool = pool_of(state).map_err(|denial| denial.into_response())?;
    let user_id = actor_or_401(extension)?;
    let project_id = resolve_project_or_404(&pool, slug, project_raw).await?;
    let gate = gate_for(method, gate_path);
    let facts = fetch_class_facts(&pool, slug, &project_id, &user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    check_class_gate(gate, method, slug, &facts)?;
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    Ok(ArchiveContext {
        pool,
        project_id,
        user_id,
        timezone,
    })
}

// ---------------------------------------------------------------------------
// Archive / unarchive (`archive.py:544-565`)
// ---------------------------------------------------------------------------

/// `GET modules/<module_id>/archive/`: the ported 500 — `get()` takes no
/// `module_id` kwarg (`archive.py:258`), so dispatch raises `TypeError`
/// after the gate.
async fn archive_get(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    match archive_context(
        &state,
        &slug,
        &project_raw,
        extension,
        "GET",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
    )
    .await
    {
        Ok(_) => {
            // Path-uuid convention (sibling handlers): a non-UUID
            // `module_id` answers the `ValidationError` 400.
            let _ = parse_uuid_or_invalid(&module_raw)?;
            Err(Denial::ServerError)
        }
        Err(response) => Ok(response),
    }
}

/// Fetch one live module row for archive writes (`Module.objects.get` over
/// the tenant scope — archived or not; a miss is the `DoesNotExist` 404).
async fn fetch_archive_module(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<Map<String, Value>, Denial> {
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT row_to_json(__r)::text AS __row FROM (
             SELECT m.id, m.status, m.archived_at
             FROM modules m JOIN workspaces w ON w.id = m.workspace_id
             WHERE m.id = $1 AND m.project_id = $2 AND w.slug = $3
             AND m.deleted_at IS NULL) AS __r"#,
    )
    .bind(module_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Denial::ServerError),
    }
}

/// `str(timezone.now())` for the archive response (`archive.py:559`):
/// `YYYY-MM-DD HH:MM:SS[.ffffff]+00:00` — space separator, microseconds
/// iff nonzero, literal `+00:00` (no `Z` rewrite — `str()` doesn't do that).
fn python_str_utc(stamp: &chrono::DateTime<chrono::Utc>) -> String {
    let base = stamp.format("%Y-%m-%d %H:%M:%S").to_string();
    if stamp.timestamp_subsec_micros() == 0 {
        format!("{base}+00:00")
    } else {
        format!("{}.{:06}+00:00", base, stamp.timestamp_subsec_micros())
    }
}

/// `POST modules/<module_id>/archive/`: completed/cancelled only (else the
/// status 400), stamp `archived_at`, soft-delete the module's favorites
/// across users, answer `{"archived_at": str(...)}`.
async fn archive_post(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = match archive_context(
        &state,
        &slug,
        &project_raw,
        extension,
        "POST",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let ArchiveContext {
        pool,
        project_id,
        user_id,
        ..
    } = context;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let row = fetch_archive_module(&pool, &slug, &project_id, &module_id).await?;
    let status = row.get("status").and_then(|value| value.as_str());
    if !matches!(status, Some("completed" | "cancelled")) {
        return Err(Denial::BadError(
            "Only completed or cancelled modules can be archived".to_owned(),
        ));
    }
    // One stamp stands in for the three `timezone.now()` calls (field,
    // `save()`, favorites cleanup — indistinguishable on the wire); the
    // favorites cleanup is the queryset `delete()` default: soft
    // (`deleted_at` only, no `updated_at` bump), across users.
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE modules SET archived_at = $2, updated_at = $2, updated_by_id = $3
           WHERE id = $1"#,
    )
    .bind(module_id)
    .bind(now)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE user_favorites uf SET deleted_at = $4
           FROM workspaces w
           WHERE uf.workspace_id = w.id AND w.slug = $1 AND uf.project_id = $2
           AND uf.entity_type = 'module' AND uf.entity_identifier = $3
           AND uf.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(module_id)
    .bind(now)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut out = String::from("{\"archived_at\":");
    out.push_str(&super::handlers_modules::json_string(&python_str_utc(&now)));
    out.push('}');
    Ok(json_response(StatusCode::OK, out))
}

/// `DELETE modules/<module_id>/archive/`: clear `archived_at` (no status
/// check), 204 empty.
async fn archive_delete(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = match archive_context(
        &state,
        &slug,
        &project_raw,
        extension,
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let ArchiveContext {
        pool,
        project_id,
        user_id,
        ..
    } = context;
    // `TimezoneMixin` activation (bad zone → 500) already ran inside
    // `archive_context`, before the body — even without rendered datetimes.
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    fetch_archive_module(&pool, &slug, &project_id, &module_id).await?;
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE modules SET archived_at = NULL, updated_at = $2, updated_by_id = $3
           WHERE id = $1"#,
    )
    .bind(module_id)
    .bind(now)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `GET archived-modules/`: archived rows as a bare array.
async fn archived_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = match archive_context(
        &state,
        &slug,
        &project_raw,
        extension,
        "GET",
        "workspaces/<slug>/projects/<project_id>/archived-modules/",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let ArchiveContext {
        pool,
        project_id,
        user_id,
        timezone,
    } = context;
    let rows = fetch_archived_rows(&pool, &project_id, &slug, &user_id, None).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_archived_row(row, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

// ---------------------------------------------------------------------------
// Archived detail (the `else` branch of `get`, `archive.py:294-543`)
// ---------------------------------------------------------------------------

/// Label estimate-distribution over the archive queryset
/// (`archive.py:386-391`): identical to the main label estimate query
/// except the ported bug — no `module_issues.deleted_at IS NULL` filter.
async fn fetch_archive_label_estimate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let total = queries::estimate_sum_sql(None);
    let completed = queries::estimate_sum_sql(Some(true));
    let pending = queries::estimate_sum_sql(Some(false));
    // Same joins, aliases, grouping and ordering as the main label
    // estimate query — only the `module_issues.deleted_at IS NULL`
    // predicate is gone (the `:386-391` ported bug).
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (
           SELECT labels.name AS label_name, labels.color AS color,
                  labels.id AS label_id, {total} AS total_estimates,
                  {completed} AS completed_estimates, {pending} AS pending_estimates
           FROM issues i JOIN projects ON projects.id = i.project_id
           JOIN workspaces ON workspaces.id = i.workspace_id
           LEFT OUTER JOIN states ON states.id = i.state_id
           JOIN module_issues ON module_issues.issue_id = i.id
           LEFT JOIN estimate_points ON estimate_points.id = i.estimate_point_id
           LEFT JOIN issue_labels ON issue_labels.issue_id = i.id
           AND issue_labels.deleted_at IS NULL
           LEFT JOIN labels ON labels.id = issue_labels.label_id
           AND labels.deleted_at IS NULL
           WHERE {} AND workspaces.slug = $1 AND i.project_id = $2
           AND module_issues.module_id = $3
           GROUP BY labels.name, labels.color, labels.id
           ORDER BY labels.name) AS __r",
        queries::issue_manager_guards_sql(),
    );
    fetch_json_maps(pool, &sql, slug, project_id, module_id).await
}

/// `GET archived-modules/<pk>/`: the detail shell plus the
/// estimate/distribution blocks. There is no 404: a miss answers 200 with
/// the `None`-instance shell (`{"member_ids": []}` — `member_ids` is the
/// only writable field and `ListField.initial` is `[]`) plus the blocks
/// the pk still computes (empty for a miss).
async fn archived_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = match archive_context(
        &state,
        &slug,
        &project_raw,
        extension,
        "GET",
        "workspaces/<slug>/projects/<project_id>/archived-modules/<pk>/",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let ArchiveContext {
        pool,
        project_id,
        user_id,
        timezone,
    } = context;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    let rows = fetch_archived_rows(&pool, &project_id, &slug, &user_id, Some(&module_id)).await?;
    let row = rows.into_iter().next();
    let estimate_type = fetch_estimate_type(&pool, &slug, &project_id).await?;
    // The `None`-instance shell renders no links or `sub_issues` (both are
    // read-only, so `get_initial` skips them) — only the row-miss still
    // runs the estimate/distribution queries below.
    let (sub_issues, links) = if row.is_some() {
        (
            fetch_sub_issues(&pool, &project_id, &module_id).await?,
            fetch_module_links(&pool, &module_id).await?,
        )
    } else {
        (0, Vec::new())
    };

    let mut out = String::from("{");
    match row.as_ref() {
        Some(row) => {
            for (index, field) in DETAIL_ROW_ORDER.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(field);
                out.push_str("\":");
                out.push_str(&shape_value(
                    field,
                    row.get(*field).unwrap_or(&Value::Null),
                    &timezone,
                ));
            }
            for field in pidash_services::app_modules::shape::MODULE_DETAIL_EXTRA_FIELDS {
                out.push(',');
                out.push('"');
                out.push_str(field);
                out.push_str("\":");
                match *field {
                    "link_module" => {
                        out.push('[');
                        for (index, link) in links.iter().enumerate() {
                            if index > 0 {
                                out.push(',');
                            }
                            out.push_str(&shape_link(link, &timezone));
                        }
                        out.push(']');
                    }
                    "sub_issues" => out.push_str(&sub_issues.to_string()),
                    _ => out.push_str(&render_float(row.get(*field).unwrap_or(&Value::Null))),
                }
            }
        }
        // The `None`-instance shell (verified live — see the `J9` probe):
        // only the writable field renders.
        None => out.push_str("\"member_ids\":[]"),
    }
    // `estimate_distribution` (`:321-430`): the assignee query keeps its
    // deleted-at filter; the label query drops it (ported bug).
    out.push_str(",\"estimate_distribution\":");
    if estimate_type {
        let assignees =
            fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, true).await?;
        let labels = fetch_archive_label_estimate(&pool, &slug, &project_id, &module_id).await?;
        out.push_str("{\"assignees\":");
        out.push_str(&render_distribution_array(
            ASSIGNEE_ESTIMATE_ORDER,
            &assignees,
        ));
        out.push_str(",\"labels\":");
        out.push_str(&render_distribution_array(
            queries::LABEL_ESTIMATE_ROW_KEYS,
            &labels,
        ));
        if let Some(chart) =
            archived_chart(&pool, &slug, &project_id, &module_id, row.as_ref(), true).await?
        {
            out.push_str(",\"completion_chart\":");
            out.push_str(&chart);
        }
        out.push('}');
    } else {
        out.push_str("{}");
    }
    // `distribution` (`:431-543`): both queries keep their filters; the
    // chart renders on dates alone (no `total_issues > 0` check — the
    // `:533` divergence from `base.py:632`).
    let assignees =
        fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    let labels = fetch_label_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    out.push_str(",\"distribution\":{\"assignees\":");
    out.push_str(&render_distribution_array(ASSIGNEE_COUNT_ORDER, &assignees));
    out.push_str(",\"labels\":");
    out.push_str(&render_distribution_array(
        queries::LABEL_COUNT_ROW_KEYS,
        &labels,
    ));
    out.push_str(",\"completion_chart\":");
    match archived_chart(&pool, &slug, &project_id, &module_id, row.as_ref(), false).await? {
        Some(chart) => out.push_str(&chart),
        None => out.push_str("{}"),
    }
    out.push('}');
    out.push('}');
    Ok(json_response(StatusCode::OK, out))
}

/// One burndown chart for the archived detail: rendered when the row has
/// both dates (`estimate` flavour picks points vs counts); `None` without
/// a row or without parseable dates.
async fn archived_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    row: Option<&Map<String, Value>>,
    estimate: bool,
) -> Result<Option<String>, Denial> {
    let Some(row) = row else {
        return Ok(None);
    };
    let has_dates = row.get("start_date").is_some_and(|v| !v.is_null())
        && row.get("target_date").is_some_and(|v| !v.is_null());
    if !has_dates {
        return Ok(None);
    }
    let start = row
        .get("start_date")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let target = row
        .get("target_date")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let (Some(start), Some(target)) = (start, target) else {
        return Ok(None);
    };
    let total_issues = row
        .get("total_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let chart = fetch_burndown_chart(
        pool,
        slug,
        project_id,
        module_id,
        start,
        target,
        total_issues,
        estimate,
    )
    .await?;
    Ok(Some(chart))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archived_row_order_matches_values_call() {
        // Concrete fields in call order, then requested annotations in
        // queryset annotation order (verified live: J7).
        assert_eq!(
            ARCHIVED_ROW_ORDER,
            &[
                "id",
                "workspace_id",
                "project_id",
                "name",
                "description",
                "description_text",
                "description_html",
                "start_date",
                "target_date",
                "status",
                "lead_id",
                "view_props",
                "sort_order",
                "external_source",
                "external_id",
                "created_at",
                "updated_at",
                "archived_at",
                "is_favorite",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "total_issues",
                "member_ids",
            ]
        );
    }

    #[test]
    fn python_str_utc_matches_str_datetime() {
        use chrono::TimeZone;
        let with_micros = chrono::Utc
            .with_ymd_and_hms(2026, 10, 1, 1, 36, 54)
            .unwrap()
            .checked_add_signed(chrono::Duration::microseconds(631680))
            .unwrap();
        assert_eq!(
            python_str_utc(&with_micros),
            "2026-10-01 01:36:54.631680+00:00"
        );
        let whole = chrono::Utc
            .with_ymd_and_hms(2026, 10, 1, 1, 36, 54)
            .unwrap();
        assert_eq!(python_str_utc(&whole), "2026-10-01 01:36:54+00:00");
    }

    #[test]
    fn archived_at_renders_utc_while_siblings_shift() {
        let ny = &chrono_tz::America::New_York;
        let mut row = Map::new();
        row.insert("id".to_owned(), Value::String("i".to_owned()));
        for field in [
            "workspace_id",
            "project_id",
            "name",
            "description",
            "description_text",
            "description_html",
            "start_date",
            "target_date",
            "status",
            "lead_id",
            "view_props",
            "external_source",
            "external_id",
            "is_favorite",
            "completed_issues",
            "cancelled_issues",
            "started_issues",
            "unstarted_issues",
            "backlog_issues",
            "total_issues",
            "member_ids",
        ] {
            row.insert(field.to_owned(), Value::Null);
        }
        row.insert(
            "sort_order".to_owned(),
            Value::Number(serde_json::Number::from(65535)),
        );
        for field in ["created_at", "updated_at", "archived_at"] {
            row.insert(
                field.to_owned(),
                Value::String("2026-10-01T01:36:54.881242Z".to_owned()),
            );
        }
        let rendered = shape_archived_row(&row, ny);
        assert!(
            rendered.contains(r#""created_at":"2026-09-30T21:36:54.881242-04:00""#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""updated_at":"2026-09-30T21:36:54.881242-04:00""#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""archived_at":"2026-10-01T01:36:54.881242Z""#),
            "{rendered}"
        );
        assert!(rendered.contains(r#""sort_order":65535.0"#), "{rendered}");
    }
}
