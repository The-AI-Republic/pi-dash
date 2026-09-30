//! Intake work-item description versions (D-32, stage 5):
//! `IntakeWorkItemDescriptionVersionEndpoint.get`
//! (`app/views/intake/base.py:569-637`) over
//! `intake-work-items/<work_item_id>/description-versions[/<pk>/]`.
//!
//! The work-item id is the **issue** id. The flow: `Project` lookup,
//! `Issue` lookup, GUEST gate (403 for guests without view-all reading
//! someone else's issue), then the single-`pk` path (the full
//! `IssueDescriptionVersionDetailSerializer`, 200) or the paginated
//! list (the 10-key `required_fields` projection with
//! `user_timezone_converter` over `created_at`/`updated_at`, 200).

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_services::app_intake::permissions as guards;

use super::{
    actor, guard_denial, guest_view_all, load_membership, parse_id, pool_of, query_last,
    raw_json_response, resolve_tenant, Denial, QueryMap,
};
use crate::serializer::render_datetime_in;
use crate::state::AppState;

/// One `issue_description_versions` row.
struct VersionRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    issue_id: Uuid,
    description_binary: Option<Vec<u8>>,
    description_html: Option<String>,
    description_stripped: Option<String>,
    description_json: serde_json::Value,
    last_saved_at: Option<DateTime<Utc>>,
    owned_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

impl VersionRow {
    fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            issue_id: row.try_get("issue_id")?,
            description_binary: row.try_get("description_binary")?,
            description_html: row.try_get("description_html")?,
            description_stripped: row.try_get("description_stripped")?,
            description_json: row.try_get("description_json")?,
            last_saved_at: row.try_get("last_saved_at")?,
            owned_by_id: row.try_get("owned_by_id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
        })
    }
}

fn opt_uuid(value: &Option<Uuid>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |id| {
        serde_json::Value::String(id.to_string())
    })
}

/// `IssueDescriptionVersionDetailSerializer`
/// (`app/serializers/issue.py:1487+`): the 14 fields in declaration
/// order. `description_binary` has no DRF JSON mapping — a set value
/// fails rendering in Django too (500); null renders null.
fn render_version_detail(
    row: &VersionRow,
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, Denial> {
    if row.description_binary.is_some() {
        return Err(Denial::ServerError);
    }
    let mut map = serde_json::Map::with_capacity(14);
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(row.id.to_string()),
    );
    map.insert(
        "workspace".to_owned(),
        serde_json::Value::String(row.workspace_id.to_string()),
    );
    map.insert(
        "project".to_owned(),
        serde_json::Value::String(row.project_id.to_string()),
    );
    map.insert(
        "issue".to_owned(),
        serde_json::Value::String(row.issue_id.to_string()),
    );
    map.insert("description_binary".to_owned(), serde_json::Value::Null);
    map.insert(
        "description_html".to_owned(),
        row.description_html
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    map.insert(
        "description_stripped".to_owned(),
        row.description_stripped
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    map.insert("description_json".to_owned(), row.description_json.clone());
    map.insert(
        "last_saved_at".to_owned(),
        row.last_saved_at.map_or(serde_json::Value::Null, |dt| {
            serde_json::Value::String(render_datetime_in(&dt, &timezone))
        }),
    );
    map.insert("owned_by".to_owned(), opt_uuid(&row.owned_by_id));
    map.insert(
        "created_at".to_owned(),
        serde_json::Value::String(render_datetime_in(&row.created_at, &timezone)),
    );
    map.insert(
        "updated_at".to_owned(),
        serde_json::Value::String(render_datetime_in(&row.updated_at, &timezone)),
    );
    map.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    Ok(serde_json::Value::Object(map))
}

/// Shared head of both versions paths: auth, tenant, the `Project`
/// and `Issue` lookups, the decorator (`ADMIN + MEMBER + GUEST`, no
/// creator bypass) and the guest creator check against the parent
/// `Issue.created_by`.
struct VersionsContext {
    pool: PgPool,
    slug: String,
    tenant_project: Uuid,
    issue_id: Uuid,
    timezone: chrono_tz::Tz,
}

// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn versions_context(
    state: &AppState,
    slug: String,
    project_id_raw: String,
    work_item_id_raw: String,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<VersionsContext, Response> {
    let actor = match actor(state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return Err(denial.into_response()),
    };
    let pool = match pool_of(state) {
        Ok(pool) => pool.clone(),
        Err(denial) => return Err(denial.into_response()),
    };
    let tenant = match resolve_tenant(&pool, &slug, &project_id_raw).await {
        Ok(tenant) => tenant,
        Err(denial) => return Err(denial.into_response()),
    };
    let work_item_id = match parse_id(&work_item_id_raw) {
        Ok(id) => id,
        Err(response) => return Err(response),
    };
    // `Project.objects.get(pk=project_id)` — a miss is 404.
    let view_all = match guest_view_all(&pool, &tenant.project_id).await {
        Ok(view_all) => view_all,
        Err(denial) => return Err(denial.into_response()),
    };
    // `Issue.objects.get(workspace__slug, project_id, pk=work_item_id)`
    // — a miss is 404.
    let issue: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT i.id, i.created_by_id FROM issues i
           INNER JOIN workspaces w ON w.id = i.workspace_id
           WHERE w.slug = $1 AND i.project_id = $2 AND i.id = $3 AND i.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(tenant.project_id)
    .bind(work_item_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    let Some((issue_id, issue_creator)) = issue else {
        return Err(Denial::NotFound.into_response());
    };
    // Decorator (`:578`): ADMIN + MEMBER + GUEST, no creator bypass.
    let membership = match load_membership(&pool, &slug, &tenant.project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return Err(denial.into_response()),
    };
    let gate = guards::versions_gate(&membership.as_guard());
    if gate.is_err() {
        return Err(guard_denial(gate));
    }
    // Guest creator check (`:583-597`): the parent `Issue.created_by`
    // decides (unlike retrieve, which tests the bridge row).
    let gate = guards::guest_view_gate(
        &membership.as_guard(),
        view_all,
        issue_creator == Some(actor.id),
    );
    if gate.is_err() {
        return Err(guard_denial(gate));
    }
    Ok(VersionsContext {
        pool,
        slug,
        tenant_project: tenant.project_id,
        issue_id,
        timezone: actor.timezone,
    })
}

/// Single-version path (`:599-608`).
pub async fn detail(
    State(state): State<AppState>,
    Path((slug, project_id, work_item_id, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let context = match versions_context(&state, slug, project_id, work_item_id, extension).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let pk = match parse_id(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let found: Option<sqlx::postgres::PgRow> = match sqlx::query(
        r#"SELECT v.* FROM issue_description_versions v
           INNER JOIN workspaces w ON w.id = v.workspace_id
           WHERE w.slug = $1 AND v.project_id = $2 AND v.issue_id = $3 AND v.id = $4
             AND v.deleted_at IS NULL"#,
    )
    .bind(&context.slug)
    .bind(context.tenant_project)
    .bind(context.issue_id)
    .bind(pk)
    .fetch_optional(&context.pool)
    .await
    {
        Ok(found) => found,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(found) = found else {
        return Denial::NotFound.into_response();
    };
    let version = match VersionRow::get(&found) {
        Ok(version) => version,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match render_version_detail(&version, context.timezone) {
        Ok(body) => raw_json_response(body.to_string()),
        Err(denial) => denial.into_response(),
    }
}

/// One decodes list-projection row: the 10 `required_fields` columns.
type VersionListRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<Uuid>,
    Option<Uuid>,
);

fn try_decode(row: &sqlx::postgres::PgRow) -> Result<VersionListRow, sqlx::Error> {
    use sqlx::Row;
    Ok((
        row.try_get("id")?,
        row.try_get("workspace_id")?,
        row.try_get("project_id")?,
        row.try_get("issue_id")?,
        row.try_get("last_saved_at")?,
        row.try_get("owned_by_id")?,
        row.try_get("created_at")?,
        row.try_get("updated_at")?,
        row.try_get("created_by_id")?,
        row.try_get("updated_by_id")?,
    ))
}

/// Paginated list path (`:610-637`): the 10-key `required_fields`
/// projection, `user_timezone_converter` over `created_at`/`updated_at`
/// into the requester's zone, and the global cursor envelope.
pub async fn list(
    State(state): State<AppState>,
    Path((slug, project_id, work_item_id)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let context =
        match versions_context(&state, slug.clone(), project_id, work_item_id, extension).await {
            Ok(context) => context,
            Err(response) => return response,
        };
    // Cursor (`global_paginator.paginate`): `None` reads `1000:0:0`;
    // garbage is a `ValueError` → 500. The third component parses but
    // is never read (`offset` is dead on the paginator too).
    let (mut page_size, page) = match query_last(&query, "cursor") {
        None => (1000i64, 0i64),
        Some(raw) => {
            let bits: Vec<&str> = raw.split(':').collect();
            if bits.len() != 3 {
                return Denial::ServerError.into_response();
            }
            let (Some(size), Some(page), Some(_offset)) = (
                bits[0].parse::<i64>().ok(),
                bits[1].parse::<i64>().ok(),
                bits[2].parse::<i64>().ok(),
            ) else {
                return Denial::ServerError.into_response();
            };
            (size, page)
        }
    };
    page_size = page_size.min(1000);
    // `page_size <= 0` divides by zero (`ceil(total / 0)`); a negative
    // page negative-indexes the slice — both are 500s in Django.
    if page_size <= 0 || page < 0 {
        return Denial::ServerError.into_response();
    }
    let total: i64 = match sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM issue_description_versions v
           INNER JOIN workspaces w ON w.id = v.workspace_id
           WHERE w.slug = $1 AND v.project_id = $2 AND v.issue_id = $3
             AND v.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(context.tenant_project)
    .bind(context.issue_id)
    .fetch_one(&context.pool)
    .await
    {
        Ok(total) => total,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let total_pages = (total + page_size - 1) / page_size;
    let start: i64 = page * page_size;
    let end: i64 = (start + page_size).min(total);
    let rows: Vec<sqlx::postgres::PgRow> = match sqlx::query(
        r#"SELECT v.id, v.workspace_id, v.project_id, v.issue_id, v.last_saved_at,
                  v.owned_by_id, v.created_at, v.updated_at, v.created_by_id, v.updated_by_id
           FROM issue_description_versions v
           INNER JOIN workspaces w ON w.id = v.workspace_id
           WHERE w.slug = $1 AND v.project_id = $2 AND v.issue_id = $3
             AND v.deleted_at IS NULL
           LIMIT $4 OFFSET $5"#,
    )
    .bind(&slug)
    .bind(context.tenant_project)
    .bind(context.issue_id)
    .bind(page_size)
    .bind(start)
    .fetch_all(&context.pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        let decoded: Result<VersionListRow, sqlx::Error> = try_decode(row);
        let (
            id,
            workspace_id,
            project_id,
            issue_id,
            last_saved_at,
            owned_by_id,
            created_at,
            updated_at,
            created_by_id,
            updated_by_id,
        ) = match decoded {
            Ok(decoded) => decoded,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let mut item = serde_json::Map::with_capacity(10);
        item.insert("id".to_owned(), serde_json::Value::String(id.to_string()));
        item.insert(
            "workspace".to_owned(),
            serde_json::Value::String(workspace_id.to_string()),
        );
        item.insert(
            "project".to_owned(),
            serde_json::Value::String(project_id.to_string()),
        );
        item.insert(
            "issue".to_owned(),
            serde_json::Value::String(issue_id.to_string()),
        );
        // `last_saved_at` is outside the converter's field set: it
        // renders in the requester's zone through the active timezone,
        // like every DRF datetime under `TimezoneMixin`.
        item.insert(
            "last_saved_at".to_owned(),
            last_saved_at.map_or(serde_json::Value::Null, |dt| {
                serde_json::Value::String(render_datetime_in(&dt, &context.timezone))
            }),
        );
        item.insert("owned_by".to_owned(), opt_uuid(&owned_by_id));
        // `user_timezone_converter` (`:570-576`): the projection's two
        // datetime fields shift into the requester's zone.
        item.insert(
            "created_at".to_owned(),
            serde_json::Value::String(render_datetime_in(&created_at, &context.timezone)),
        );
        item.insert(
            "updated_at".to_owned(),
            serde_json::Value::String(render_datetime_in(&updated_at, &context.timezone)),
        );
        item.insert("created_by".to_owned(), opt_uuid(&created_by_id));
        item.insert("updated_by".to_owned(), opt_uuid(&updated_by_id));
        results.push(serde_json::Value::Object(item));
    }
    let next_cursor: Option<String> = if end < total {
        Some(format!("{page_size}:{}:0", page + 1))
    } else {
        None
    };
    let envelope = serde_json::json!({
        "prev_cursor": format!("{page_size}:{}:0", page - 1),
        "cursor": format!("{page_size}:{page}:0"),
        "next_cursor": next_cursor,
        "prev_page_results": page > 0,
        "next_page_results": next_cursor.is_some(),
        "page_count": results.len(),
        "total_results": total,
        "total_pages": total_pages,
        "results": results,
    });
    raw_json_response(envelope.to_string())
}
