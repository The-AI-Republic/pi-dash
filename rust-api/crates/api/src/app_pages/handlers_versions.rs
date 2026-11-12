//! Page versions + duplicate handlers (D-30, stage 5, PIDASHCONV-338).
//!
//! Ports three endpoints (routes in `apps/api/pi_dash/app/urls/page.py`):
//!
//! * `GET .../pages/<page_id>/versions/` (`PageVersionEndpoint.get`,
//!   collection branch, `version.py:27-31`): workspace+page filtered
//!   `PageVersionSerializer` rows (`many=True`), 200.
//! * `GET .../pages/<page_id>/versions/<pk>/` (pk branch, `version.py:21-26`):
//!   `+pk` get, `PageVersionDetailSerializer`, 200; a miss bubbles
//!   `DoesNotExist` to `handle_exception`'s 404.
//! * `POST .../pages/<page_id>/duplicate/` (`PageDuplicateEndpoint.post`,
//!   `base.py:578-639`): the project-scoped fetch, the private-owner 403,
//!   the clone (rename, binary reset, requester ownership), one
//!   `ProjectPage` row per live source project, the `page_transaction` +
//!   `copy_s3_objects` publishes, and the re-fetch with the `project_ids`
//!   annotation under `PageDetailSerializer`, 201.
//!
//! Fixtures: `serializers/page_version_shapes.golden.json` (F30-03, both
//! version shapes), `guards/permissions.golden.json` (F30-09, class matrix),
//! `tasks/publish.golden.json` (F30-10, both duplicate publishes),
//! `handlers/io.golden.json` (F30-11, duplicate I/O), `handlers/versions.golden.json`
//! (F30-12, pk vs collection branches). Trace:
//! `rust-api/fixtures/app_pages/TRACE.md`.
//!
//! Layering: the class half of auth is [`super::gate`] (`resolve_gate`);
//! the duplicate private-owner check is `gate::check_duplicate_private`;
//! datetimes render through `crate::serializer::render_datetime_in` in the
//! requester's zone (`TimezoneMixin`); the copy's `description_stripped`
//! recompute is `pidash_db::app_pages::strip::sync_description_stripped`;
//! publishes use the merged `pidash_jobs::app_pages` constructors and
//! enqueue best-effort post-commit like the description handlers.
//!
//! Gate order (preserved): URL resolution (`<uuid:page_id>`/`<uuid:pk>`
//! mismatch proxies to Django's resolver 404), `BaseViewSet.initial`
//! project rewrite (authenticated only) and session auth, then the class
//! gate (`ProjectPagePermission`), then the view body with its inline
//! guard. Anonymous callers 401 before any gate.
//!
//! Ported bugs and quirks (translate, don't redesign — also in the PR):
//!
//! * QUIRK-merged-fetch (`base.py:582-587`): Django merges the
//!   `projects__id` + `project_pages__deleted_at` spans into ONE join and
//!   trims the `projects` table entirely, so the fetch needs a LIVE link to
//!   the URL project — a soft-deleted link 404s (verified vs live Django).
//! * QUIRK-clone-keeps-audit (`base.py:596-602`): `pk=None` on the loaded
//!   instance keeps `_state.adding=False`, so crum's non-adding branch sets
//!   `updated_by=request.user` — the copy's `created_by` AND `updated_by`
//!   are both the requester (verified; the "wipe" reading is wrong).
//! * QUIRK-bridge-null-updated (`base.py:604-611`): each `ProjectPage` row
//!   is a fresh instance (`adding=True`), so crum overwrites the passed
//!   `updated_by_id` with NULL — bridges store `updated_by=NULL` (verified).
//! * QUIRK-shadow-project (`base.py:604-623`): the loop variable shadows
//!   the URL kwarg, so `copy_s3` receives the LAST id of the `-created_at`
//!   link list (i.e. the OLDEST live link); with no links the rewritten URL
//!   kwarg survives (unreachable — the fetch needs a live link).
//! * QUIRK-uuid-one-filter (`base.py:630-634`): `~Q(projects__id=True)`
//!   adapts `True` to UUID `00000000-...-000000000001`, an effective no-op;
//!   the LEFT JOIN carries no deleted filter, so the annotation INCLUDES
//!   soft-deleted links (verified).
//! * QUIRK-duplicate-shape (`base.py:628-639`): the re-fetch annotates ONLY
//!   `project_ids`, so `is_favorite`/`label_ids`/`labels` `SkipField` out —
//!   the 201 body has 17 keys, never those three (verified).
//! * QUIRK-binary-base64 (`fields/__init__.py` `BinaryField.value_to_string`):
//!   non-NULL version bytes render base64 (infallible — even invalid UTF-8),
//!   NULL renders null (verified).
//!
//! Task delivery: `serve` carries no AMQP publisher (only the worker does),
//! so `.delay()` calls enqueue a [`pidash_jobs::queue::NewJob`] into
//! `rust_job_queue`; the worker forwards Python-owned names to the broker.
//! Enqueue is best-effort after commit: a missing queue table must not turn
//! user-visible writes into 500s, so failures are traced and the response
//! stands.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gate;
use super::{actor_user_id, enqueue_best_effort, json_response, pool_of, Denial};

use pidash_db::entity::{page_versions, pages, project_pages, users, workspaces};
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Select, Set,
};

/// SeaORM connection over the request's sqlx pool (no I/O).
fn orm(pool: &PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

// ---------------------------------------------------------------------------
// Queries (factored so the unit tests pin the ported quirks statically)
// ---------------------------------------------------------------------------

/// Duplicate source fetch (`base.py:582-587`): Django merges the two spans
/// into ONE `project_pages` join and trims the `projects` table entirely
/// (QUIRK-merged-fetch) — a live link to the URL project, a live page, the
/// URL workspace. Base `Page.objects` carries the soft-delete manager.
/// The link's `project_id`/`deleted_at` predicates ride as `WHERE`
/// filters, which for an inner join is the same statement as the `ON`
/// qualifiers (`pp.page_id = p.id` + `pp.project_id = ?` + `pp.deleted_at
/// IS NULL`).
fn source_fetch_query(page_id: &Uuid, project_id: &Uuid, slug: &str) -> Select<pages::Entity> {
    pages::Entity::find()
        .select_only()
        .column(pages::Column::WorkspaceId)
        .column(pages::Column::Name)
        .column(pages::Column::DescriptionJson)
        .column(pages::Column::DescriptionHtml)
        .column(pages::Column::OwnedById)
        .column(pages::Column::Access)
        .column(pages::Column::Color)
        .column(pages::Column::ParentId)
        .column(pages::Column::ArchivedAt)
        .column(pages::Column::IsLocked)
        .column(pages::Column::ViewProps)
        .column(pages::Column::LogoProps)
        .column(pages::Column::IsGlobal)
        .column(pages::Column::MovedToPage)
        .column(pages::Column::MovedToProject)
        .column(pages::Column::SortOrder)
        .column(pages::Column::ExternalId)
        .column(pages::Column::ExternalSource)
        .join(JoinType::InnerJoin, pages::Relation::Workspaces.def())
        .join(JoinType::InnerJoin, pages::Relation::ProjectPages.def())
        .filter(pages::Column::Id.eq(*page_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_pages::Column::ProjectId.eq(*project_id))
        .filter(project_pages::Column::DeletedAt.is_null())
        .filter(pages::Column::DeletedAt.is_null())
}

/// Duplicate re-link list (`base.py:594`): the default (live-only) manager
/// in `Meta.ordering` (`-created_at`). The loop shadows `project_id`, so
/// the LAST row wins (QUIRK-shadow-project).
fn duplicate_links_query(page_id: &Uuid) -> Select<project_pages::Entity> {
    project_pages::Entity::find()
        .select_only()
        .column(project_pages::Column::ProjectId)
        .filter(project_pages::Column::PageId.eq(*page_id))
        .filter(project_pages::Column::DeletedAt.is_null())
        .order_by_desc(project_pages::Column::CreatedAt)
}

/// Duplicate re-fetch (`base.py:628-637`): the row plus the
/// `Coalesce-ArrayAgg` annotation. `True` adapts to UUID `...0001`
/// (QUIRK-uuid-one-filter, an effective no-op) and the LEFT JOIN has no
/// deleted filter, so soft-deleted links ARE included. `.first()`.
fn duplicate_refetch_sql() -> &'static str {
    r#"SELECT p.id, p.name, p.owned_by_id, p.access, p.color, p.parent_id,
       p.is_locked, p.archived_at, p.workspace_id, p.created_at, p.updated_at,
       p.created_by_id, p.updated_by_id, p.view_props, p.logo_props,
       p.description_html, p.description_json,
       COALESCE((SELECT ARRAY_AGG(DISTINCT pp.project_id) FILTER (
                   WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001'
                       AND pp.project_id IS NOT NULL))
                 FROM project_pages pp WHERE pp.page_id = p.id),
                '{}'::uuid[]) AS project_ids
       FROM pages p WHERE p.id = $1 AND p.deleted_at IS NULL LIMIT 1"#
}

/// Version rows (`version.py:23,28`): workspace slug + page, live only (the
/// default manager), `Meta.ordering` (`-created_at`).
fn versions_list_query(slug: &str, page_id: &Uuid) -> Select<page_versions::Entity> {
    page_versions::Entity::find()
        .select_only()
        .column(page_versions::Column::Id)
        .column(page_versions::Column::WorkspaceId)
        .column(page_versions::Column::PageId)
        .column(page_versions::Column::LastSavedAt)
        .column(page_versions::Column::OwnedById)
        .column(page_versions::Column::CreatedAt)
        .column(page_versions::Column::UpdatedAt)
        .column(page_versions::Column::CreatedById)
        .column(page_versions::Column::UpdatedById)
        .join(
            JoinType::InnerJoin,
            page_versions::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(page_versions::Column::PageId.eq(*page_id))
        .filter(page_versions::Column::DeletedAt.is_null())
        .order_by_desc(page_versions::Column::CreatedAt)
}

/// See [`versions_list_query`] with the detail columns plus `pv.id = pk`.
fn version_detail_query(slug: &str, page_id: &Uuid, pk: &Uuid) -> Select<page_versions::Entity> {
    page_versions::Entity::find()
        .select_only()
        .column(page_versions::Column::Id)
        .column(page_versions::Column::WorkspaceId)
        .column(page_versions::Column::PageId)
        .column(page_versions::Column::LastSavedAt)
        .column(page_versions::Column::DescriptionBinary)
        .column(page_versions::Column::DescriptionHtml)
        .column(page_versions::Column::DescriptionJson)
        .column(page_versions::Column::OwnedById)
        .column(page_versions::Column::CreatedAt)
        .column(page_versions::Column::UpdatedAt)
        .column(page_versions::Column::CreatedById)
        .column(page_versions::Column::UpdatedById)
        .join(
            JoinType::InnerJoin,
            page_versions::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(page_versions::Column::PageId.eq(*page_id))
        .filter(page_versions::Column::Id.eq(*pk))
        .filter(page_versions::Column::DeletedAt.is_null())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `page_versions` list row ([`versions_list_query`]), decoded by name.
#[derive(Debug, sea_orm::FromQueryResult)]
struct VersionRow {
    id: Uuid,
    workspace_id: Uuid,
    page_id: Uuid,
    last_saved_at: DateTimeWithTimeZone,
    owned_by_id: Uuid,
    created_at: DateTimeWithTimeZone,
    updated_at: DateTimeWithTimeZone,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

/// One `page_versions` detail row ([`version_detail_query`]), decoded by name.
#[derive(Debug, sea_orm::FromQueryResult)]
struct VersionDetailRow {
    id: Uuid,
    workspace_id: Uuid,
    page_id: Uuid,
    last_saved_at: DateTimeWithTimeZone,
    description_binary: Option<Vec<u8>>,
    description_html: String,
    description_json: Value,
    owned_by_id: Uuid,
    created_at: DateTimeWithTimeZone,
    updated_at: DateTimeWithTimeZone,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

/// The duplicate source page ([`source_fetch_query`]), decoded by name.
#[derive(Debug, sea_orm::FromQueryResult)]
struct SourcePage {
    workspace_id: Uuid,
    name: String,
    description_json: Value,
    description_html: String,
    owned_by_id: Uuid,
    access: i16,
    color: String,
    parent_id: Option<Uuid>,
    archived_at: Option<NaiveDate>,
    is_locked: bool,
    view_props: Value,
    logo_props: Value,
    is_global: bool,
    moved_to_page: Option<Uuid>,
    moved_to_project: Option<Uuid>,
    sort_order: f64,
    external_id: Option<String>,
    external_source: Option<String>,
}

/// The duplicate re-fetch ([`duplicate_refetch_sql`]).
#[derive(Debug, sqlx::FromRow)]
struct CopyRow {
    id: Uuid,
    name: String,
    owned_by_id: Uuid,
    access: i16,
    color: String,
    parent_id: Option<Uuid>,
    is_locked: bool,
    archived_at: Option<NaiveDate>,
    workspace_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    view_props: Value,
    logo_props: Value,
    description_html: String,
    description_json: Value,
    project_ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Response bodies (field order = serializer `Meta.fields` order)
// ---------------------------------------------------------------------------

/// `PageVersionSerializer` row (`page.py:139-149`).
#[derive(Debug, Serialize)]
struct VersionRowBody {
    id: Uuid,
    workspace: Uuid,
    page: Uuid,
    last_saved_at: String,
    owned_by: Uuid,
    created_at: String,
    updated_at: String,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

/// `PageVersionDetailSerializer` row (`page.py:156-169`).
#[derive(Debug, Serialize)]
struct VersionDetailBody {
    id: Uuid,
    workspace: Uuid,
    page: Uuid,
    last_saved_at: String,
    description_binary: Option<String>,
    description_html: String,
    description_json: Value,
    owned_by: Uuid,
    created_at: String,
    updated_at: String,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

/// `PageDetailSerializer` over the duplicate re-fetch (`base.py:638-639`):
/// the 19 observed keys — `is_favorite`/`label_ids`/`sub_pages_count`/
/// `labels` `SkipField` out because the re-fetch annotates only
/// `project_ids` (QUIRK-duplicate-shape); `parent_id` is a model attribute.
/// `description_json` trails `description_html` (NEWFRONT-932, main
/// `page.py:138,141`).
#[derive(Debug, Serialize)]
struct DuplicateBody {
    id: Uuid,
    name: String,
    owned_by: Uuid,
    access: i16,
    color: String,
    parent: Option<Uuid>,
    parent_id: Option<Uuid>,
    is_locked: bool,
    archived_at: Option<NaiveDate>,
    workspace: Uuid,
    created_at: String,
    updated_at: String,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    view_props: Value,
    logo_props: Value,
    project_ids: Vec<Uuid>,
    description_html: String,
    description_json: Value,
}

fn render_row(row: &VersionRow, tz: &Tz) -> VersionRowBody {
    let render = crate::serializer::render_datetime_in;
    VersionRowBody {
        id: row.id,
        workspace: row.workspace_id,
        page: row.page_id,
        last_saved_at: render(&row.last_saved_at.with_timezone(&Utc), tz),
        owned_by: row.owned_by_id,
        created_at: render(&row.created_at.with_timezone(&Utc), tz),
        updated_at: render(&row.updated_at.with_timezone(&Utc), tz),
        created_by: row.created_by_id,
        updated_by: row.updated_by_id,
    }
}

fn render_detail(row: &VersionDetailRow, tz: &Tz) -> VersionDetailBody {
    let render = crate::serializer::render_datetime_in;
    VersionDetailBody {
        id: row.id,
        workspace: row.workspace_id,
        page: row.page_id,
        last_saved_at: render(&row.last_saved_at.with_timezone(&Utc), tz),
        description_binary: row.description_binary.as_deref().map(render_binary),
        description_html: row.description_html.clone(),
        description_json: row.description_json.clone(),
        owned_by: row.owned_by_id,
        created_at: render(&row.created_at.with_timezone(&Utc), tz),
        updated_at: render(&row.updated_at.with_timezone(&Utc), tz),
        created_by: row.created_by_id,
        updated_by: row.updated_by_id,
    }
}

fn render_copy(row: &CopyRow, tz: &Tz) -> DuplicateBody {
    let render = crate::serializer::render_datetime_in;
    DuplicateBody {
        id: row.id,
        name: row.name.clone(),
        owned_by: row.owned_by_id,
        access: row.access,
        color: row.color.clone(),
        parent: row.parent_id,
        parent_id: row.parent_id,
        is_locked: row.is_locked,
        archived_at: row.archived_at,
        workspace: row.workspace_id,
        created_at: render(&row.created_at, tz),
        updated_at: render(&row.updated_at, tz),
        created_by: row.created_by_id,
        updated_by: row.updated_by_id,
        view_props: row.view_props.clone(),
        logo_props: row.logo_props.clone(),
        project_ids: row.project_ids.clone(),
        description_html: row.description_html.clone(),
        description_json: row.description_json.clone(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `BinaryField.value_to_string` (`fields/__init__.py`): binary data is
/// serialized as standard base64 (QUIRK-binary-base64).
pub fn render_binary(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The `copy_s3` `project_id` (`base.py:604-623`): the loop variable shadows
/// the URL kwarg, so the LAST id of the `-created_at` link list wins; with
/// no links the (rewritten) URL kwarg survives (QUIRK-shadow-project).
pub fn shadowed_project_id(links: &[Uuid], url_project_id: &Uuid) -> Uuid {
    links.last().copied().unwrap_or(*url_project_id)
}

/// `user_timezone` decoded by name; `None` is a NULL column (or a missing
/// row — sessions FK to users, so the miss is unreachable).
#[derive(Debug, sea_orm::FromQueryResult)]
struct TimezoneRow {
    user_timezone: Option<String>,
}

/// Activate the actor's rendering zone (`TimezoneMixin.initial` runs inside
/// `initial()`, before the view body — an unknown zone 500s before any
/// write). A missing row is unreachable (sessions FK to users) and falls
/// back to UTC; an unknown zone name 500s (`ZoneInfo(...)` raises).
async fn request_timezone(pool: &PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let row: Option<TimezoneRow> = users::Entity::find()
        .select_only()
        .column(users::Column::UserTimezone)
        .filter(users::Column::Id.eq(*user_id))
        .into_model()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.and_then(|row| row.user_timezone)
        .as_deref()
        .unwrap_or("UTC")
        .parse()
        .map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

/// `GET .../pages/<page_id>/versions/` (`PageVersionEndpoint.get`,
/// collection branch, `version.py:27-31`).
pub async fn versions_list(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = super::parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    versions_response(state, &slug, &project_raw, &page_id, None, extension).await
}

/// `GET .../pages/<page_id>/versions/<pk>/` (`PageVersionEndpoint.get`, pk
/// branch, `version.py:21-26`).
pub async fn version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let (Ok(page_id), Ok(pk)) = (
        super::parse_page_id(&page_raw),
        super::parse_page_id(&pk_raw),
    ) else {
        return crate::edge::proxy(State(state), req).await;
    };
    versions_response(state, &slug, &project_raw, &page_id, Some(pk), extension).await
}

async fn versions_response(
    state: AppState,
    slug: &str,
    project_raw: &str,
    page_id: &Uuid,
    pk: Option<Uuid>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // The rewrite runs for authenticated requests only (anonymous callers
    // 401 inside the gate): the project value is unused on the anonymous
    // path, so a raw fallback stands in.
    let project_id = if actor_user_id(extension.clone()).is_ok() {
        match super::resolve_project_id(pool, slug, project_raw).await {
            Ok(id) => id,
            Err(denial) => return denial.into_response(),
        }
    } else {
        project_raw.parse::<Uuid>().unwrap_or_default()
    };
    let gate = match gate::resolve_gate(&state, "GET", slug, &project_id, Some(*page_id), extension)
        .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let tz = match request_timezone(pool, &gate.user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    if let Some(pk) = pk {
        let row: Option<VersionDetailRow> = match version_detail_query(slug, page_id, &pk)
            .into_model()
            .one(&orm(pool))
            .await
        {
            Ok(row) => row,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let Some(row) = row else {
            return Denial::ObjectNotFound.into_response();
        };
        let body = serde_json::to_string(&render_detail(&row, &tz)).expect("serializable row");
        return json_response(StatusCode::OK, body);
    }
    let rows: Vec<VersionRow> = match versions_list_query(slug, page_id)
        .into_model()
        .all(&orm(pool))
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let bodies: Vec<VersionRowBody> = rows.iter().map(|row| render_row(row, &tz)).collect();
    let body = serde_json::to_string(&bodies).expect("serializable rows");
    json_response(StatusCode::OK, body)
}

// ---------------------------------------------------------------------------
// Duplicate
// ---------------------------------------------------------------------------

/// `POST .../pages/<page_id>/duplicate/` (`PageDuplicateEndpoint.post`,
/// `base.py:581-639`). The request body is never read.
pub async fn duplicate(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = super::parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = if actor_user_id(extension.clone()).is_ok() {
        match super::resolve_project_id(pool, &slug, &project_raw).await {
            Ok(id) => id,
            Err(denial) => return denial.into_response(),
        }
    } else {
        project_raw.parse::<Uuid>().unwrap_or_default()
    };
    let gate = match gate::resolve_gate(
        &state,
        "POST",
        &slug,
        &project_id,
        Some(page_id),
        extension,
    )
    .await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    // Before any write: `TimezoneMixin.initial` precedes the view body, so
    // an unknown zone 500s without cloning anything.
    let tz = match request_timezone(pool, &gate.user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let source: Option<SourcePage> = match source_fetch_query(&page_id, &project_id, &slug)
        .into_model()
        .one(&orm(pool))
        .await
    {
        Ok(source) => source,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(source) = source else {
        return Denial::ObjectNotFound.into_response();
    };
    // The private-owner inline guard (`base.py:590-591`): private pages
    // duplicate only for their owner. (Private non-owners normally 403
    // earlier, inside the class, with DRF's default body.)
    if gate::check_duplicate_private(i32::from(source.access), source.owned_by_id == gate.user_id) {
        return json_response(
            StatusCode::FORBIDDEN,
            gate::DUPLICATE_PRIVATE_BODY.to_owned(),
        );
    }
    // All live source projects, `-created_at` (`base.py:594`).
    let link_ids: Vec<Uuid> = match duplicate_links_query(&page_id)
        .into_tuple()
        .all(&orm(pool))
        .await
    {
        Ok(links) => links,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // The clone (`base.py:596-602`): new row, `"<name> (Copy)"`, binary
    // reset, requester ownership. `pk=None` keeps `_state.adding=False`, so
    // crum's non-adding branch sets `updated_by=request.user` too — both
    // audit columns are the requester (QUIRK-clone-keeps-audit).
    // `description_html`/`description_json` are KEPT; `description_stripped`
    // is recomputed by `Page.save()`; labels (M2M) are not copied.
    let new_id = Uuid::new_v4();
    let stripped = pidash_db::app_pages::strip::sync_description_stripped(Some(
        source.description_html.as_str(),
    ));
    let name = format!("{} (Copy)", source.name);
    let now = chrono::Utc::now();
    let db = orm(pool);
    let insert = pages::Entity::insert(pages::ActiveModel {
        id: Set(new_id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        created_by_id: Set(Some(gate.user_id)),
        updated_by_id: Set(Some(gate.user_id)),
        deleted_at: Set(None),
        workspace_id: Set(source.workspace_id),
        name: Set(name),
        description_json: Set(source.description_json.clone()),
        description_binary: Set(None),
        description_html: Set(source.description_html.clone()),
        description_stripped: Set(stripped),
        owned_by_id: Set(gate.user_id),
        access: Set(source.access),
        color: Set(source.color.clone()),
        parent_id: Set(source.parent_id),
        archived_at: Set(source.archived_at),
        is_locked: Set(source.is_locked),
        view_props: Set(source.view_props.clone()),
        logo_props: Set(source.logo_props.clone()),
        is_global: Set(source.is_global),
        moved_to_page: Set(source.moved_to_page),
        moved_to_project: Set(source.moved_to_project),
        sort_order: Set(source.sort_order),
        external_id: Set(source.external_id.clone()),
        external_source: Set(source.external_source.clone()),
    })
    .exec_without_returning(&db)
    .await;
    if let Err(error) = insert {
        // `handle_exception`'s `IntegrityError` branch: practically
        // unreachable (fresh UUIDs), mirrored for completeness.
        return if pidash_db::orm_tx::is_integrity_violation(&error) {
            super::Denial::BadError("The payload is not valid".to_owned()).into_response()
        } else {
            Denial::ServerError.into_response()
        };
    }
    // One bridge per live source project (`base.py:604-611`). Fresh
    // instances (`adding=True`): crum overwrites the passed `updated_by_id`
    // with NULL — bridges store `updated_by=NULL` (QUIRK-bridge-null-updated).
    for link_id in &link_ids {
        let bridge = project_pages::Entity::insert(project_pages::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            created_by_id: Set(Some(gate.user_id)),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            workspace_id: Set(source.workspace_id),
            project_id: Set(*link_id),
            page_id: Set(new_id),
        })
        .exec_without_returning(&db)
        .await;
        if bridge.is_err() {
            return Denial::ServerError.into_response();
        }
    }
    // Both publishes, in source order, best-effort after commit
    // (`base.py:613-626`). `copy_s3` takes the shadowed loop-var id
    // (QUIRK-shadow-project).
    let new_id_text = new_id.to_string();
    enqueue_best_effort(
        pool,
        &pidash_jobs::app_pages::page_transaction_duplicate_job(
            &new_id_text,
            Some(source.description_html.as_str()),
        ),
    )
    .await;
    let effective = shadowed_project_id(&link_ids, &project_id).to_string();
    enqueue_best_effort(
        pool,
        &pidash_jobs::app_pages::copy_s3_duplicate_job(
            &new_id_text,
            &effective,
            &slug,
            &gate.user_id.to_string(),
        ),
    )
    .await;
    // Re-fetch with the `project_ids` annotation + `PageDetailSerializer`
    // (`base.py:628-639`), 201. The row was just inserted; a miss means a
    // concurrent delete, i.e. the serializer's `None` dereference — 500.
    // raw-sql: correlated `ARRAY_AGG(DISTINCT ...) FILTER (WHERE NOT (...))` subquery coalesced to `'{}'::uuid[]` — sea-query has no aggregate FILTER/ARRAY_AGG construct.
    let copy: Option<CopyRow> = match sqlx::query_as(duplicate_refetch_sql()) // raw-sql: correlated ARRAY_AGG(DISTINCT) FILTER subquery coalesced to '{}'::uuid[] — sea-query has no aggregate FILTER/ARRAY_AGG construct.
        .bind(new_id)
        .fetch_optional(pool)
        .await
    {
        Ok(copy) => copy,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(copy) = copy else {
        return Denial::ServerError.into_response();
    };
    let body = serde_json::to_string(&render_copy(&copy, &tz)).expect("serializable row");
    json_response(StatusCode::CREATED, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    const F30_03: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_version_shapes.golden.json"
    );
    const F30_11: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/handlers/io.golden.json"
    );
    const F30_12: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/handlers/versions.golden.json"
    );

    fn golden(path: &str) -> Value {
        let raw = std::fs::read_to_string(path).expect("fixture golden exists");
        serde_json::from_str(&raw).expect("fixture golden is valid JSON")
    }

    fn fixed_uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("11111111-1111-4111-8111-1111111111{n:02x}")).expect("fixed uuid")
    }

    fn fixed_dt() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .single()
            .expect("fixed date")
            + chrono::Duration::microseconds(6_007)
    }

    fn fixed_dtz() -> DateTimeWithTimeZone {
        fixed_dt().into()
    }

    fn fixed_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 4).expect("fixed date")
    }

    fn fixed_row() -> VersionRow {
        VersionRow {
            id: fixed_uuid(1),
            workspace_id: fixed_uuid(2),
            page_id: fixed_uuid(3),
            last_saved_at: fixed_dtz(),
            owned_by_id: fixed_uuid(4),
            created_at: fixed_dtz(),
            updated_at: fixed_dtz(),
            created_by_id: Some(fixed_uuid(4)),
            updated_by_id: None,
        }
    }

    /// F30-03/F30-12: both version shapes, in order, against the goldens.
    #[test]
    fn version_shapes_match_f30_03_and_f30_12() {
        use pidash_services::app_pages::shape::{PAGE_VERSION_DETAIL_FIELDS, PAGE_VERSION_FIELDS};
        let parsed = golden(F30_03);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        let fields = |index: usize| {
            cases[index]["fields"]
                .as_array()
                .expect("golden carries fields")
                .iter()
                .map(|field| field.as_str().expect("field strings"))
                .collect::<Vec<_>>()
        };
        assert_eq!(PAGE_VERSION_FIELDS, fields(0).as_slice());
        assert_eq!(PAGE_VERSION_DETAIL_FIELDS, fields(1).as_slice());
        let branches = golden(F30_12);
        let shapes = &branches["shapes"];
        let shape = |key: &str| {
            shapes[key]
                .as_array()
                .expect("golden carries shapes")
                .iter()
                .map(|field| field.as_str().expect("field strings"))
                .collect::<Vec<_>>()
        };
        assert_eq!(PAGE_VERSION_FIELDS, shape("list").as_slice());
        assert_eq!(PAGE_VERSION_DETAIL_FIELDS, shape("detail").as_slice());
        // Both branches answer 200 under the page permission class.
        for branch in branches["branches"]
            .as_array()
            .expect("golden carries branches")
        {
            assert_eq!(branch["response"]["status"], 200);
        }
        assert!(branches["permission"]
            .as_str()
            .expect("golden carries permission")
            .contains("ProjectPagePermission"));
    }

    /// The list row renders byte-exact: 9 keys in `Meta.fields` order,
    /// datetimes `iso-8601` with the `Z` rewrite, null audit tail.
    #[test]
    fn version_row_renders_byte_exact() {
        let body = serde_json::to_string(&render_row(&fixed_row(), &Tz::UTC)).expect("serializes");
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111101","workspace":"11111111-1111-4111-8111-111111111102","page":"11111111-1111-4111-8111-111111111103","last_saved_at":"2026-01-02T03:04:05.006007Z","owned_by":"11111111-1111-4111-8111-111111111104","created_at":"2026-01-02T03:04:05.006007Z","updated_at":"2026-01-02T03:04:05.006007Z","created_by":"11111111-1111-4111-8111-111111111104","updated_by":null}"#
        );
    }

    /// The detail row renders byte-exact: 12 keys, NULL binary as null,
    /// JSON passed through untouched.
    #[test]
    fn version_detail_renders_byte_exact() {
        let row = VersionDetailRow {
            id: fixed_uuid(1),
            workspace_id: fixed_uuid(2),
            page_id: fixed_uuid(3),
            last_saved_at: fixed_dtz(),
            description_binary: None,
            description_html: "<p>v1</p>".to_owned(),
            description_json: serde_json::json!({"a": 1}),
            owned_by_id: fixed_uuid(4),
            created_at: fixed_dtz(),
            updated_at: fixed_dtz(),
            created_by_id: Some(fixed_uuid(4)),
            updated_by_id: None,
        };
        let body = serde_json::to_string(&render_detail(&row, &Tz::UTC)).expect("serializes");
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111101","workspace":"11111111-1111-4111-8111-111111111102","page":"11111111-1111-4111-8111-111111111103","last_saved_at":"2026-01-02T03:04:05.006007Z","description_binary":null,"description_html":"<p>v1</p>","description_json":{"a":1},"owned_by":"11111111-1111-4111-8111-111111111104","created_at":"2026-01-02T03:04:05.006007Z","updated_at":"2026-01-02T03:04:05.006007Z","created_by":"11111111-1111-4111-8111-111111111104","updated_by":null}"#
        );
    }

    /// QUIRK-binary-base64: non-NULL bytes render standard base64
    /// (infallible — even invalid UTF-8), matching live-Django probes.
    #[test]
    fn binary_renders_base64() {
        assert_eq!(render_binary(b"hello-bytes"), "aGVsbG8tYnl0ZXM=");
        assert_eq!(render_binary(b"\xff\xfe-invalid"), "//4taW52YWxpZA==");
        assert_eq!(render_binary(b""), "");
    }

    /// QUIRK-duplicate-shape: the 201 body carries exactly the 19 observed
    /// keys — `is_favorite`/`label_ids`/`sub_pages_count`/`labels`
    /// `SkipField` out — with `description_json` trailing `description_html`.
    #[test]
    fn duplicate_body_renders_byte_exact() {
        let row = CopyRow {
            id: fixed_uuid(5),
            name: "Probe page (Copy)".to_owned(),
            owned_by_id: fixed_uuid(4),
            access: 0,
            color: String::new(),
            parent_id: None,
            is_locked: false,
            archived_at: None,
            workspace_id: fixed_uuid(2),
            created_at: fixed_dt(),
            updated_at: fixed_dt(),
            created_by_id: Some(fixed_uuid(4)),
            updated_by_id: Some(fixed_uuid(4)),
            view_props: serde_json::json!({"full_width": false}),
            logo_props: serde_json::json!({}),
            description_html: "<p>Hi</p>".to_owned(),
            description_json: serde_json::json!({"type": "doc"}),
            project_ids: vec![fixed_uuid(6)],
        };
        let body = serde_json::to_string(&render_copy(&row, &Tz::UTC)).expect("serializes");
        assert_eq!(
            body,
            r#"{"id":"11111111-1111-4111-8111-111111111105","name":"Probe page (Copy)","owned_by":"11111111-1111-4111-8111-111111111104","access":0,"color":"","parent":null,"parent_id":null,"is_locked":false,"archived_at":null,"workspace":"11111111-1111-4111-8111-111111111102","created_at":"2026-01-02T03:04:05.006007Z","updated_at":"2026-01-02T03:04:05.006007Z","created_by":"11111111-1111-4111-8111-111111111104","updated_by":"11111111-1111-4111-8111-111111111104","view_props":{"full_width":false},"logo_props":{},"project_ids":["11111111-1111-4111-8111-111111111106"],"description_html":"<p>Hi</p>","description_json":{"type":"doc"}}"#
        );
        // A populated `archived_at` renders `YYYY-MM-DD`.
        let archived = CopyRow {
            archived_at: Some(fixed_date()),
            ..row
        };
        let body = serde_json::to_string(&render_copy(&archived, &Tz::UTC)).expect("serializes");
        assert!(body.contains(r#""archived_at":"2026-03-04""#));
    }

    /// QUIRK-shadow-project: the last link id wins; with no links the URL
    /// kwarg survives.
    #[test]
    fn shadowed_project_id_last_wins() {
        let old = fixed_uuid(7);
        let new = fixed_uuid(8);
        let url = fixed_uuid(9);
        assert_eq!(shadowed_project_id(&[new, old], &url), old);
        assert_eq!(shadowed_project_id(&[], &url), url);
    }

    /// F30-11: the private-owner denial body + the duplicate transform pins.
    #[test]
    fn duplicate_denial_and_transform_match_f30_11() {
        let parsed = golden(F30_11);
        let duplicate = parsed["actions"]
            .as_array()
            .expect("golden carries actions")
            .iter()
            .find(|action| action["action"] == "duplicate")
            .expect("golden carries duplicate");
        assert_eq!(duplicate["valid"]["status"], 201);
        assert_eq!(duplicate["private_denied"]["status"], 403);
        assert_eq!(
            duplicate["private_denied"]["body"],
            serde_json::json!({"error": "Permission denied"})
        );
        assert_eq!(
            gate::DUPLICATE_PRIVATE_BODY,
            r#"{"error":"Permission denied"}"#
        );
        let transform = duplicate["transform"].as_str().expect("transform note");
        for needle in [
            "(Copy)",
            "description_binary=None",
            "ProjectPage re-created",
            "project_ids annotation",
        ] {
            assert!(transform.contains(needle), "transform pins {needle}");
        }
    }

    /// The ported quirks, pinned statically on the converted statements: the
    /// merged single-`project_pages`-join fetch (no `projects` table), the
    /// `-created_at` link order, the verbatim `...0001` annotation filter
    /// over the kept re-fetch (raw-sql exception — see its note).
    #[test]
    fn ported_sql_quirks_pinned() {
        use sea_orm::{DbBackend, QueryTrait};
        let page_id = fixed_uuid(3);
        let project_id = fixed_uuid(6);
        let fetch = source_fetch_query(&page_id, &project_id, "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert_eq!(
            fetch.matches("INNER JOIN \"project_pages\"").count(),
            1,
            "{fetch}"
        );
        assert!(!fetch.contains("JOIN \"projects\""), "{fetch}");
        assert!(fetch.contains("INNER JOIN"), "{fetch}");
        assert!(fetch.contains("IS NULL"), "{fetch}");

        let links = duplicate_links_query(&page_id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(links.contains("ORDER BY"), "{links}");
        assert!(links.contains("DESC"), "{links}");
        assert!(links.contains("IS NULL"), "{links}");

        let refetch = duplicate_refetch_sql();
        assert!(refetch.contains("00000000-0000-0000-0000-000000000001"));
        assert!(refetch.contains("LEFT OUTER JOIN") || refetch.contains("pp WHERE pp.page_id"));
        assert!(!refetch.contains("pp.deleted_at"));
        assert!(refetch.contains("'{}'::uuid[]"));

        let list = versions_list_query("acme", &page_id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(list.contains("ORDER BY"), "{list}");
        assert!(list.contains("DESC"), "{list}");
        assert!(list.contains("IS NULL"), "{list}");

        let detail = version_detail_query("acme", &page_id, &fixed_uuid(1))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(detail.contains("description_binary"), "{detail}");
        assert!(detail.contains("IS NULL"), "{detail}");
    }
}
