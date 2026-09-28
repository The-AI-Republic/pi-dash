//! Intake handlers: `IntakeIssuePublicViewSet` (D-02, stage 4).
//!
//! Port of `apps/api/pi_dash/space/views/intake.py:31-280` with routes
//! from `space/urls/intake.py:15-29`. Five actions over three routes (the
//! `inbox-issues` alias shares the viewset); the `workspace-boards` route
//! in the same URL module belongs to the project handlers (PIDASHCONV-174)
//! and is NOT registered here — it keeps proxying to Django.
//!
//! Action map (`list` bypasses `get_queryset`, so the mixin queryset never
//! serves traffic):
//!
//! * `list` (`:56-105`): intake-`None` 400, `issue_filters(query_params,
//!   "GET")` list compiled by [`super::filters`], rows rendered through
//!   the APP `IssueStateIntakeSerializer` (`app/serializers/intake.py:
//!   127-139` — see the twin note below), 200.
//! * `create` (`:107-173`): intake-`None` 400, name-required 400, priority
//!   allowlist 400, triage auto-create, `Issue` + `IntakeIssue` writes,
//!   `issue.activity.created` task, APP detail rendering, 200 (not 201).
//! * `retrieve` (`:236-256`): scoped lookups, APP detail rendering, 200.
//! * `partial_update` (`:175-234`): creator-ownership 400, the APP
//!   `IssueCreateSerializer` partial path (3-key subset, sanitized HTML,
//!   sync-lock), `issue.activity.updated` task, APP `IssueCreate`
//!   rendering, 200.
//! * `destroy` (`:258-280`): creator-ownership 400, SOFT delete of the
//!   intake row only (the `Issue` survives), 204.
//!
//! Serializer twin (read before touching the rendering): `views/intake.py`
//! imports `IssueStateIntakeSerializer`, `IssueCreateSerializer` and
//! `IssueSerializer` from `pi_dash.app.serializers`, NOT from
//! `space/serializer/`. The wire therefore follows the app twins:
//! no `bridge_id` anywhere (the annotation exists on list rows but the
//! app serializer never declares it), `sub_issues_count` on list rows
//! only (detail/create/patch instances carry no such annotation, so DRF
//! skips the read-only field), and the app `ProjectLite` nest
//! (`cover_image_url`/`logo_props`/`is_default` instead of the space
//! `icon_prop`/`emoji`). The identical nests (state/label/user/
//! intake-lite) reuse `pidash_services::space::serializers` views.
//!
//! `get_queryset` (`:37-54`) is NOT served: it resolves the board from
//! `slug`/`project_id` kwargs, but the routes only supply
//! `anchor`/`intake_id`/`pk`, so both lookups are `NULL` and the path
//! always misses. The queries layer keeps the shape for the record
//! (`intake_dead_board_get_sql`); no handler calls it.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * QUIRK-priority-default (`:119-126` vs `:149`): creation validates
//!   `priority` defaulting to `"none"` but inserts `"low"`.
//! * QUIRK-no-workspace-insert (`:145-152,:165-170`): the writes pass no
//!   `workspace_id`; `ProjectBaseModel.save` backfills it from the project
//!   row, so the handlers read the board's workspace through the project.
//! * QUIRK-triage-resequence (`db/models/state.py:132-139`): `State.save`
//!   reseeds `sequence` to `max+15000` (and `slug` to `"triage"`) whenever
//!   the project already has states; `65000` survives only on empty
//!   projects.
//! * QUIRK-intake-id-from-url (`:165-170`): the bridge row takes
//!   `intake_id` from the URL kwarg, not from `board.intake`.
//! * QUIRK-destroy-keeps-issue (`:258-280`): destroy soft-deletes the
//!   intake row only (verified live: `deleted_at` set, issue untouched),
//!   and fires `soft_delete_related_objects("db", "intakeissue", pk,
//!   "default")`. The queries layer's hard-`DELETE` builder text does not
//!   match the live `SoftDeleteModel.delete()` path; the handler follows
//!   Django, not the builder.
//! * QUIRK-pop-mutates (`:197`): a missing/non-dict `issue` key raises
//!   (`AttributeError`) → the 500 envelope (verified live).
//! * QUIRK-updated_at-filter (`utils/issue_filters.py`): `updated_at`
//!   filters `created_at` (owned by [`super::filters`]).
//! * Dead `allow_triage_state`/`project_id` serializer context (`:215`):
//!   accepted and ignored — the app `validate` reads `allow_triage_state`
//!   but the 3-key subset never carries `state`, and `update` never reads
//!   `project_id`. Ported as a no-op (the sync-lock/date/sanitize arms
//!   that DO fire are implemented).
//!
//! Task delivery: `serve` carries no AMQP publisher (only the worker
//! does), so `.delay()` calls enqueue a [`pidash_jobs::queue::NewJob`]
//! into `rust_job_queue`; the worker forwards Python-owned names to the
//! broker. Enqueue is best-effort after commit: a missing queue table (or
//! broker outage on Django's side) must not turn user-visible writes into
//! 500s, so failures are traced and the response stands.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_services::space::guards::{self, AnchorLookup, ExceptionKind};
use pidash_services::space::queries::intake_assets as queries;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::{
    filters::{self, FilterValue},
    guards_error, owned, raw_json_response, Denial, QueryMap,
};

/// Owned intake routes: the collection (both aliases share the viewset)
/// and the detail. Every other method proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/",
            owned(
                axum::routing::get(collection_list).post(collection_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/intakes/{intake_id}/inbox-issues/",
            owned(
                axum::routing::get(collection_list).post(collection_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/",
            owned(
                axum::routing::get(detail_retrieve)
                    .patch(detail_partial_update)
                    .delete(detail_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
}

// ---------------------------------------------------------------------------
// Request context: auth + anchor board
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone (`TimezoneMixin.initial` activates
/// the user's zone; datetimes render in it).
struct Actor {
    id: Uuid,
    timezone: chrono_tz::Tz,
}

/// `request.user` through Django-session auth.
///
/// `BaseSessionAuthentication` + `IsAuthenticated`
/// (`views/base.py:48,52`): anonymous answers the DRF `NotAuthenticated`
/// body before anything else runs.
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// The resolved anchor board: tenant scope. `board.intake` itself is
/// only ever `None`-checked (the bridge takes `intake_id` from the URL),
/// so the gate consumes it and the struct keeps the tenant pair.
struct Board {
    workspace_id: Uuid,
    project_id: Uuid,
}

/// `DeployBoard.objects.get(anchor=anchor, entity_name="project")`
/// (`views/intake.py:57`; same call at `:108,:176,:237,:259`): a miss
/// raises `DoesNotExist`, which answers the `ObjectDoesNotExist` envelope
/// (verified live: 404 + body, the dispatch-computed response).
#[allow(clippy::result_large_err)]
async fn load_board(pool: &PgPool, anchor: &str) -> Result<Board, Response> {
    let row = sqlx::query(&queries::intake_board_get_sql())
        .bind(anchor)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
    let Some(row) = row else {
        return Err(guards_error(
            guards::resolve_anchor(AnchorLookup::GetRaises, false).unwrap_err(),
        )
        .into_response());
    };
    // BOARD_COLUMNS order (`project_meta.rs`): workspace 6, project 7,
    // intake 13. `project_id` is non-nullable on the board row.
    let workspace_id: Uuid = row
        .try_get(6)
        .map_err(|_| Denial::ServerError.into_response())?;
    let project_id: Option<Uuid> = row
        .try_get(7)
        .map_err(|_| Denial::ServerError.into_response())?;
    let intake_id: Option<Uuid> = row
        .try_get(13)
        .map_err(|_| Denial::ServerError.into_response())?;
    let Some(project_id) = project_id else {
        return Err(Denial::ServerError.into_response());
    };
    // `board.intake is None` → 400 on every action (`:58,:109,:177,
    // `:238,:260`).
    if intake_id.is_none() {
        return Err(guards_error(guards::intake_not_enabled()));
    }
    Ok(Board {
        workspace_id,
        project_id,
    })
}

/// Parse a UUID path segment: Django's `<uuid:>` converter 404s on
/// garbage, so unparseable ids behave as missing rows.
#[allow(clippy::result_large_err)]
fn parse_id(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::NotFound.into_response())
}

/// Convert a guards-layer denial (`Result<(), ErrorBody>`) into its wire
/// response. Call sites only invoke this on the denying branch, so `Ok`
/// degrades to the 500 envelope instead of panicking.
fn guard_denial(result: Result<(), guards::ErrorBody>) -> Response {
    match result {
        Ok(()) => Denial::ServerError.into_response(),
        Err(error) => guards_error(error),
    }
}

/// Map a database failure onto the `handle_exception` matrix: integrity
/// violations (SQLSTATE 23xxx) → `{"error": "The payload is not valid"}`
/// 400; anything else → the 500 envelope.
fn db_error(error: sqlx::Error) -> Response {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref().unwrap_or("").starts_with("23") {
            return guards_error(guards::handle_exception(ExceptionKind::IntegrityError));
        }
    }
    Denial::ServerError.into_response()
}

// ---------------------------------------------------------------------------
// Rows: issues plus the relations the serializers render
// ---------------------------------------------------------------------------

/// One `issues` row with every column the responses render.
struct IssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    point: Option<i32>,
    estimate_point_id: Option<Uuid>,
    name: String,
    description_json: serde_json::Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    priority: String,
    complexity_score: i32,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<chrono::NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    type_id: Option<Uuid>,
    git_work_branch: String,
    created_via: Option<String>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
}

impl IssueRow {
    fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            project_id: row.try_get("project_id")?,
            workspace_id: row.try_get("workspace_id")?,
            parent_id: row.try_get("parent_id")?,
            state_id: row.try_get("state_id")?,
            point: row.try_get("point")?,
            estimate_point_id: row.try_get("estimate_point_id")?,
            name: row.try_get("name")?,
            description_json: row.try_get("description_json")?,
            description_html: row.try_get("description_html")?,
            description_stripped: row.try_get("description_stripped")?,
            description_binary: row.try_get("description_binary")?,
            priority: row.try_get("priority")?,
            complexity_score: row.try_get("complexity_score")?,
            start_date: row.try_get("start_date")?,
            target_date: row.try_get("target_date")?,
            sequence_id: row.try_get("sequence_id")?,
            sort_order: row.try_get("sort_order")?,
            completed_at: row.try_get("completed_at")?,
            archived_at: row.try_get("archived_at")?,
            is_draft: row.try_get("is_draft")?,
            external_source: row.try_get("external_source")?,
            external_id: row.try_get("external_id")?,
            type_id: row.try_get("type_id")?,
            git_work_branch: row.try_get("git_work_branch")?,
            created_via: row.try_get("created_via")?,
            assigned_pod_id: row.try_get("assigned_pod_id")?,
            agent_executor: row.try_get("agent_executor")?,
        })
    }
}

const ISSUE_COLUMNS: &str = "id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, created_via, assigned_pod_id, agent_executor";

/// `Issue.objects.get(pk, workspace_id, project_id)` (`:199-203,:250-254`).
async fn fetch_issue(
    pool: &PgPool,
    issue_id: &Uuid,
    board: &Board,
) -> Result<Option<IssueRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {ISSUE_COLUMNS} FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1 AND \"issues\".\"workspace_id\" = $2 AND \"issues\".\"project_id\" = $3)"
    ))
    .bind(issue_id)
    .bind(board.workspace_id)
    .bind(board.project_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| IssueRow::get(&row)).transpose()
}

/// One `states` row for the lite nest (plain join: `select_related`
/// applies no soft-delete scope).
#[derive(Debug, Clone)]
struct StateLite {
    id: Uuid,
    name: String,
    color: String,
    group: String,
}

async fn fetch_state(pool: &PgPool, state_id: &Uuid) -> Result<Option<StateLite>, sqlx::Error> {
    sqlx::query("SELECT id, name, color, \"group\" FROM \"states\" WHERE \"states\".\"id\" = $1")
        .bind(state_id)
        .fetch_optional(pool)
        .await?
        .map(|row| {
            Ok(StateLite {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                color: row.try_get("color")?,
                group: row.try_get("group")?,
            })
        })
        .transpose()
}

/// One `projects` row for the app `ProjectLite` nest (plain join, like
/// `select_related`).
struct ProjectRow {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_asset_id: Option<Uuid>,
    logo_props: serde_json::Value,
    description: String,
    is_default: bool,
}

async fn fetch_project(
    pool: &PgPool,
    project_id: &Uuid,
) -> Result<Option<ProjectRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, identifier, name, cover_image, cover_image_asset_id, logo_props, description, is_default FROM \"projects\" WHERE \"projects\".\"id\" = $1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        Ok(ProjectRow {
            id: row.try_get("id")?,
            identifier: row.try_get("identifier")?,
            name: row.try_get("name")?,
            cover_image: row.try_get("cover_image")?,
            cover_image_asset_id: row.try_get("cover_image_asset_id")?,
            logo_props: row.try_get("logo_props")?,
            description: row.try_get("description")?,
            is_default: row.try_get("is_default")?,
        })
    })
    .transpose()
}

/// `cover_image_url` (`db/models/project.py:176-185`): the cover asset's
/// URL, else the raw `cover_image` text, else null.
async fn cover_image_url(
    pool: &PgPool,
    project: &ProjectRow,
) -> Result<Option<String>, sqlx::Error> {
    if let Some(asset_id) = project.cover_image_asset_id {
        if let Some(url) = asset_url(pool, &asset_id).await? {
            return Ok(Some(url));
        }
    }
    Ok(project.cover_image.clone())
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-99`). Cover assets are
/// `PROJECT_COVER` (`/api/assets/v2/static/<id>/`); the sibling branches
/// are ported so any asset kind resolves exactly.
async fn asset_url(pool: &PgPool, asset_id: &Uuid) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT entity_type, workspace_id, project_id, issue_id FROM \"file_assets\" WHERE \"file_assets\".\"id\" = $1",
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let entity_type: String = row.try_get("entity_type")?;
    match entity_type.as_str() {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        "ISSUE_ATTACHMENT" => {
            let workspace_id: Uuid = row.try_get("workspace_id")?;
            let project_id: Option<Uuid> = row.try_get("project_id")?;
            let issue_id: Option<Uuid> = row.try_get("issue_id")?;
            let workspace_slug: Option<String> = sqlx::query_scalar(
                "SELECT slug FROM \"workspaces\" WHERE \"workspaces\".\"id\" = $1",
            )
            .bind(workspace_id)
            .fetch_optional(pool)
            .await?
            .flatten();
            match (workspace_slug, project_id, issue_id) {
                (Some(slug), Some(project), Some(issue)) => Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
                ))),
                _ => Ok(None),
            }
        }
        "ISSUE_DESCRIPTION"
        | "COMMENT_DESCRIPTION"
        | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => {
            let workspace_id: Uuid = row.try_get("workspace_id")?;
            let project_id: Option<Uuid> = row.try_get("project_id")?;
            let workspace_slug: Option<String> = sqlx::query_scalar(
                "SELECT slug FROM \"workspaces\" WHERE \"workspaces\".\"id\" = $1",
            )
            .bind(workspace_id)
            .fetch_optional(pool)
            .await?
            .flatten();
            match (workspace_slug, project_id) {
                (Some(slug), Some(project)) => Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
                ))),
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

/// One label row for `label_details` + the `labels` PK list (single fetch
/// serves both, like the shared M2M queryset).
#[derive(Debug, Clone)]
struct LabelRow {
    id: Uuid,
    name: String,
    color: String,
}

async fn fetch_labels(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<(Vec<Uuid>, Vec<LabelRow>), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT \"labels\".\"id\", \"labels\".\"name\", \"labels\".\"color\" FROM \"labels\" INNER JOIN \"issue_labels\" ON (\"labels\".\"id\" = \"issue_labels\".\"label_id\") WHERE \"issue_labels\".\"issue_id\" = $1",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await?;
    let mut ids = Vec::with_capacity(rows.len());
    let mut details = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        ids.push(id);
        details.push(LabelRow {
            id,
            name: row.try_get("name")?,
            color: row.try_get("color")?,
        });
    }
    Ok((ids, details))
}

/// One user row for `assignee_details` + the `assignees` PK list.
#[derive(Debug, Clone)]
struct AssigneeRow {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

async fn fetch_assignees(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<(Vec<Uuid>, Vec<AssigneeRow>), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT \"users\".\"id\", \"users\".\"first_name\", \"users\".\"last_name\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\", \"users\".\"is_bot\", \"users\".\"display_name\" FROM \"users\" INNER JOIN \"issue_assignees\" ON (\"users\".\"id\" = \"issue_assignees\".\"assignee_id\") WHERE \"issue_assignees\".\"issue_id\" = $1",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await?;
    let mut ids = Vec::with_capacity(rows.len());
    let mut details = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        ids.push(id);
        let avatar_asset_id: Option<Uuid> = row.try_get("avatar_asset_id")?;
        let avatar: String = row.try_get("avatar")?;
        let mut avatar_url = None;
        if let Some(asset_id) = avatar_asset_id {
            avatar_url = asset_url(pool, &asset_id).await?;
        }
        if avatar_url.is_none() && !avatar.is_empty() {
            avatar_url = Some(avatar.clone());
        }
        details.push(AssigneeRow {
            id,
            first_name: row.try_get("first_name")?,
            last_name: row.try_get("last_name")?,
            avatar,
            avatar_url,
            is_bot: row.try_get("is_bot")?,
            display_name: row.try_get("display_name")?,
        });
    }
    Ok((ids, details))
}

/// One bridge row for the `issue_intake` nest (the `.only(status,
/// duplicate_to, snoozed_till, source)` prefetch, newest first per the
/// `IntakeIssue` ordering).
#[derive(Debug, Clone)]
struct BridgeRow {
    id: Uuid,
    status: i32,
    duplicate_to_id: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<String>,
}

async fn fetch_bridges(
    pool: &PgPool,
    issue_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<BridgeRow>>, sqlx::Error> {
    let mut map: HashMap<Uuid, Vec<BridgeRow>> = HashMap::new();
    if issue_ids.is_empty() {
        return Ok(map);
    }
    let placeholders = issue_ids
        .iter()
        .enumerate()
        .map(|(index, _)| format!("${}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    // The fixture prefetch shape (`intake_issue_prefetch_sql`), plus the
    // model ordering (`-created_at`) Django applies when evaluating it.
    let sql = format!(
        "SELECT \"intake_issues\".\"id\", \"intake_issues\".\"status\", \"intake_issues\".\"duplicate_to_id\", \"intake_issues\".\"snoozed_till\", \"intake_issues\".\"source\", \"intake_issues\".\"issue_id\" FROM \"intake_issues\" WHERE (\"intake_issues\".\"deleted_at\" IS NULL AND \"intake_issues\".\"issue_id\" IN ({placeholders})) ORDER BY \"intake_issues\".\"created_at\" DESC"
    );
    let mut query = sqlx::query(&sql);
    for issue_id in issue_ids {
        query = query.bind(issue_id);
    }
    for row in query.fetch_all(pool).await? {
        let issue_id: Uuid = row.try_get("issue_id")?;
        map.entry(issue_id).or_default().push(BridgeRow {
            id: row.try_get("id")?,
            status: row.try_get("status")?,
            duplicate_to_id: row.try_get("duplicate_to_id")?,
            snoozed_till: row.try_get("snoozed_till")?,
            source: row.try_get("source")?,
        });
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Rendering: the app-twin wire shapes
// ---------------------------------------------------------------------------

/// DRF `DateTimeField.to_representation` in the request zone
/// (`enforce_timezone` → `get_current_timezone`, i.e. the activated user
/// zone): ISO-8601 with `Z` for UTC, `±HH:MM` otherwise, sub-second
/// digits only when nonzero.
fn render_dt(moment: &DateTime<Utc>, timezone: chrono_tz::Tz) -> String {
    use chrono::TimeZone;
    let local = timezone.from_utc_datetime(&moment.naive_utc());
    let text = local.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false);
    if text.ends_with("+00:00") {
        format!("{}Z", &text[..text.len() - 6])
    } else {
        text
    }
}

fn uuid_string(id: &Uuid) -> serde_json::Value {
    serde_json::Value::String(id.to_string())
}

fn opt_uuid(id: &Option<Uuid>) -> serde_json::Value {
    id.as_ref()
        .map(uuid_string)
        .unwrap_or(serde_json::Value::Null)
}

fn opt_string(text: &Option<String>) -> serde_json::Value {
    text.as_ref()
        .map(|text| serde_json::Value::String(text.clone()))
        .unwrap_or(serde_json::Value::Null)
}

fn opt_dt(moment: &Option<DateTime<Utc>>, timezone: chrono_tz::Tz) -> serde_json::Value {
    moment
        .as_ref()
        .map(|moment| serde_json::Value::String(render_dt(moment, timezone)))
        .unwrap_or(serde_json::Value::Null)
}

/// Everything one app-twin detail row needs, fetched in one place.
struct Detail {
    issue: IssueRow,
    state: Option<StateLite>,
    project: ProjectRow,
    cover_url: Option<String>,
    label_ids: Vec<Uuid>,
    labels: Vec<LabelRow>,
    assignee_ids: Vec<Uuid>,
    assignees: Vec<AssigneeRow>,
    bridges: Vec<BridgeRow>,
}

async fn load_detail(
    pool: &PgPool,
    issue: IssueRow,
    bridges: Vec<BridgeRow>,
) -> Result<Detail, sqlx::Error> {
    let state = match issue.state_id {
        Some(state_id) => fetch_state(pool, &state_id).await?,
        None => None,
    };
    // `select_related` rows always exist in practice (non-nullable FKs);
    // a missing row degrades to the same null DRF renders for a null
    // source, never to a 500.
    let project = fetch_project(pool, &issue.project_id)
        .await?
        .unwrap_or_else(|| ProjectRow {
            id: issue.project_id,
            identifier: String::new(),
            name: String::new(),
            cover_image: None,
            cover_image_asset_id: None,
            logo_props: serde_json::Value::Object(Default::default()),
            description: String::new(),
            is_default: false,
        });
    let cover_url = cover_image_url(pool, &project).await?;
    let (label_ids, labels) = fetch_labels(pool, &issue.id).await?;
    let (assignee_ids, assignees) = fetch_assignees(pool, &issue.id).await?;
    Ok(Detail {
        issue,
        state,
        project,
        cover_url,
        label_ids,
        labels,
        assignee_ids,
        assignees,
        bridges,
    })
}

/// App `ProjectLiteSerializer` (`app/serializers/project.py:120-133`):
/// `cover_image_url`/`logo_props`/`is_default` where the space twin has
/// `icon_prop`/`emoji`.
fn render_app_project(project: &ProjectRow, cover_url: &Option<String>) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(8);
    map.insert("id".to_string(), uuid_string(&project.id));
    map.insert(
        "identifier".to_string(),
        serde_json::Value::String(project.identifier.clone()),
    );
    map.insert(
        "name".to_string(),
        serde_json::Value::String(project.name.clone()),
    );
    map.insert("cover_image".to_string(), opt_string(&project.cover_image));
    map.insert("cover_image_url".to_string(), opt_string(cover_url));
    map.insert("logo_props".to_string(), project.logo_props.clone());
    map.insert(
        "description".to_string(),
        serde_json::Value::String(project.description.clone()),
    );
    map.insert(
        "is_default".to_string(),
        serde_json::Value::Bool(project.is_default),
    );
    serde_json::Value::Object(map)
}

fn render_state_lite(state: &StateLite) -> serde_json::Value {
    serde_json::json!({
        "id": state.id.to_string(),
        "name": state.name,
        "color": state.color,
        "group": state.group,
    })
}

fn render_label(label: &LabelRow) -> serde_json::Value {
    serde_json::json!({
        "id": label.id.to_string(),
        "name": label.name,
        "color": label.color,
    })
}

fn render_assignee(user: &AssigneeRow) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(7);
    map.insert("id".to_string(), uuid_string(&user.id));
    map.insert(
        "first_name".to_string(),
        serde_json::Value::String(user.first_name.clone()),
    );
    map.insert(
        "last_name".to_string(),
        serde_json::Value::String(user.last_name.clone()),
    );
    map.insert(
        "avatar".to_string(),
        serde_json::Value::String(user.avatar.clone()),
    );
    map.insert("avatar_url".to_string(), opt_string(&user.avatar_url));
    map.insert("is_bot".to_string(), serde_json::Value::Bool(user.is_bot));
    map.insert(
        "display_name".to_string(),
        serde_json::Value::String(user.display_name.clone()),
    );
    serde_json::Value::Object(map)
}

fn render_bridge(bridge: &BridgeRow, timezone: chrono_tz::Tz) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(5);
    map.insert("id".to_string(), uuid_string(&bridge.id));
    map.insert(
        "status".to_string(),
        serde_json::Value::Number(bridge.status.into()),
    );
    map.insert(
        "duplicate_to".to_string(),
        opt_uuid(&bridge.duplicate_to_id),
    );
    map.insert(
        "snoozed_till".to_string(),
        opt_dt(&bridge.snoozed_till, timezone),
    );
    map.insert("source".to_string(), opt_string(&bridge.source));
    serde_json::Value::Object(map)
}

/// App `IssueStateIntakeSerializer` row (`app/serializers/intake.py:
/// 127-139`): the six declared nests first (no `bridge_id` — the app twin
/// never declares it), then every `Issue` column except `workpad`, in
/// model order. `sub_issues_count` renders on list rows only: detail and
/// create instances carry no such annotation, so DRF skips the read-only
/// field there.
#[allow(clippy::result_large_err)]
fn render_app_detail(
    detail: &Detail,
    timezone: chrono_tz::Tz,
    sub_issues_count: Option<i64>,
) -> Result<serde_json::Value, Response> {
    let issue = &detail.issue;
    if issue.description_binary.is_some() {
        // `BinaryField` has no DRF JSON mapping: rendering fails in
        // Django too (500 through the renderer).
        return Err(Denial::ServerError.into_response());
    }
    let mut map = serde_json::Map::with_capacity(48);
    map.insert("id".to_string(), uuid_string(&issue.id));
    map.insert(
        "state_detail".to_string(),
        detail
            .state
            .as_ref()
            .map(render_state_lite)
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "project_detail".to_string(),
        render_app_project(&detail.project, &detail.cover_url),
    );
    map.insert(
        "label_details".to_string(),
        detail.labels.iter().map(render_label).collect(),
    );
    map.insert(
        "assignee_details".to_string(),
        detail.assignees.iter().map(render_assignee).collect(),
    );
    if let Some(count) = sub_issues_count {
        map.insert(
            "sub_issues_count".to_string(),
            serde_json::Value::Number(count.into()),
        );
    }
    map.insert(
        "issue_intake".to_string(),
        detail
            .bridges
            .iter()
            .map(|bridge| render_bridge(bridge, timezone))
            .collect(),
    );
    map.insert(
        "created_at".to_string(),
        serde_json::Value::String(render_dt(&issue.created_at, timezone)),
    );
    map.insert(
        "updated_at".to_string(),
        serde_json::Value::String(render_dt(&issue.updated_at, timezone)),
    );
    map.insert(
        "deleted_at".to_string(),
        opt_dt(&issue.deleted_at, timezone),
    );
    map.insert(
        "point".to_string(),
        issue
            .point
            .map(|point| point.into())
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "name".to_string(),
        serde_json::Value::String(issue.name.clone()),
    );
    map.insert(
        "description_json".to_string(),
        issue.description_json.clone(),
    );
    map.insert(
        "description_html".to_string(),
        serde_json::Value::String(issue.description_html.clone()),
    );
    map.insert(
        "description_stripped".to_string(),
        opt_string(&issue.description_stripped),
    );
    map.insert("description_binary".to_string(), serde_json::Value::Null);
    map.insert(
        "priority".to_string(),
        serde_json::Value::String(issue.priority.clone()),
    );
    map.insert(
        "complexity_score".to_string(),
        serde_json::Value::Number(issue.complexity_score.into()),
    );
    map.insert(
        "start_date".to_string(),
        issue
            .start_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "target_date".to_string(),
        issue
            .target_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "sequence_id".to_string(),
        serde_json::Value::Number(issue.sequence_id.into()),
    );
    map.insert(
        "sort_order".to_string(),
        serde_json::Number::from_f64(issue.sort_order)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "completed_at".to_string(),
        opt_dt(&issue.completed_at, timezone),
    );
    map.insert(
        "archived_at".to_string(),
        issue
            .archived_at
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "is_draft".to_string(),
        serde_json::Value::Bool(issue.is_draft),
    );
    map.insert(
        "external_source".to_string(),
        opt_string(&issue.external_source),
    );
    map.insert("external_id".to_string(), opt_string(&issue.external_id));
    map.insert(
        "git_work_branch".to_string(),
        serde_json::Value::String(issue.git_work_branch.clone()),
    );
    map.insert("created_via".to_string(), opt_string(&issue.created_via));
    // Live wire order (DRF field build, not model-definition order):
    // `agent_executor` precedes the audit/FK block, and `assigned_pod`
    // trails `type` just before the M2M lists.
    map.insert(
        "agent_executor".to_string(),
        opt_string(&issue.agent_executor),
    );
    map.insert("created_by".to_string(), opt_uuid(&issue.created_by_id));
    map.insert("updated_by".to_string(), opt_uuid(&issue.updated_by_id));
    map.insert("project".to_string(), uuid_string(&issue.project_id));
    map.insert("workspace".to_string(), uuid_string(&issue.workspace_id));
    map.insert("parent".to_string(), opt_uuid(&issue.parent_id));
    map.insert("state".to_string(), opt_uuid(&issue.state_id));
    map.insert(
        "estimate_point".to_string(),
        opt_uuid(&issue.estimate_point_id),
    );
    map.insert("type".to_string(), opt_uuid(&issue.type_id));
    map.insert("assigned_pod".to_string(), opt_uuid(&issue.assigned_pod_id));
    map.insert(
        "assignees".to_string(),
        detail.assignee_ids.iter().map(uuid_string).collect(),
    );
    map.insert(
        "labels".to_string(),
        detail.label_ids.iter().map(uuid_string).collect(),
    );
    Ok(serde_json::Value::Object(map))
}

/// App `IssueCreateSerializer` row (`app/serializers/issue.py:136-182`):
/// the `*_id` declared fields first (`label_ids`/`assignee_ids` are
/// write-only, so `super()` drops them and the override re-adds them from
/// `initial_data` — always `[]` on this path, whose input carries only the
/// 3-key `issue` subset), then every `Issue` column except `workpad` and
/// `assigned_pod` (surfaced as `assigned_pod_id`), in model order.
#[allow(clippy::result_large_err)]
fn render_app_issue_create(
    detail: &Detail,
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, Response> {
    let issue = &detail.issue;
    if issue.description_binary.is_some() {
        return Err(Denial::ServerError.into_response());
    }
    let mut map = serde_json::Map::with_capacity(48);
    map.insert("id".to_string(), uuid_string(&issue.id));
    map.insert("state_id".to_string(), opt_uuid(&issue.state_id));
    map.insert("parent_id".to_string(), opt_uuid(&issue.parent_id));
    map.insert(
        "assigned_pod_id".to_string(),
        opt_uuid(&issue.assigned_pod_id),
    );
    map.insert("project_id".to_string(), uuid_string(&issue.project_id));
    map.insert("workspace_id".to_string(), uuid_string(&issue.workspace_id));
    map.insert(
        "created_at".to_string(),
        serde_json::Value::String(render_dt(&issue.created_at, timezone)),
    );
    map.insert(
        "updated_at".to_string(),
        serde_json::Value::String(render_dt(&issue.updated_at, timezone)),
    );
    map.insert(
        "deleted_at".to_string(),
        opt_dt(&issue.deleted_at, timezone),
    );
    map.insert(
        "point".to_string(),
        issue
            .point
            .map(|point| point.into())
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "name".to_string(),
        serde_json::Value::String(issue.name.clone()),
    );
    map.insert(
        "description_json".to_string(),
        issue.description_json.clone(),
    );
    map.insert(
        "description_html".to_string(),
        serde_json::Value::String(issue.description_html.clone()),
    );
    map.insert(
        "description_stripped".to_string(),
        opt_string(&issue.description_stripped),
    );
    map.insert("description_binary".to_string(), serde_json::Value::Null);
    map.insert(
        "priority".to_string(),
        serde_json::Value::String(issue.priority.clone()),
    );
    map.insert(
        "complexity_score".to_string(),
        serde_json::Value::Number(issue.complexity_score.into()),
    );
    map.insert(
        "start_date".to_string(),
        issue
            .start_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "target_date".to_string(),
        issue
            .target_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "sequence_id".to_string(),
        serde_json::Value::Number(issue.sequence_id.into()),
    );
    map.insert(
        "sort_order".to_string(),
        serde_json::Number::from_f64(issue.sort_order)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "completed_at".to_string(),
        opt_dt(&issue.completed_at, timezone),
    );
    map.insert(
        "archived_at".to_string(),
        issue
            .archived_at
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "is_draft".to_string(),
        serde_json::Value::Bool(issue.is_draft),
    );
    map.insert(
        "external_source".to_string(),
        opt_string(&issue.external_source),
    );
    map.insert("external_id".to_string(), opt_string(&issue.external_id));
    map.insert(
        "git_work_branch".to_string(),
        serde_json::Value::String(issue.git_work_branch.clone()),
    );
    map.insert("created_via".to_string(), opt_string(&issue.created_via));
    map.insert(
        "agent_executor".to_string(),
        opt_string(&issue.agent_executor),
    );
    map.insert("created_by".to_string(), opt_uuid(&issue.created_by_id));
    map.insert("updated_by".to_string(), opt_uuid(&issue.updated_by_id));
    map.insert("project".to_string(), uuid_string(&issue.project_id));
    map.insert("workspace".to_string(), uuid_string(&issue.workspace_id));
    map.insert("parent".to_string(), opt_uuid(&issue.parent_id));
    map.insert("state".to_string(), opt_uuid(&issue.state_id));
    map.insert(
        "estimate_point".to_string(),
        opt_uuid(&issue.estimate_point_id),
    );
    map.insert("type".to_string(), opt_uuid(&issue.type_id));
    map.insert(
        "assignees".to_string(),
        detail.assignee_ids.iter().map(uuid_string).collect(),
    );
    map.insert(
        "labels".to_string(),
        detail.label_ids.iter().map(uuid_string).collect(),
    );
    // `to_representation` override: `initial_data` carries no
    // `assignee_ids`/`label_ids` on this path, so both render `[]`.
    map.insert(
        "assignee_ids".to_string(),
        serde_json::Value::Array(Vec::new()),
    );
    map.insert(
        "label_ids".to_string(),
        serde_json::Value::Array(Vec::new()),
    );
    Ok(serde_json::Value::Object(map))
}

/// App `IssueSerializer` snapshot (`app/serializers/issue.py:1039-1093`)
/// for the update activity's `current_instance`: `Meta.fields` order with
/// the unannotated fields skipped (no `cycle_id`/`module_ids`/`label_ids`/
/// `assignee_ids`/counts on a plain instance — DRF drops read-only fields
/// whose attribute is missing).
fn render_issue_snapshot(
    detail: &Detail,
    timezone: chrono_tz::Tz,
    is_synced: bool,
) -> serde_json::Value {
    let issue = &detail.issue;
    let mut map = serde_json::Map::with_capacity(24);
    map.insert("id".to_string(), uuid_string(&issue.id));
    map.insert(
        "name".to_string(),
        serde_json::Value::String(issue.name.clone()),
    );
    map.insert("state_id".to_string(), opt_uuid(&issue.state_id));
    map.insert(
        "sort_order".to_string(),
        serde_json::Number::from_f64(issue.sort_order)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "completed_at".to_string(),
        opt_dt(&issue.completed_at, timezone),
    );
    map.insert(
        "estimate_point".to_string(),
        opt_uuid(&issue.estimate_point_id),
    );
    map.insert(
        "priority".to_string(),
        serde_json::Value::String(issue.priority.clone()),
    );
    map.insert(
        "complexity_score".to_string(),
        serde_json::Value::Number(issue.complexity_score.into()),
    );
    map.insert(
        "start_date".to_string(),
        issue
            .start_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "target_date".to_string(),
        issue
            .target_date
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "sequence_id".to_string(),
        serde_json::Value::Number(issue.sequence_id.into()),
    );
    map.insert("project_id".to_string(), uuid_string(&issue.project_id));
    map.insert("parent_id".to_string(), opt_uuid(&issue.parent_id));
    map.insert(
        "assigned_pod_id".to_string(),
        opt_uuid(&issue.assigned_pod_id),
    );
    map.insert(
        "agent_executor".to_string(),
        opt_string(&issue.agent_executor),
    );
    map.insert(
        "created_at".to_string(),
        serde_json::Value::String(render_dt(&issue.created_at, timezone)),
    );
    map.insert(
        "updated_at".to_string(),
        serde_json::Value::String(render_dt(&issue.updated_at, timezone)),
    );
    map.insert("created_by".to_string(), opt_uuid(&issue.created_by_id));
    map.insert("updated_by".to_string(), opt_uuid(&issue.updated_by_id));
    map.insert(
        "is_draft".to_string(),
        serde_json::Value::Bool(issue.is_draft),
    );
    map.insert(
        "archived_at".to_string(),
        issue
            .archived_at
            .map(|date| serde_json::Value::String(date.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert("is_synced".to_string(), serde_json::Value::Bool(is_synced));
    serde_json::Value::Object(map)
}

/// Whether the issue is git-synced (`app/serializers/issue.py:63-76`):
/// empty `external_source` short-circuits before any query; otherwise a
/// `GitIssueSync`/`GithubIssueSync` row decides.
async fn is_synced(pool: &PgPool, issue: &IssueRow) -> Result<bool, sqlx::Error> {
    let Some(source) = issue.external_source.as_deref() else {
        return Ok(false);
    };
    if source.is_empty() {
        return Ok(false);
    }
    let git: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM \"git_issue_syncs\" WHERE \"git_issue_syncs\".\"deleted_at\" IS NULL AND \"git_issue_syncs\".\"issue_id\" = $1 LIMIT 1")
            .bind(issue.id)
            .fetch_optional(pool)
            .await?
            .flatten();
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM \"github_issue_syncs\" WHERE \"github_issue_syncs\".\"deleted_at\" IS NULL AND \"github_issue_syncs\".\"issue_id\" = $1 LIMIT 1")
            .bind(issue.id)
            .fetch_optional(pool)
            .await?
            .flatten();
    Ok(github.is_some())
}

// ---------------------------------------------------------------------------
// Tasks: issue_activity publishing through the queue
// ---------------------------------------------------------------------------

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort after commit: without it the response
/// still stands (see the module docs).
async fn enqueue_message(pool: &PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `soft_delete_related_objects.delay("db", "intakeissue", pk, "default")`
/// (`db/mixins.py:77`): positional args, no kwargs.
async fn enqueue_soft_delete(pool: &PgPool, intake_issue_id: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            serde_json::Value::String("db".to_string()),
            serde_json::Value::String("intakeissue".to_string()),
            serde_json::Value::String(intake_issue_id.to_string()),
            serde_json::Value::String("default".to_string()),
        ],
        Default::default(),
    );
    enqueue_message(pool, message).await;
}

// ---------------------------------------------------------------------------
// Bodies and validation
// ---------------------------------------------------------------------------

/// Parse the request body the way DRF does for JSON posts: empty → `{}`;
/// malformed → `ParseError` 400; non-object JSON → the attribute errors
/// the view code hits (500 envelope).
#[allow(clippy::result_large_err)]
fn parse_body(raw: &[u8]) -> Result<serde_json::Value, Response> {
    if raw.is_empty() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    match serde_json::from_slice::<serde_json::Value>(raw) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(_) => Err(Denial::ServerError.into_response()),
        Err(error) => Err(Denial::BadJson(serde_json::json!({
            "detail": format!("JSON parse error - {error}"),
        }))
        .into_response()),
    }
}

/// Python truthiness for JSON values (`request.data...get("name", False)`
/// gates on truthiness: `""`/`null`/`false`/`0`/`[]`/`{}` all deny).
fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => {
            number.as_i64().is_some_and(|n| n != 0)
                || number.as_u64().is_some_and(|n| n != 0)
                || number.as_f64().is_some_and(|n| n != 0.0)
        }
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
    }
}

/// DRF `CharField.to_internal_value` (`trim_whitespace=True`): bools,
/// dicts and lists fail; numbers stringify; strings strip. `None` is
/// rejected upstream (`may not be null`).
fn char_internal(value: &serde_json::Value) -> Result<String, ()> {
    match value {
        serde_json::Value::String(text) => Ok(text.trim().to_string()),
        serde_json::Value::Number(number) => Ok(number.to_string().trim().to_string()),
        serde_json::Value::Bool(_) | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Err(())
        }
        serde_json::Value::Null => Err(()),
    }
}

/// Bind a JSON scalar the way psycopg adapts the Python value: typed
/// binds coerce server-side exactly like inline literals. Containers have
/// no adaptation (Django raises → 500).
#[allow(clippy::result_large_err)]
fn bind_text<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &serde_json::Value,
) -> Result<sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>, Response> {
    match value {
        serde_json::Value::String(text) => Ok(query.bind(text.clone())),
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(query.bind(int))
            } else if let Some(uint) = number.as_u64() {
                Ok(query.bind(uint as i64))
            } else if let Some(float) = number.as_f64() {
                Ok(query.bind(float))
            } else {
                Err(Denial::ServerError.into_response())
            }
        }
        serde_json::Value::Bool(flag) => Ok(query.bind(*flag)),
        _ => Err(Denial::ServerError.into_response()),
    }
}

/// The git-sync lock message (`app/serializers/issue.py:225`).
const SYNC_LOCKED_MESSAGE: &str =
    "This field is synced from a Git provider and is read-only. Unbind the project's repository to edit.";

// ---------------------------------------------------------------------------
// list / create (both collection aliases share the viewset)
// ---------------------------------------------------------------------------

/// `list` (`views/intake.py:56-105`): the `issue_filters` list through
/// the APP twin, `sub_issues_count` annotated, bridge `id` carried on the
/// row but never rendered (undeclared on the app serializer).
async fn collection_list(
    State(state): State<AppState>,
    Path((anchor, intake_id_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let board = match load_board(pool, &anchor).await {
        Ok(board) => board,
        Err(response) => return response,
    };
    let intake_id = match parse_id(&intake_id_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let today = Utc::now().with_timezone(&actor.timezone).date_naive();
    let compiled = match filters::compile(&query, today, 4) {
        Ok(compiled) => compiled,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let extra = if compiled.where_extra.is_empty() {
        None
    } else {
        Some(compiled.where_extra.as_str())
    };
    let mut sql = queries::intake_list_sql(extra);
    if !compiled.joins.is_empty() {
        // The builder owns the FROM/WHERE shape; filter joins splice in
        // ahead of the main WHERE (the last ` WHERE (` in the statement).
        let Some(pos) = sql.rfind(" WHERE (") else {
            return Denial::ServerError.into_response();
        };
        sql.insert_str(pos, &compiled.joins);
    }
    let mut statement = sqlx::query(&sql)
        .bind(intake_id)
        .bind(board.workspace_id)
        .bind(board.project_id);
    for value in &compiled.values {
        statement = match value {
            FilterValue::Uuid(id) => statement.bind(id),
            FilterValue::Text(text) => statement.bind(text),
            FilterValue::Int(number) => statement.bind(number),
        };
    }
    let rows = match statement.fetch_all(pool).await {
        Ok(rows) => rows,
        Err(error) => return db_error(error),
    };
    let mut issues = Vec::with_capacity(rows.len());
    let mut counts = Vec::with_capacity(rows.len());
    for row in &rows {
        match IssueRow::get(row) {
            Ok(issue) => {
                let count: i64 = row.try_get("sub_issues_count").unwrap_or(0);
                issues.push(issue);
                counts.push(count);
            }
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    let issue_ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    let bridges = match fetch_bridges(pool, &issue_ids).await {
        Ok(bridges) => bridges,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut rendered = Vec::with_capacity(issues.len());
    for (issue, count) in issues.into_iter().zip(counts) {
        let bridge_rows = bridges.get(&issue.id).cloned().unwrap_or_default();
        let detail = match load_detail(pool, issue, bridge_rows).await {
            Ok(detail) => detail,
            Err(_) => return Denial::ServerError.into_response(),
        };
        match render_app_detail(&detail, actor.timezone, Some(count)) {
            Ok(value) => rendered.push(value),
            Err(response) => return response,
        }
    }
    raw_json_response(serde_json::Value::Array(rendered).to_string())
}

/// `create` (`views/intake.py:107-173`): name-required and priority
/// gates, triage auto-create, `Issue` + `IntakeIssue` writes, the
/// `issue.activity.created` task, APP detail rendering, 200.
async fn collection_create(
    State(state): State<AppState>,
    Path((anchor, intake_id_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let board = match load_board(pool, &anchor).await {
        Ok(board) => board,
        Err(response) => return response,
    };
    let intake_id = match parse_id(&intake_id_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `request.data.get("issue", {})`: missing → `{}`; a non-dict raises
    // before the name gate (500).
    let issue_value = data.get("issue").cloned().unwrap_or_default();
    let serde_json::Value::Object(issue_data) = &issue_value else {
        return Denial::ServerError.into_response();
    };
    let name_value = issue_data
        .get("name")
        .cloned()
        .unwrap_or(serde_json::Value::Bool(false));
    if !json_truthy(&name_value) {
        return guard_denial(guards::check_intake_create(false, "none"));
    }
    // Validation defaults a missing priority to `"none"` (`:119`), while
    // the insert below falls back to `"low"` (`:149`) — the default-low
    // quirk. An explicit non-string (including null) is not in the
    // allowlist and denies.
    let priority_check = match issue_data.get("priority") {
        None => "none".to_string(),
        Some(serde_json::Value::String(priority)) => priority.clone(),
        Some(_) => {
            return guard_denial(guards::check_intake_create(true, ""));
        }
    };
    if guards::check_intake_create(true, &priority_check).is_err() {
        return guard_denial(guards::check_intake_create(true, &priority_check));
    }
    let priority_insert = match issue_data.get("priority") {
        Some(serde_json::Value::String(priority)) => priority.clone(),
        _ => "low".to_string(),
    };
    // Triage auto-create (`:128-143`): reuse the state, else insert with
    // the `State.save` resequence (`max+15000`, `65000` on empty).
    // The queries-layer lookup selects `states.*`; decoding that row as a
    // scalar fails whenever a triage row exists, so read the `id` column
    // off the row instead (foundation read-only).
    let triage_lookup = queries::triage_lookup_sql();
    let triage_id: Option<Uuid> = match sqlx::query(&triage_lookup)
        .bind(board.project_id)
        .bind(board.workspace_id)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => match row.try_get::<Option<Uuid>, _>("id") {
            Ok(id) => id,
            Err(_) => return Denial::ServerError.into_response(),
        },
        Ok(None) => None,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let now = Utc::now();
    let state_id = match triage_id {
        Some(id) => id,
        None => {
            let max_sequence: Option<f64> = match sqlx::query_scalar(
                "SELECT MAX(sequence) FROM \"states\" WHERE \"states\".\"project_id\" = $1",
            )
            .bind(board.project_id)
            .fetch_one(pool)
            .await
            {
                Ok(max) => max,
                Err(_) => return Denial::ServerError.into_response(),
            };
            let sequence = max_sequence.map(|max| max + 15000.0).unwrap_or(65000.0);
            let new_id = Uuid::new_v4();
            // Full `State.objects.create` row: the queries-layer helper
            // emits only the explicit create kwargs, so the ORM-filled
            // columns (`description=""`, `slug="triage"` from `save()`,
            // `is_triage=False`) are set here (foundation read-only).
            if let Err(error) = sqlx::query(
                "INSERT INTO \"states\" (id, created_at, updated_at, created_by_id, name, description, slug, color, project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES ($1, $2, $3, $4, 'Triage', '', 'triage', '#4E5355', $5, $6, $7, 'triage', false, false)",
            )
            .bind(new_id)
            .bind(now)
            .bind(now)
            .bind(actor.id)
            .bind(board.project_id)
            .bind(board.workspace_id)
            .bind(sequence)
            .execute(pool)
            .await
            {
                return db_error(error);
            }
            new_id
        }
    };
    // `Issue.save` backfills (`db/models/issue.py:305-360`): the project
    // default pod (or NULL), the next sequence, tag-stripped text, and
    // triage sort order — all inside the advisory lock the save takes.
    let issue_id = Uuid::new_v4();
    let sequence_id: i32 = match sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(sequence) FROM \"issue_sequences\" WHERE \"issue_sequences\".\"project_id\" = $1",
    )
    .bind(board.project_id)
    .fetch_one(pool)
    .await
    {
        Ok(max) => max.map(|max| max + 1).unwrap_or(1) as i32,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let pod_id: Option<Uuid> = match sqlx::query_scalar(
        "SELECT id FROM \"pod\" WHERE \"pod\".\"deleted_at\" IS NULL AND \"pod\".\"project_id\" = $1 AND \"pod\".\"is_default\" = TRUE ORDER BY \"pod\".\"created_at\" ASC LIMIT 1",
    )
    .bind(board.project_id)
    .fetch_optional(pool)
    .await
    {
        Ok(pod) => pod,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let sort_order: f64 = match sqlx::query_scalar::<_, Option<f64>>(
        "SELECT MAX(sort_order) FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL AND \"issues\".\"project_id\" = $1 AND \"issues\".\"state_id\" = $2",
    )
    .bind(board.project_id)
    .bind(state_id)
    .fetch_one(pool)
    .await
    {
        Ok(max) => max.map(|max| max + 10000.0).unwrap_or(65535.0),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let description_json = issue_data
        .get("description_json")
        .cloned()
        .unwrap_or_else(super::sanitize::default_description_json);
    // `issue_data.get("description_html", DEFAULT_DESCRIPTION)` (`:148`):
    // an explicit null stores NULL (no serializer validation on create).
    let description_html: Option<String> = match issue_data.get("description_html") {
        None => Some(super::sanitize::DEFAULT_DESCRIPTION_HTML.to_string()),
        Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(html)) => Some(html.clone()),
        Some(_) => return Denial::ServerError.into_response(),
    };
    let description_stripped = description_html
        .as_deref()
        .map(super::sanitize::strip_tags)
        .filter(|stripped| !stripped.is_empty());
    // The advisory lock serializes sequence assignment
    // (`db/models/issue.py:336`); failure degrades to the plain path.
    let lock_key = advisory_key(&board.project_id);
    let _ = sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(pool)
        .await;
    let mut insert = sqlx::query(
        "INSERT INTO \"issues\" (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, NULL, $7, NULL, NULL, $8, $9, $10, $11, NULL, $12, 0, NULL, NULL, $13, $14, NULL, NULL, FALSE, NULL, NULL, NULL, '', '', NULL, $15, NULL)",
    )
    .bind(issue_id)
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(board.project_id)
    .bind(board.workspace_id)
    .bind(state_id);
    insert = match bind_text(insert, &name_value) {
        Ok(query) => query,
        Err(response) => return response,
    };
    insert = insert
        .bind(description_json)
        .bind(description_html)
        .bind(description_stripped)
        .bind(priority_insert)
        .bind(sequence_id)
        .bind(sort_order)
        .bind(pod_id);
    if let Err(error) = insert.execute(pool).await {
        return db_error(error);
    }
    if let Err(error) = sqlx::query(
        "INSERT INTO \"issue_sequences\" (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, FALSE)",
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(board.project_id)
    .bind(board.workspace_id)
    .bind(issue_id)
    .bind(i64::from(sequence_id))
    .execute(pool)
    .await
    {
        return db_error(error);
    }
    // `issue_activity.delay` (`:155-163`): `requested_data` is the full
    // `request.data` dump, `current_instance` is `None`.
    let epoch = Utc::now().timestamp();
    let publish = pidash_jobs::space::intake_created(
        super::python_dumps(&data),
        actor.id.to_string(),
        issue_id.to_string(),
        board.project_id.to_string(),
        epoch,
    );
    enqueue_message(pool, publish.message()).await;
    // The bridge takes `intake_id` from the URL (`:165-170`); the
    // workspace is backfilled from the project (`:165`).
    let bridge_id = Uuid::new_v4();
    if let Err(error) = sqlx::query(
        "INSERT INTO \"intake_issues\" (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, intake_id, issue_id, status, snoozed_till, duplicate_to_id, source, source_email, external_source, external_id, extra) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}')",
    )
    .bind(bridge_id)
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(board.project_id)
    .bind(board.workspace_id)
    .bind(intake_id)
    .bind(issue_id)
    .execute(pool)
    .await
    {
        return db_error(error);
    }
    let issue = match fetch_issue(pool, &issue_id, &board).await {
        Ok(Some(issue)) => issue,
        Ok(None) => return Denial::ServerError.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let bridges = match fetch_bridges(pool, &[issue_id]).await {
        Ok(bridges) => bridges,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let detail = match load_detail(
        pool,
        issue,
        bridges.get(&issue_id).cloned().unwrap_or_default(),
    )
    .await
    {
        Ok(detail) => detail,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match render_app_detail(&detail, actor.timezone, None) {
        Ok(value) => raw_json_response(value.to_string()),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// retrieve / partial_update / destroy
// ---------------------------------------------------------------------------

/// The scoped bridge lookup the detail actions share
/// (`:199-203,:250-254,:264-268`): `IntakeIssue.objects.get(id=pk,
/// intake_id, workspace_id, project_id)`; a miss raises → 404. Returns
/// the bridge id, its creator, and the linked issue id.
async fn fetch_bridge(
    pool: &PgPool,
    bridge_id: &Uuid,
    intake_id: &Uuid,
    board: &Board,
) -> Result<Option<(Uuid, Option<Uuid>, Uuid)>, sqlx::Error> {
    sqlx::query(
        "SELECT \"intake_issues\".\"id\", \"intake_issues\".\"created_by_id\", \"intake_issues\".\"issue_id\" FROM \"intake_issues\" WHERE (\"intake_issues\".\"deleted_at\" IS NULL AND \"intake_issues\".\"id\" = $1 AND \"intake_issues\".\"intake_id\" = $2 AND \"intake_issues\".\"workspace_id\" = $3 AND \"intake_issues\".\"project_id\" = $4)",
    )
    .bind(bridge_id)
    .bind(intake_id)
    .bind(board.workspace_id)
    .bind(board.project_id)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        Ok((
            row.try_get("id")?,
            row.try_get("created_by_id")?,
            row.try_get("issue_id")?,
        ))
    })
    .transpose()
}

/// `retrieve` (`views/intake.py:236-256`): scoped lookups, APP detail
/// rendering (no `sub_issues_count`), 200.
async fn detail_retrieve(
    State(state): State<AppState>,
    Path((anchor, intake_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let board = match load_board(pool, &anchor).await {
        Ok(board) => board,
        Err(response) => return response,
    };
    let intake_id = match parse_id(&intake_id_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pk = match parse_id(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let (_, _, issue_id) = match fetch_bridge(pool, &pk, &intake_id, &board).await {
        Ok(Some(triple)) => triple,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let issue = match fetch_issue(pool, &issue_id, &board).await {
        Ok(Some(issue)) => issue,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let bridges = match fetch_bridges(pool, &[issue_id]).await {
        Ok(bridges) => bridges,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let detail = match load_detail(
        pool,
        issue,
        bridges.get(&issue_id).cloned().unwrap_or_default(),
    )
    .await
    {
        Ok(detail) => detail,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match render_app_detail(&detail, actor.timezone, None) {
        Ok(value) => raw_json_response(value.to_string()),
        Err(response) => response,
    }
}

/// `partial_update` (`views/intake.py:175-234`): creator ownership, the
/// APP `IssueCreateSerializer` partial path over the 3-key subset, the
/// `issue.activity.updated` task, APP `IssueCreate` rendering, 200.
async fn detail_partial_update(
    State(state): State<AppState>,
    Path((anchor, intake_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let board = match load_board(pool, &anchor).await {
        Ok(board) => board,
        Err(response) => return response,
    };
    let intake_id = match parse_id(&intake_id_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pk = match parse_id(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    // `request.data.pop("issue", False)`: missing/non-dict raises (500).
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let serde_json::Value::Object(mut top) = data else {
        return Denial::ServerError.into_response();
    };
    let issue_value = top
        .remove("issue")
        .unwrap_or(serde_json::Value::Bool(false));
    let serde_json::Value::Object(input) = &issue_value else {
        return Denial::ServerError.into_response();
    };
    let (_, created_by, issue_id) = match fetch_bridge(pool, &pk, &intake_id, &board).await {
        Ok(Some(triple)) => triple,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    if created_by.as_ref() != Some(&actor.id) {
        return guard_denial(guards::check_intake_edit_owner(
            &created_by.map(|id| id.to_string()).unwrap_or_default(),
            &actor.id.to_string(),
        ));
    }
    let stored = match fetch_issue(pool, &issue_id, &board).await {
        Ok(Some(issue)) => issue,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    // The 3-key subset with stored fallbacks (`:205-209`).
    let name_raw = input
        .get("name")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::String(stored.name.clone()));
    let html_raw = input
        .get("description_html")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::String(stored.description_html.clone()));
    let json_raw = input
        .get("description_json")
        .cloned()
        .unwrap_or_else(|| stored.description_json.clone());
    // Field-level validation first (DRF runs it before `validate()`).
    let mut field_errors = serde_json::Map::new();
    let name = match char_internal(&name_raw) {
        Ok(name) if name.is_empty() => {
            field_errors.insert(
                "name".to_string(),
                serde_json::json!(["This field may not be blank."]),
            );
            String::new()
        }
        Ok(name) if name.chars().count() > 255 => {
            field_errors.insert(
                "name".to_string(),
                serde_json::json!(["Ensure this field has no more than 255 characters."]),
            );
            String::new()
        }
        Ok(name) => name,
        Err(()) => {
            let message = if name_raw.is_null() {
                "This field may not be null."
            } else {
                "Not a valid string."
            };
            field_errors.insert("name".to_string(), serde_json::json!([message]));
            String::new()
        }
    };
    let description_html = match char_internal(&html_raw) {
        Ok(html) => Some(html),
        Err(()) => {
            let message = if html_raw.is_null() {
                "This field may not be null."
            } else {
                "Not a valid string."
            };
            field_errors.insert("description_html".to_string(), serde_json::json!([message]));
            None
        }
    };
    if json_raw.is_null() {
        field_errors.insert(
            "description_json".to_string(),
            serde_json::json!(["This field may not be null."]),
        );
    }
    if !field_errors.is_empty() {
        return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
    }
    let description_html = description_html.expect("validated");
    // `validate()`: the sync lock over the changed locked fields
    // (`app/serializers/issue.py:241-250`; only `external_source`-set
    // rows ever reach the queries), then the HTML sanitize.
    let synced = match is_synced(pool, &stored).await {
        Ok(synced) => synced,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if synced {
        let changed = (name != stored.name)
            || (description_html != stored.description_html)
            || (json_raw != stored.description_json);
        if changed {
            // First changed locked field in `validate()` iteration
            // order wins; all three subset keys are locked.
            let field = if name != stored.name {
                "name"
            } else if description_html != stored.description_html {
                "description_html"
            } else {
                "description_json"
            };
            return Denial::BadJson(serde_json::json!({
                field: [SYNC_LOCKED_MESSAGE],
            }))
            .into_response();
        }
    }
    let description_html = match super::sanitize::sanitize_html(&description_html) {
        super::sanitize::Sanitize::Clean(html) => html,
        super::sanitize::Sanitize::Invalid => {
            return Denial::BadJson(serde_json::json!({
                "error": ["html content is not valid"],
            }))
            .into_response();
        }
    };
    // The activity fires before `save()` (`:223-232`): snapshot and
    // payload render from the pre-save row.
    let bridges = match fetch_bridges(pool, &[issue_id]).await {
        Ok(bridges) => bridges,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let pre_save = match load_detail(
        pool,
        stored,
        bridges.get(&issue_id).cloned().unwrap_or_default(),
    )
    .await
    {
        Ok(detail) => detail,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let current_instance =
        super::python_dumps(&render_issue_snapshot(&pre_save, actor.timezone, synced));
    let mut subset = serde_json::Map::with_capacity(3);
    subset.insert("name".to_string(), serde_json::Value::String(name.clone()));
    subset.insert(
        "description_html".to_string(),
        serde_json::Value::String(description_html.clone()),
    );
    subset.insert("description_json".to_string(), json_raw.clone());
    let epoch = Utc::now().timestamp();
    let publish = pidash_jobs::space::intake_updated(
        super::python_dumps(&serde_json::Value::Object(subset)),
        actor.id.to_string(),
        issue_id.to_string(),
        board.project_id.to_string(),
        current_instance,
        epoch,
    );
    enqueue_message(pool, publish.message()).await;
    // `update()`: the 3 attrs plus the save backfills (`updated_at`,
    // `updated_by`, recomputed stripped text, `completed_at`).
    let now = Utc::now();
    let description_stripped = if description_html.is_empty() {
        None
    } else {
        let stripped = super::sanitize::strip_tags(&description_html);
        if stripped.is_empty() {
            None
        } else {
            Some(stripped)
        }
    };
    // `Issue.save`: `completed_at = now` only when the state group is
    // `completed`, else `None` (the state never changes on this path).
    let completed_at = match pre_save.issue.state_id {
        Some(state_id) => match fetch_state(pool, &state_id).await {
            Ok(state)
                if state
                    .as_ref()
                    .is_some_and(|state| state.group == "completed") =>
            {
                Some(now)
            }
            Ok(_) => None,
            Err(_) => return Denial::ServerError.into_response(),
        },
        None => None,
    };
    if let Err(error) = sqlx::query(
        "UPDATE \"issues\" SET name = $1, description_html = $2, description_json = $3, description_stripped = $4, updated_at = $5, updated_by_id = $6, completed_at = $7 WHERE \"issues\".\"id\" = $8",
    )
    .bind(name)
    .bind(description_html)
    .bind(json_raw)
    .bind(description_stripped)
    .bind(now)
    .bind(actor.id)
    .bind(completed_at)
    .bind(issue_id)
    .execute(pool)
    .await
    {
        return db_error(error);
    }
    let issue = match fetch_issue(pool, &issue_id, &board).await {
        Ok(Some(issue)) => issue,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let bridges = match fetch_bridges(pool, &[issue_id]).await {
        Ok(bridges) => bridges,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let detail = match load_detail(
        pool,
        issue,
        bridges.get(&issue_id).cloned().unwrap_or_default(),
    )
    .await
    {
        Ok(detail) => detail,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match render_app_issue_create(&detail, actor.timezone) {
        Ok(value) => raw_json_response(value.to_string()),
        Err(response) => response,
    }
}

/// `destroy` (`views/intake.py:258-280`): creator ownership, SOFT delete
/// of the intake row only (verified live — the queries layer's hard
/// `DELETE` text does not match `SoftDeleteModel.delete()`), 204.
async fn detail_destroy(
    State(state): State<AppState>,
    Path((anchor, intake_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let board = match load_board(pool, &anchor).await {
        Ok(board) => board,
        Err(response) => return response,
    };
    let intake_id = match parse_id(&intake_id_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let pk = match parse_id(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let (_, created_by, _) = match fetch_bridge(pool, &pk, &intake_id, &board).await {
        Ok(Some(triple)) => triple,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    if created_by.as_ref() != Some(&actor.id) {
        return guard_denial(guards::check_intake_delete_owner(
            &created_by.map(|id| id.to_string()).unwrap_or_default(),
            &actor.id.to_string(),
        ));
    }
    // `intake_issue.delete()` (`db/mixins.py:72-77`): `deleted_at` plus a
    // full `save()` (`updated_at`, CRUM `updated_by`), then the related
    // objects task.
    let now = Utc::now();
    if let Err(error) = sqlx::query(
        "UPDATE \"intake_issues\" SET deleted_at = $1, updated_at = $2, updated_by_id = $3 WHERE \"intake_issues\".\"id\" = $4",
    )
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(pk)
    .execute(pool)
    .await
    {
        return db_error(error);
    }
    enqueue_soft_delete(pool, &pk).await;
    StatusCode::NO_CONTENT.into_response()
}

/// `convert_uuid_to_integer` (`db/models/issue.py:22-26`): first 8 bytes
/// of `sha256(str(project_id))` as a signed big-endian integer — the
/// advisory-lock key `Issue.save` takes.
fn advisory_key(project_id: &Uuid) -> i64 {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(project_id.to_string().as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("eight bytes"))
}
