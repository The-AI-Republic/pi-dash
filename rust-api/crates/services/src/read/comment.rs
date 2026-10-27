//! App comment rendering (`app/serializers/issue.py`).
//!
//! Verbatim move of the `space::social` comment-render closure (OVL-05,
//! PIDASHCONV-1543): [`render_comment`] plus the row decoders, the
//! `row_to_json` fetchers and the leaf renderers it calls. The space
//! handlers call [`render_comment`]; the file keeps its own copies of
//! the shared fetch/getter helpers for its other (non-comment) callers.

use chrono::{DateTime, TimeZone};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{DbError, Error};

// ---------------------------------------------------------------------------
// row_to_json fetching + field access (the project_meta shape)
// ---------------------------------------------------------------------------

/// One bound `$N` parameter. The comment closure only binds UUID
/// columns (the file keeps the `Text`/`Null` arms for its other
/// callers).
#[derive(Debug, Clone, Copy, PartialEq)]
enum SqlParam {
    Uuid(Uuid),
}

/// Parse a UUID-typed bind. Callers pass path segments Django's `<uuid:>`
/// converter already validated, or ids read back from the database —
/// anything else is a 500 like Python's `ValidationError` → fallback.
fn uuid_param(raw: &str) -> Result<SqlParam, Error> {
    Uuid::parse_str(raw)
        .map(SqlParam::Uuid)
        .map_err(|_| Error::Failed("uuid-param"))
}

/// Fetch zero or one row of `inner` as a JSON object.
async fn fetch_optional_object(
    pool: &PgPool,
    inner: &str,
    params: &[SqlParam],
    site: &'static str,
) -> Result<Option<Value>, Error> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let row: Option<sqlx::postgres::PgRow> = query
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new(site, error))?;
    match row {
        None => Ok(None),
        Some(row) => {
            let text: String = row
                .try_get("__row")
                .map_err(|_| Error::Failed("row-to-json"))?;
            let value: Value =
                serde_json::from_str(&text).map_err(|_| Error::Failed("row-to-json"))?;
            match value {
                Value::Object(_) => Ok(Some(value)),
                _ => Err(Error::Failed("row-to-json")),
            }
        }
    }
}

/// Fetch every row of `inner` as JSON objects.
async fn fetch_all_objects(
    pool: &PgPool,
    inner: &str,
    params: &[SqlParam],
    site: &'static str,
) -> Result<Vec<Value>, Error> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let rows: Vec<sqlx::postgres::PgRow> = query
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new(site, error))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row
            .try_get("__row")
            .map_err(|_| Error::Failed("row-to-json"))?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Error::Failed("row-to-json"))?;
        match value {
            Value::Object(_) => out.push(value),
            _ => return Err(Error::Failed("row-to-json")),
        }
    }
    Ok(out)
}

fn obj(value: &Value) -> Result<&serde_json::Map<String, Value>, Error> {
    value.as_object().ok_or(Error::Failed("row-object"))
}

fn req_str(obj: &serde_json::Map<String, Value>, key: &'static str) -> Result<String, Error> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(Error::Failed(key)),
    }
}

fn opt_str(
    obj: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<String>, Error> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(Error::Failed(key)),
    }
}

fn req_bool(obj: &serde_json::Map<String, Value>, key: &'static str) -> Result<bool, Error> {
    match obj.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(Error::Failed(key)),
    }
}

fn req_json(obj: &serde_json::Map<String, Value>, key: &'static str) -> Result<Value, Error> {
    obj.get(key).cloned().ok_or(Error::Failed(key))
}

/// Render a `row_to_json` timestamptz value exactly like DRF `DateTimeField`
/// with the default `iso-8601` format: ISO-8601 in the request's zone with
/// `+00:00` rewritten to `Z` (serializer kernel rule).
fn render_dt(raw: &str, tz: &chrono_tz::Tz) -> Result<String, Error> {
    let dt = DateTime::parse_from_rfc3339(raw).map_err(|_| Error::Failed("render-dt"))?;
    Ok(render_datetime_in(&dt, tz))
}

fn render_dt_opt(raw: Option<String>, tz: &chrono_tz::Tz) -> Result<Option<String>, Error> {
    raw.map(|text| render_dt(&text, tz)).transpose()
}

/// Render an aware datetime through a zone: ISO-8601 with `+00:00`
/// rewritten to `Z` (the `pidash-api` serializer kernel, moved with the
/// comment render).
fn render_datetime_in<TzFrom: TimeZone, TzTo: TimeZone>(dt: &DateTime<TzFrom>, tz: &TzTo) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    let local = dt.with_timezone(tz);
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &local.format("%Y-%m-%dT%H:%M:%S").to_string(),
        local.timestamp_subsec_nanos(),
        &suffix,
    )
}

fn render_utc_parts(base: &str, nanos: u32, suffix: &str) -> String {
    // DRF `DateTimeField` renders Python datetimes, which carry only
    // microseconds: no fraction when micros == 0, exactly 6 digits
    // otherwise. Truncate sub-microsecond nanos (e.g. `Utc::now()`
    // stamps) so HTTP output never carries 9 digits (PIDASHCONV-867).
    let micros = nanos / 1000;
    if micros == 0 {
        format!("{base}{suffix}")
    } else {
        format!("{base}.{:06}{suffix}", micros)
    }
}

// ---------------------------------------------------------------------------
// User / asset lookups behind the nests
// ---------------------------------------------------------------------------

/// `SELECT` for the user row behind `actor_detail` nests. Unlike every
/// other forward FK here, this carries no soft-deletion guard: `User`
/// (`db/models/user.py:56`, `AbstractBaseUser` + `PermissionsMixin`) has no
/// `deleted_at` column, so Django emits no filter for it.
fn user_detail_sql() -> String {
    "SELECT \"users\".\"id\", \"users\".\"first_name\", \"users\".\"last_name\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\", \"users\".\"is_bot\", \"users\".\"display_name\" FROM \"users\" WHERE (\"users\".\"id\" = $1)".to_string()
}

/// `SELECT` for the `display_name` behind reaction/vote rows
/// (`source="actor.display_name"`). Same unguarded `users` read as
/// [`user_detail_sql`]: no `deleted_at` column exists to filter on.
fn display_name_sql() -> String {
    "SELECT display_name FROM users WHERE id = $1".to_string()
}

/// `SELECT` for the logo/cover asset row behind `*_url` properties (the
/// project_meta shape).
fn logo_asset_sql() -> String {
    "SELECT \"a\".\"id\", \"a\".\"entity_type\", \"a\".\"workspace_id\", \"a\".\"project_id\", \"a\".\"issue_id\", \"w\".\"slug\" AS \"workspace_slug\" FROM \"file_assets\" AS \"a\" LEFT OUTER JOIN \"workspaces\" AS \"w\" ON (\"a\".\"workspace_id\" = \"w\".\"id\") WHERE (\"a\".\"deleted_at\" IS NULL AND \"a\".\"id\" = $1)".to_string()
}

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`): static-asset
/// branches render `/api/assets/v2/static/<id>/`; attachment/description
/// branches interpolate ids; anything else renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => None,
    }
}

/// Resolve a logo/cover/avatar URL the way the model properties do: the
/// asset's `asset_url` when the FK is set (even when that computes to
/// `None`), else the raw string column, else `None`. Empty strings are
/// falsy in Python (`if self.avatar`), so they render `None` too.
async fn logo_or_cover_url(
    pool: &PgPool,
    raw: Option<String>,
    asset_id: Option<String>,
) -> Result<Option<String>, Error> {
    let Some(asset_id) = asset_id else {
        return Ok(raw.filter(|text| !text.is_empty()));
    };
    let row = fetch_optional_object(
        pool,
        &logo_asset_sql(),
        &[uuid_param(&asset_id)?],
        "logo-asset",
    )
    .await?
    .ok_or(Error::Failed("logo-asset"))?;
    let o = obj(&row)?;
    let entity_type = opt_str(o, "entity_type")?;
    let workspace_slug = opt_str(o, "workspace_slug")?;
    let project_id = opt_str(o, "project_id")?;
    let issue_id = opt_str(o, "issue_id")?;
    let id = req_str(o, "id")?;
    Ok(asset_url(
        &id,
        entity_type.as_deref(),
        workspace_slug.as_deref(),
        project_id.as_deref(),
        issue_id.as_deref(),
    ))
}

// ---------------------------------------------------------------------------
// Comment shapes (app IssueCommentSerializer, issue.py:948-989)
// ---------------------------------------------------------------------------

/// App `UserLiteSerializer` (`app/serializers/user.py:141-154`), in
/// `Meta.fields` order.
#[derive(Debug, Serialize, Clone)]
pub struct ActorDetailView {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: Option<String>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub display_name: String,
}

/// Render one user row as `actor_detail`. A dangling FK is a 500 like
/// Python's `DoesNotExist` → fallback envelope.
pub async fn render_actor_detail(pool: &PgPool, user_id: &str) -> Result<ActorDetailView, Error> {
    let row = fetch_optional_object(
        pool,
        &user_detail_sql(),
        &[uuid_param(user_id)?],
        "actor-detail",
    )
    .await?
    .ok_or(Error::Failed("actor-detail"))?;
    let o = obj(&row)?;
    let avatar = opt_str(o, "avatar")?;
    let avatar_url =
        logo_or_cover_url(pool, avatar.clone(), opt_str(o, "avatar_asset_id")?).await?;
    Ok(ActorDetailView {
        id: req_str(o, "id")?,
        first_name: opt_str(o, "first_name")?.unwrap_or_default(),
        last_name: opt_str(o, "last_name")?.unwrap_or_default(),
        // `avatar` is the raw column: DRF renders `""` verbatim. Only the
        // `avatar_url` property folds falsy values to `None`
        // (`db/models/user.py:143-151`, via `logo_or_cover_url` above).
        avatar,
        avatar_url,
        is_bot: req_bool(o, "is_bot")?,
        display_name: opt_str(o, "display_name")?.unwrap_or_default(),
    })
}

/// App `IssueFlatSerializer` for `issue_detail`
/// (`app/serializers/issue.py:105-123`), in `Meta.fields` order.
#[derive(Debug, Serialize, Clone)]
pub struct IssueFlatView {
    pub id: String,
    pub name: String,
    pub description_json: Value,
    pub description_html: Option<String>,
    pub priority: String,
    pub complexity_score: Value,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub sequence_id: i64,
    pub sort_order: Value,
    pub is_draft: bool,
}

/// App `ProjectLiteSerializer` for `project_detail`
/// (`app/serializers/project.py:120-133`), in `Meta.fields` order.
#[derive(Debug, Serialize, Clone)]
pub struct ProjectLiteView {
    pub id: String,
    pub identifier: String,
    pub name: String,
    pub cover_image: Option<String>,
    pub cover_image_url: Option<String>,
    pub logo_props: Value,
    pub description: Option<String>,
    pub is_default: bool,
}

/// App `WorkspaceLiteSerializer` for `workspace_detail`
/// (`app/serializers/workspace.py:79-83`), in `Meta.fields` order.
#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceLiteView {
    pub name: String,
    pub slug: String,
    pub id: String,
    pub logo_url: Option<String>,
}

/// App `CommentReactionSerializer` (`app/serializers/issue.py:917-937`),
/// in `Meta.fields` order. `display_name` reads `actor.display_name`.
#[derive(Debug, Serialize, Clone)]
pub struct CommentReactionView {
    pub id: String,
    pub actor: String,
    pub comment: String,
    pub reaction: String,
    pub display_name: String,
    pub deleted_at: Option<String>,
    pub workspace: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
}

/// App `IssueCommentSerializer` (`app/serializers/issue.py:948-972`):
/// `Meta.fields = "__all__"`, which DRF orders as pk, declared fields in
/// creation order, concrete model fields, then forward relations
/// (`get_default_field_names`). `is_member` is only present on
/// queryset-annotated rows (list/retrieve), never on create/update
/// responses.
#[derive(Debug, Serialize, Clone)]
pub struct CommentView {
    pub id: String,
    pub actor_detail: Option<ActorDetailView>,
    pub issue_detail: IssueFlatView,
    pub project_detail: ProjectLiteView,
    pub workspace_detail: WorkspaceLiteView,
    pub comment_reactions: Vec<CommentReactionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_member: Option<bool>,
    pub is_synced: bool,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub comment_stripped: Option<String>,
    pub comment_json: Value,
    pub comment_html: Option<String>,
    pub attachments: Value,
    pub labels: Value,
    pub access: Option<String>,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub speaker_type: Option<String>,
    pub speaker_label: Option<String>,
    pub speaker_agent_run_id: Option<String>,
    pub edited_at: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub project: Option<String>,
    pub workspace: Option<String>,
    pub description: Option<String>,
    pub issue: Option<String>,
    pub actor: Option<String>,
    pub parent: Option<String>,
}

/// The `issue_comments` columns one render needs, read from the
/// `row_to_json` object. `created_by`/`updated_by` arrive as
/// `created_by_id` from `SELECT *` (the column names); the `or_else`
/// fallback covers aliased selects.
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub deleted_at: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub comment_stripped: Option<String>,
    pub comment_json: Value,
    pub comment_html: Option<String>,
    pub description_id: Option<String>,
    pub attachments: Value,
    pub labels: Value,
    pub issue_id: Option<String>,
    pub actor_id: Option<String>,
    pub access: Option<String>,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub speaker_type: Option<String>,
    pub speaker_label: Option<String>,
    pub speaker_agent_run_id: Option<String>,
    pub edited_at: Option<String>,
    pub parent_id: Option<String>,
    pub is_member: Option<bool>,
}

impl CommentRow {
    /// Decode a `row_to_json` comment object.
    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let o = obj(value)?;
        let is_member = match o.get("is_member") {
            None => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Null) => None,
            _ => return Err(Error::Failed("is-member")),
        };
        Ok(Self {
            id: req_str(o, "id")?,
            created_at: req_str(o, "created_at")?,
            updated_at: req_str(o, "updated_at")?,
            created_by: opt_str(o, "created_by_id")?
                .or_else(|| opt_str(o, "created_by").unwrap_or(None)),
            updated_by: opt_str(o, "updated_by_id")?
                .or_else(|| opt_str(o, "updated_by").unwrap_or(None)),
            deleted_at: opt_str(o, "deleted_at")?,
            project_id: opt_str(o, "project_id")?,
            workspace_id: opt_str(o, "workspace_id")?,
            comment_stripped: opt_str(o, "comment_stripped")?,
            comment_json: o.get("comment_json").cloned().unwrap_or(Value::Null),
            comment_html: opt_str(o, "comment_html")?,
            description_id: opt_str(o, "description_id")?,
            attachments: o.get("attachments").cloned().unwrap_or(Value::Null),
            labels: o.get("labels").cloned().unwrap_or(Value::Null),
            issue_id: opt_str(o, "issue_id")?,
            actor_id: opt_str(o, "actor_id")?,
            access: opt_str(o, "access")?,
            external_source: opt_str(o, "external_source")?,
            external_id: opt_str(o, "external_id")?,
            speaker_type: opt_str(o, "speaker_type")?,
            speaker_label: opt_str(o, "speaker_label")?,
            speaker_agent_run_id: opt_str(o, "speaker_agent_run_id")?,
            edited_at: opt_str(o, "edited_at")?,
            parent_id: opt_str(o, "parent_id")?,
            is_member,
        })
    }
}

/// Render one comment row with its nests. `include_member` is true for
/// list/retrieve (annotated queryset) and false for create/update (plain
/// instance: the key is skipped, like DRF's `SkipField`).
pub async fn render_comment(
    pool: &PgPool,
    row: &CommentRow,
    tz: &chrono_tz::Tz,
    include_member: bool,
) -> Result<CommentView, Error> {
    let issue_detail = render_issue_flat(pool, row.issue_id.as_deref()).await?;
    let project_detail = render_project_lite(pool, row.project_id.as_deref()).await?;
    let workspace_detail = render_workspace_lite(pool, row.workspace_id.as_deref()).await?;
    let comment_reactions = render_comment_reactions(pool, &row.id, tz).await?;
    let actor_detail = match row.actor_id.as_deref() {
        None => None,
        Some(actor) => Some(render_actor_detail(pool, actor).await?),
    };
    let is_synced = comment_is_synced(pool, row.external_source.as_deref(), &row.id).await?;
    Ok(CommentView {
        id: row.id.clone(),
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        comment_reactions,
        is_member: if include_member { row.is_member } else { None },
        is_synced,
        created_at: render_dt(&row.created_at, tz)?,
        updated_at: render_dt(&row.updated_at, tz)?,
        created_by: row.created_by.clone(),
        updated_by: row.updated_by.clone(),
        deleted_at: render_dt_opt(row.deleted_at.clone(), tz)?,
        project: row.project_id.clone(),
        workspace: row.workspace_id.clone(),
        comment_stripped: row.comment_stripped.clone(),
        comment_json: row.comment_json.clone(),
        comment_html: row.comment_html.clone(),
        description: row.description_id.clone(),
        attachments: row.attachments.clone(),
        labels: row.labels.clone(),
        issue: row.issue_id.clone(),
        actor: row.actor_id.clone(),
        access: row.access.clone(),
        external_source: row.external_source.clone(),
        external_id: row.external_id.clone(),
        speaker_type: row.speaker_type.clone(),
        speaker_label: row.speaker_label.clone(),
        speaker_agent_run_id: row.speaker_agent_run_id.clone(),
        edited_at: render_dt_opt(row.edited_at.clone(), tz)?,
        parent: row.parent_id.clone(),
    })
}

/// `issue_detail`: the flat leaf over the comment's issue FK. The read
/// mirrors `_base_manager` (no `deleted_at` filter), so a soft-deleted
/// issue still renders; only a genuinely missing row is a 500 like
/// Python's `DoesNotExist` → fallback envelope (PIDASHCONV-1260).
async fn render_issue_flat(pool: &PgPool, issue_id: Option<&str>) -> Result<IssueFlatView, Error> {
    let Some(issue_id) = issue_id else {
        return Err(Error::Failed("issue-detail"));
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"is_draft\" FROM \"issues\" WHERE (\"issues\".\"id\" = $1)",
        &[uuid_param(issue_id)?],
        "issue-detail",
    )
    .await?
    .ok_or(Error::Failed("issue-detail"))?;
    let o = obj(&row)?;
    let sequence_id = match o.get("sequence_id") {
        Some(Value::Number(n)) => n.as_i64().ok_or(Error::Failed("sequence-id"))?,
        _ => return Err(Error::Failed("sequence-id")),
    };
    // DRF `FloatField` renders `65535.0`, never Postgres `float8out`'s
    // `65535` (the project_meta kernel rule).
    let sort_order = match o.get("sort_order") {
        Some(Value::Number(n)) => {
            let float = n.as_f64().ok_or(Error::Failed("sort-order"))?;
            serde_json::Number::from_f64(float)
                .map(Value::Number)
                .ok_or(Error::Failed("sort-order"))?
        }
        _ => return Err(Error::Failed("sort-order")),
    };
    Ok(IssueFlatView {
        id: req_str(o, "id")?,
        name: req_str(o, "name")?,
        description_json: req_json(o, "description_json")?,
        description_html: opt_str(o, "description_html")?,
        priority: req_str(o, "priority")?,
        complexity_score: req_json(o, "complexity_score")?,
        start_date: opt_str(o, "start_date")?,
        target_date: opt_str(o, "target_date")?,
        sequence_id,
        sort_order,
        is_draft: req_bool(o, "is_draft")?,
    })
}

/// `project_detail`: the app lite leaf over the comment's project FK.
/// Unfiltered `_base_manager` read: a soft-deleted project still renders
/// (PIDASHCONV-1260).
async fn render_project_lite(
    pool: &PgPool,
    project_id: Option<&str>,
) -> Result<ProjectLiteView, Error> {
    let Some(project_id) = project_id else {
        return Err(Error::Failed("project-detail"));
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"projects\".\"id\", \"projects\".\"identifier\", \"projects\".\"name\", \"projects\".\"cover_image\", \"projects\".\"cover_image_asset_id\", \"projects\".\"logo_props\", \"projects\".\"description\", \"projects\".\"is_default\" FROM \"projects\" WHERE (\"projects\".\"id\" = $1)",
        &[uuid_param(project_id)?],
        "project-detail",
    )
    .await?
    .ok_or(Error::Failed("project-detail"))?;
    let o = obj(&row)?;
    let cover_image_url = logo_or_cover_url(
        pool,
        opt_str(o, "cover_image")?,
        opt_str(o, "cover_image_asset_id")?,
    )
    .await?;
    Ok(ProjectLiteView {
        id: req_str(o, "id")?,
        identifier: req_str(o, "identifier")?,
        name: req_str(o, "name")?,
        cover_image: opt_str(o, "cover_image")?,
        cover_image_url,
        logo_props: req_json(o, "logo_props")?,
        description: opt_str(o, "description")?,
        is_default: req_bool(o, "is_default")?,
    })
}

/// `workspace_detail`: the app lite leaf over the comment's workspace FK.
/// Unfiltered `_base_manager` read: a soft-deleted workspace still renders
/// (PIDASHCONV-1260).
async fn render_workspace_lite(
    pool: &PgPool,
    workspace_id: Option<&str>,
) -> Result<WorkspaceLiteView, Error> {
    let Some(workspace_id) = workspace_id else {
        return Err(Error::Failed("workspace-detail"));
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"workspaces\".\"id\", \"workspaces\".\"name\", \"workspaces\".\"slug\", \"workspaces\".\"logo\", \"workspaces\".\"logo_asset_id\" FROM \"workspaces\" WHERE (\"workspaces\".\"id\" = $1)",
        &[uuid_param(workspace_id)?],
        "workspace-detail",
    )
    .await?
    .ok_or(Error::Failed("workspace-detail"))?;
    let o = obj(&row)?;
    let logo_url =
        logo_or_cover_url(pool, opt_str(o, "logo")?, opt_str(o, "logo_asset_id")?).await?;
    Ok(WorkspaceLiteView {
        name: req_str(o, "name")?,
        slug: req_str(o, "slug")?,
        id: req_str(o, "id")?,
        logo_url,
    })
}

/// `comment_reactions`: the live reaction rows for one comment, newest
/// first (model `ordering = ("-created_at",)`).
async fn render_comment_reactions(
    pool: &PgPool,
    comment_id: &str,
    tz: &chrono_tz::Tz,
) -> Result<Vec<CommentReactionView>, Error> {
    let rows = fetch_all_objects(
        pool,
        "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"comment_id\" = $1) ORDER BY \"comment_reactions\".\"created_at\" DESC",
        &[uuid_param(comment_id)?],
        "comment-reactions",
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(render_comment_reaction_row(pool, row, tz).await?);
    }
    Ok(out)
}

pub async fn render_comment_reaction_row(
    pool: &PgPool,
    row: &Value,
    tz: &chrono_tz::Tz,
) -> Result<CommentReactionView, Error> {
    let o = obj(row)?;
    let actor_id = req_str(o, "actor_id")?;
    let display_name = fetch_display_name(pool, &actor_id).await?;
    Ok(CommentReactionView {
        id: req_str(o, "id")?,
        actor: actor_id,
        comment: req_str(o, "comment_id")?,
        reaction: req_str(o, "reaction")?,
        display_name,
        deleted_at: render_dt_opt(opt_str(o, "deleted_at")?, tz)?,
        workspace: opt_str(o, "workspace_id")?,
        project: opt_str(o, "project_id")?,
        created_at: render_dt(&req_str(o, "created_at")?, tz)?,
        updated_at: render_dt(&req_str(o, "updated_at")?, tz)?,
        created_by: opt_str(o, "created_by_id")?
            .or_else(|| opt_str(o, "created_by").unwrap_or(None)),
        updated_by: opt_str(o, "updated_by_id")?
            .or_else(|| opt_str(o, "updated_by").unwrap_or(None)),
    })
}

/// `display_name` for reaction rows (`source="actor.display_name"`).
async fn fetch_display_name(pool: &PgPool, user_id: &str) -> Result<String, Error> {
    let id = Uuid::parse_str(user_id).map_err(|_| Error::Failed("display-name"))?;
    let row: Option<(Option<String>,)> = sqlx::query_as(&display_name_sql())
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("display-name", error))?;
    row.map(|row| row.0.unwrap_or_default())
        .ok_or(Error::Failed("display-name"))
}

/// The synced-comment lock (`app/serializers/issue.py:79-91`): native rows
/// (`external_source` empty) short-circuit before any DB probe.
pub async fn comment_is_synced(
    pool: &PgPool,
    external_source: Option<&str>,
    comment_id: &str,
) -> Result<bool, Error> {
    if external_source.unwrap_or("").is_empty() {
        return Ok(false);
    }
    let id = Uuid::parse_str(comment_id).map_err(|_| Error::Failed("comment-synced"))?;
    let git: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM git_comment_syncs WHERE deleted_at IS NULL AND comment_id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("comment-synced-git", error))?;
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM github_comment_syncs WHERE deleted_at IS NULL AND comment_id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| DbError::new("comment-synced-github", error))?;
    Ok(github.is_some())
}
