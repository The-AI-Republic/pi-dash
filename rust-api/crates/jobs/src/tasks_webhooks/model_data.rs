//! Live `get_model_data` execution for the webhook fan-out (PIDASHCONV-1105).
//!
//! Port of `get_model_data` (`apps/api/pi_dash/bgtasks/webhook_task.py:143-187`)
//! plus the nine DRF serializers it dispatches through (`SERIALIZER_MAPPER`,
//! `:58-68`). The dispatch plan (bindings, lookup mode, error messages)
//! stays in [`pidash_services::tasks_webhooks::webhook_data`]; this module
//! owns the row fetch and the byte-exact DRF rendering.
//!
//! Every shape below is pinned against a Django-shell golden
//! (`rust-api/fixtures/tasks_webhooks/fx-web-05-model-data.json`, fixed
//! UUIDs and timestamps): key order, `null` vs missing, datetime `Z`
//! rendering, float/int coercions, base64 bytes, and the nested
//! label/assignee/state/issue-detail expansions.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * QUIRK-ND-1 (`IssueExpandSerializer.cycle`/`module`): the sources
//!   (`issue_cycle.cycle`, `issue_module.module`) traverse a *reverse FK*
//!   manager (`related_name="issue_cycle"` on `CycleIssue.issue`), and a
//!   manager has no `.cycle`/`.module` attribute — `AttributeError` on a
//!   `read_only` field is `SkipField`, so the keys are ALWAYS omitted,
//!   whether or not link rows exist. The renderers below never emit them.
//! * QUIRK-ND-2 (annotation fields): `total_members`, `total_issues`,
//!   `sub_issues_count`, `is_member` and siblings have no model attribute
//!   on a bare `.get()` instance, so DRF skips them (`SkipField`) while
//!   `cover_image_url` (a real property) renders. The renderers emit
//!   exactly the resolving subset the golden pins.
//! * QUIRK-ND-3 (`issue_comment.url`): rendered by `issue_web_url` and
//!   *omitted* (not null) when the web base URL is unconfigured or any
//!   identifier part is missing (`to_representation` pops `None`).
//! * QUIRK-ND-4 (`description_binary`): DRF has no binary mapping, so the
//!   bytes render through the base64 fallback (`'AP9BQkN/gA=='` in the
//!   golden for `b"\x00\xffABC\x7f\x80"`).
//! * QUIRK-ND-5 (nested `issue_detail`): constructed without context, so
//!   its labels/assignees render as bare id-string lists while the
//!   top-level issue expands them to objects.
//!
//!Fetch note (soft-delete scope): every `Model.objects.get(pk=…)` here
//!reads through the inherited `SoftDeletionManager`
//!(`pi_dash/db/mixins.py:65`), so the fetch adds `deleted_at IS NULL`.
//!That covers projects, issues, cycles, modules, the `cycle_issues`/
//!`module_issues` link rows, `issue_comments`, `intake_issues`, and the
//!`issue_labels`/`issue_assignees` through rows (whose reverse-FK manager
//!is derived from `IssueLabel`/`IssueAssignee._default_manager` —
//!`SoftDeletionManager`). Two families stay UNguarded, matching Python:
//!forward-FK dereferences (`instance.state`, `instance.issue`,
//!`obj.workspace`, `obj.project`, `obj.avatar_asset`) go through
//!`_base_manager` (a plain `Manager`), and M2M targets
//!(`module.members.all()` → `User`) use the target's unfiltered
//!`UserManager`. The nested `intake_issue.issue_detail` is such a forward
//!FK (`source="issue"`), so its issue read is unscoped. A garbage id
//!raises Django's `ValidationError` ("not a valid UUID"), which the
//!fan-out's broad `except` logs and swallows like any non-`DoesNotExist`
//!error; [`ModelDataError::InvalidId`] carries that path.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Failures of `get_model_data`, mirroring the Python exception routing:
/// [`ModelDataError::NotFound`] is the silent `ObjectDoesNotExist` path,
/// everything else logs and returns through the fan-out's broad `except`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelDataError {
    /// `ValueError("Model not found for event: {event}")` (`:160-161`).
    UnknownModel(String),
    /// `ValidationError("'…' is not a valid UUID.")` from a garbage id on
    /// a UUID pk lookup — logged and swallowed, never silent.
    InvalidId(String),
    /// `ObjectDoesNotExist("No {event} found with id: {event_id}")`
    /// (`:186`) — the silent race path.
    NotFound(String, String),
    /// Any store failure mid-read: Python's broad `except` logs it and
    /// returns (never the silent path, never a retry).
    Store(String),
}

impl std::fmt::Display for ModelDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelDataError::UnknownModel(event) => {
                write!(f, "Model not found for event: {event}")
            }
            ModelDataError::InvalidId(id) => write!(f, "'{id}' is not a valid UUID."),
            ModelDataError::NotFound(event, id) => {
                write!(f, "No {event} found with id: {id}")
            }
            ModelDataError::Store(detail) => write!(f, "model data store failed: {detail}"),
        }
    }
}

impl std::error::Error for ModelDataError {}

/// True for the silent race path (`isinstance(e, ObjectDoesNotExist)`).
pub fn is_not_found(error: &ModelDataError) -> bool {
    matches!(error, ModelDataError::NotFound(_, _))
}

/// True when `event` has no `MODEL_MAPPER` binding (the `ValueError`
/// path — the fan-out checks before querying).
pub fn binding_unknown(event: &str) -> bool {
    pidash_services::tasks_webhooks::webhook_data::binding_for(event).is_none()
}

/// `web_base_url` (`pi_dash/utils/host.py`): `WEB_URL or APP_BASE_URL`
/// with the trailing slash stripped. Empty/unset on both sides yields
/// `None` (the `""` Python returns is falsy at every caller).
pub fn web_base_url(web_url: Option<&str>, app_base_url: Option<&str>) -> Option<String> {
    let base = web_url
        .filter(|s| !s.is_empty())
        .or_else(|| app_base_url.filter(|s| !s.is_empty()))?;
    let stripped = base.trim_end_matches('/').to_owned();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// `issue_web_url` (`pi_dash/utils/host.py:86-99`):
/// `{base}/{slug}/browse/{identifier}-{seq}`, or `None` when the base is
/// unconfigured or any identifier part is missing.
pub fn issue_web_url(
    base: Option<&str>,
    workspace_slug: Option<&str>,
    project_identifier: Option<&str>,
    sequence_id: Option<i32>,
) -> Option<String> {
    let base = base.filter(|s| !s.is_empty())?;
    let slug = workspace_slug.filter(|s| !s.is_empty())?;
    let identifier = project_identifier.filter(|s| !s.is_empty())?;
    let seq = sequence_id?;
    Some(format!("{base}/{slug}/browse/{identifier}-{seq}"))
}

/// DRF `DateTimeField` ISO-8601 rendering: `isoformat()` with a `Z` suffix
/// for UTC. Python prints microseconds as exactly six digits when nonzero
/// (`.100000`, never trimmed), and omits the fraction when zero —
/// `chrono::SecondsFormat::AutoSi` would trim, so this formats by hand.
pub fn drf_datetime(value: &DateTime<Utc>) -> String {
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        value.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        format!("{}.{:06}Z", value.format("%Y-%m-%dT%H:%M:%S"), micros)
    }
}

/// DRF `DateField` rendering: `isoformat()` (`YYYY-MM-DD`).
pub fn drf_date(value: &NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

fn uuid_value(id: &Uuid) -> Value {
    Value::String(id.to_string())
}

fn opt_uuid(id: &Option<Uuid>) -> Value {
    id.as_ref().map_or(Value::Null, uuid_value)
}

fn opt_str(text: &Option<String>) -> Value {
    text.as_ref()
        .map_or(Value::Null, |s| Value::String(s.clone()))
}

fn opt_datetime(value: &Option<DateTime<Utc>>) -> Value {
    value
        .as_ref()
        .map_or(Value::Null, |dt| Value::String(drf_datetime(dt)))
}

fn opt_date(value: &Option<NaiveDate>) -> Value {
    value
        .as_ref()
        .map_or(Value::Null, |d| Value::String(drf_date(d)))
}

fn float_value(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-99`): the entity-type
/// branches. `workspace_slug` is read only on the workspace-scoped
/// branches, exactly like the `self.workspace.slug` dereferences.
pub fn asset_url(
    asset_id: &Uuid,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&Uuid>,
    issue_id: Option<&Uuid>,
) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let slug = workspace_slug?;
            let project = project_id?;
            let issue = issue_id?;
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
            let slug = workspace_slug?;
            let project = project_id?;
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => None,
    }
}

/// One `file_assets` row as `asset_url` reads it.
pub struct FileAssetRow {
    pub id: Uuid,
    pub entity_type: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
}

async fn fetch_file_asset(pool: &PgPool, id: Uuid) -> Result<Option<FileAssetRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, entity_type, workspace_id, project_id, issue_id FROM file_assets WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| FileAssetRow {
        id: row.get("id"),
        entity_type: row.try_get("entity_type").unwrap_or(None),
        workspace_id: row.try_get("workspace_id").unwrap_or(None),
        project_id: row.try_get("project_id").unwrap_or(None),
        issue_id: row.try_get("issue_id").unwrap_or(None),
    }))
}

async fn workspace_slug(pool: &PgPool, id: Uuid) -> Result<Option<String>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT slug FROM workspaces WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|row| row.try_get("slug").ok()))
}

/// Resolve `avatar_url` / `cover_image_url`: the asset FK wins when set
/// (`avatar_asset.asset_url`, possibly `None`), else the text field when
/// truthy, else `None` (`User.avatar_url`, `Project.cover_image_url`).
async fn image_url(
    pool: &PgPool,
    asset_id: Option<Uuid>,
    text: &str,
) -> Result<Option<String>, sqlx::Error> {
    if let Some(id) = asset_id {
        if let Some(asset) = fetch_file_asset(pool, id).await? {
            let slug = match asset.workspace_id {
                Some(wid) => workspace_slug(pool, wid).await?,
                None => None,
            };
            return Ok(asset_url(
                &asset.id,
                asset.entity_type.as_deref(),
                slug.as_deref(),
                asset.project_id.as_ref(),
                asset.issue_id.as_ref(),
            ));
        }
        return Ok(None);
    }
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text.to_owned()))
    }
}

// ---------------------------------------------------------------------------
// user (UserLiteSerializer, 7 keys in Meta.fields order)
// ---------------------------------------------------------------------------

/// `users` columns `UserLiteSerializer` reads.
pub struct UserRow {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub avatar: String,
    pub avatar_asset_id: Option<Uuid>,
    pub display_name: String,
}

async fn fetch_user(pool: &PgPool, id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, first_name, last_name, email, avatar, avatar_asset_id, display_name \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| UserRow {
        id: row.get("id"),
        first_name: row.get("first_name"),
        last_name: row.get("last_name"),
        email: row.try_get("email").unwrap_or(None),
        avatar: row.get("avatar"),
        avatar_asset_id: row.try_get("avatar_asset_id").unwrap_or(None),
        display_name: row.get("display_name"),
    }))
}

/// Render `UserLiteSerializer` in `Meta.fields` order (`id, first_name,
/// last_name, email, avatar, avatar_url, display_name`; the doubled
/// `email` entry collapses to its first position).
pub fn render_user(row: &UserRow, avatar_url: Option<&str>) -> Map<String, Value> {
    let mut out = Map::with_capacity(7);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert(
        "first_name".to_owned(),
        Value::String(row.first_name.clone()),
    );
    out.insert("last_name".to_owned(), Value::String(row.last_name.clone()));
    out.insert("email".to_owned(), opt_str(&row.email));
    out.insert("avatar".to_owned(), Value::String(row.avatar.clone()));
    out.insert(
        "avatar_url".to_owned(),
        avatar_url.map_or(Value::Null, |u| Value::String(u.to_owned())),
    );
    out.insert(
        "display_name".to_owned(),
        Value::String(row.display_name.clone()),
    );
    out
}

// ---------------------------------------------------------------------------
// project (ProjectSerializer, __all__ + resolving declared fields)
// ---------------------------------------------------------------------------

/// `projects` columns `ProjectSerializer` reads, in render order.
pub struct ProjectRow {
    pub id: Uuid,
    pub cover_image_asset_id: Option<Uuid>,
    pub cover_image: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: String,
    pub description: String,
    pub description_text: Option<String>,
    pub description_html: Option<String>,
    pub network: i32,
    pub identifier: String,
    pub emoji: Option<String>,
    pub icon_prop: Option<Value>,
    pub module_view: bool,
    pub cycle_view: bool,
    pub issue_views_view: bool,
    pub page_view: bool,
    pub intake_view: bool,
    pub is_time_tracking_enabled: bool,
    pub is_issue_type_enabled: bool,
    pub is_default: bool,
    pub guest_view_all_features: bool,
    pub members_can_edit_states: bool,
    pub archive_in: i32,
    pub close_in: i32,
    pub logo_props: Value,
    pub archived_at: Option<DateTime<Utc>>,
    pub timezone: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub repo_url: String,
    pub base_branch: String,
    pub agent_default_interval_seconds: i32,
    pub agent_default_max_ticks: i32,
    pub agent_review_default_interval_seconds: i32,
    pub agent_test_default_interval_seconds: i32,
    pub agent_ticking_enabled: bool,
    pub default_agent_executor: String,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub default_assignee_id: Option<Uuid>,
    pub project_lead_id: Option<Uuid>,
    pub estimate_id: Option<Uuid>,
    pub default_state_id: Option<Uuid>,
}

async fn fetch_project(pool: &PgPool, id: Uuid) -> Result<Option<ProjectRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, cover_image_asset_id, cover_image, created_at, updated_at, deleted_at, \
         name, description, description_text, description_html, network, identifier, emoji, \
         icon_prop, module_view, cycle_view, issue_views_view, page_view, intake_view, \
         is_time_tracking_enabled, is_issue_type_enabled, is_default, guest_view_all_features, \
         members_can_edit_states, archive_in, close_in, logo_props, archived_at, timezone, \
         external_source, external_id, repo_url, base_branch, agent_default_interval_seconds, \
         agent_default_max_ticks, agent_review_default_interval_seconds, \
         agent_test_default_interval_seconds, agent_ticking_enabled, default_agent_executor, \
         created_by_id, updated_by_id, workspace_id, default_assignee_id, project_lead_id, \
         estimate_id, default_state_id \
         FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ProjectRow {
        id: row.get("id"),
        cover_image_asset_id: row.try_get("cover_image_asset_id").unwrap_or(None),
        cover_image: row.try_get("cover_image").unwrap_or(None),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        name: row.get("name"),
        description: row.get("description"),
        description_text: row.try_get("description_text").unwrap_or(None),
        description_html: row.try_get("description_html").unwrap_or(None),
        network: row.get("network"),
        identifier: row.get("identifier"),
        emoji: row.try_get("emoji").unwrap_or(None),
        icon_prop: row.try_get("icon_prop").unwrap_or(None),
        module_view: row.get("module_view"),
        cycle_view: row.get("cycle_view"),
        issue_views_view: row.get("issue_views_view"),
        page_view: row.get("page_view"),
        intake_view: row.get("intake_view"),
        is_time_tracking_enabled: row.get("is_time_tracking_enabled"),
        is_issue_type_enabled: row.get("is_issue_type_enabled"),
        is_default: row.get("is_default"),
        guest_view_all_features: row.get("guest_view_all_features"),
        members_can_edit_states: row.get("members_can_edit_states"),
        archive_in: row.get("archive_in"),
        close_in: row.get("close_in"),
        logo_props: row.get("logo_props"),
        archived_at: row.try_get("archived_at").unwrap_or(None),
        timezone: row.get("timezone"),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        repo_url: row.get("repo_url"),
        base_branch: row.get("base_branch"),
        agent_default_interval_seconds: row.get("agent_default_interval_seconds"),
        agent_default_max_ticks: row.get("agent_default_max_ticks"),
        agent_review_default_interval_seconds: row.get("agent_review_default_interval_seconds"),
        agent_test_default_interval_seconds: row.get("agent_test_default_interval_seconds"),
        agent_ticking_enabled: row.get("agent_ticking_enabled"),
        default_agent_executor: row.get("default_agent_executor"),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        workspace_id: row.get("workspace_id"),
        default_assignee_id: row.try_get("default_assignee_id").unwrap_or(None),
        project_lead_id: row.try_get("project_lead_id").unwrap_or(None),
        estimate_id: row.try_get("estimate_id").unwrap_or(None),
        default_state_id: row.try_get("default_state_id").unwrap_or(None),
    }))
}

/// Render `ProjectSerializer`: `id`, the one resolving declared field
/// (`cover_image_url`; the annotation siblings skip per QUIRK-ND-2), then
/// every model field in model order.
pub fn render_project(row: &ProjectRow, cover_image_url: Option<&str>) -> Map<String, Value> {
    let mut out = Map::with_capacity(47);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert(
        "cover_image_url".to_owned(),
        cover_image_url.map_or(Value::Null, |u| Value::String(u.to_owned())),
    );
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    out.insert(
        "description_text".to_owned(),
        opt_str(&row.description_text),
    );
    out.insert(
        "description_html".to_owned(),
        opt_str(&row.description_html),
    );
    out.insert("network".to_owned(), Value::from(row.network));
    out.insert(
        "identifier".to_owned(),
        Value::String(row.identifier.clone()),
    );
    out.insert("emoji".to_owned(), opt_str(&row.emoji));
    out.insert(
        "icon_prop".to_owned(),
        row.icon_prop.clone().unwrap_or(Value::Null),
    );
    out.insert("module_view".to_owned(), Value::from(row.module_view));
    out.insert("cycle_view".to_owned(), Value::from(row.cycle_view));
    out.insert(
        "issue_views_view".to_owned(),
        Value::from(row.issue_views_view),
    );
    out.insert("page_view".to_owned(), Value::from(row.page_view));
    out.insert("intake_view".to_owned(), Value::from(row.intake_view));
    out.insert(
        "is_time_tracking_enabled".to_owned(),
        Value::from(row.is_time_tracking_enabled),
    );
    out.insert(
        "is_issue_type_enabled".to_owned(),
        Value::from(row.is_issue_type_enabled),
    );
    out.insert("is_default".to_owned(), Value::from(row.is_default));
    out.insert(
        "guest_view_all_features".to_owned(),
        Value::from(row.guest_view_all_features),
    );
    out.insert(
        "members_can_edit_states".to_owned(),
        Value::from(row.members_can_edit_states),
    );
    out.insert("cover_image".to_owned(), opt_str(&row.cover_image));
    out.insert("archive_in".to_owned(), Value::from(row.archive_in));
    out.insert("close_in".to_owned(), Value::from(row.close_in));
    out.insert("logo_props".to_owned(), row.logo_props.clone());
    out.insert("archived_at".to_owned(), opt_datetime(&row.archived_at));
    out.insert("timezone".to_owned(), Value::String(row.timezone.clone()));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert("repo_url".to_owned(), Value::String(row.repo_url.clone()));
    out.insert(
        "base_branch".to_owned(),
        Value::String(row.base_branch.clone()),
    );
    out.insert(
        "agent_default_interval_seconds".to_owned(),
        Value::from(row.agent_default_interval_seconds),
    );
    out.insert(
        "agent_default_max_ticks".to_owned(),
        Value::from(row.agent_default_max_ticks),
    );
    out.insert(
        "agent_review_default_interval_seconds".to_owned(),
        Value::from(row.agent_review_default_interval_seconds),
    );
    out.insert(
        "agent_test_default_interval_seconds".to_owned(),
        Value::from(row.agent_test_default_interval_seconds),
    );
    out.insert(
        "agent_ticking_enabled".to_owned(),
        Value::from(row.agent_ticking_enabled),
    );
    out.insert(
        "default_agent_executor".to_owned(),
        Value::String(row.default_agent_executor.clone()),
    );
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert(
        "default_assignee".to_owned(),
        opt_uuid(&row.default_assignee_id),
    );
    out.insert("project_lead".to_owned(), opt_uuid(&row.project_lead_id));
    out.insert(
        "cover_image_asset".to_owned(),
        opt_uuid(&row.cover_image_asset_id),
    );
    out.insert("estimate".to_owned(), opt_uuid(&row.estimate_id));
    out.insert("default_state".to_owned(), opt_uuid(&row.default_state_id));
    out
}

// ---------------------------------------------------------------------------
// issue (IssueExpandSerializer, workpad excluded, expand-driven relations)
// ---------------------------------------------------------------------------

/// `issues` columns `IssueExpandSerializer` reads (`workpad` excluded).
pub struct IssueRow {
    pub id: Uuid,
    pub state_id: Option<Uuid>,
    pub description_json: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub point: Option<i32>,
    pub name: String,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_binary: Option<Vec<u8>>,
    pub priority: String,
    pub complexity_score: i32,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub work_type: Option<String>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub estimate_point_id: Option<Uuid>,
    pub type_id: Option<Uuid>,
    pub assigned_pod_id: Option<Uuid>,
}

async fn fetch_issue(
    pool: &PgPool,
    id: Uuid,
    soft_scope: bool,
) -> Result<Option<IssueRow>, sqlx::Error> {
    // `true` mirrors a `Model.objects.get(pk=)` read (the `SoftDeletionManager`
    // adds `deleted_at IS NULL`); `false` mirrors a forward-FK dereference
    // (`instance.issue`), which goes through `_base_manager` and is unscoped.
    let guard = if soft_scope {
        " AND deleted_at IS NULL"
    } else {
        ""
    };
    let sql = format!(
        "SELECT id, state_id, description_json, created_at, updated_at, deleted_at, point, \
         name, description_html, description_stripped, description_binary, priority, \
         complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, \
         archived_at, is_draft, external_source, external_id, git_work_branch, created_via, \
         agent_executor, work_type, created_by_id, updated_by_id, project_id, workspace_id, \
         parent_id, estimate_point_id, type_id, assigned_pod_id \
         FROM issues WHERE id = $1{guard}"
    );
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(&sql).bind(id).fetch_optional(pool).await?;
    Ok(row.map(|row| IssueRow {
        id: row.get("id"),
        state_id: row.try_get("state_id").unwrap_or(None),
        description_json: row.get("description_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        point: row.try_get("point").unwrap_or(None),
        name: row.get("name"),
        description_html: row.get("description_html"),
        description_stripped: row.try_get("description_stripped").unwrap_or(None),
        description_binary: row.try_get("description_binary").unwrap_or(None),
        priority: row.get("priority"),
        complexity_score: row.get("complexity_score"),
        start_date: row.try_get("start_date").unwrap_or(None),
        target_date: row.try_get("target_date").unwrap_or(None),
        sequence_id: row.get("sequence_id"),
        sort_order: row.get("sort_order"),
        completed_at: row.try_get("completed_at").unwrap_or(None),
        archived_at: row.try_get("archived_at").unwrap_or(None),
        is_draft: row.get("is_draft"),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        git_work_branch: row.get("git_work_branch"),
        created_via: row.try_get("created_via").unwrap_or(None),
        agent_executor: row.try_get("agent_executor").unwrap_or(None),
        work_type: row.try_get("work_type").unwrap_or(None),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        parent_id: row.try_get("parent_id").unwrap_or(None),
        estimate_point_id: row.try_get("estimate_point_id").unwrap_or(None),
        type_id: row.try_get("type_id").unwrap_or(None),
        assigned_pod_id: row.try_get("assigned_pod_id").unwrap_or(None),
    }))
}

/// `StateLiteSerializer`: `id, name, color, group` (field order).
pub struct StateLiteRow {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub group: String,
}

async fn fetch_state_lite(pool: &PgPool, id: Uuid) -> Result<Option<StateLiteRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT id, name, color, \"group\" FROM states WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|row| StateLiteRow {
        id: row.get("id"),
        name: row.get("name"),
        color: row.get("color"),
        group: row.get("group"),
    }))
}

pub fn render_state_lite(row: &StateLiteRow) -> Map<String, Value> {
    let mut out = Map::with_capacity(4);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert("color".to_owned(), Value::String(row.color.clone()));
    out.insert("group".to_owned(), Value::String(row.group.clone()));
    out
}

/// `LabelLiteSerializer`: `id, name, color` (field order).
pub struct LabelLiteRow {
    pub id: Uuid,
    pub name: String,
    pub color: String,
}

pub fn render_label_lite(row: &LabelLiteRow) -> Map<String, Value> {
    let mut out = Map::with_capacity(3);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert("color".to_owned(), Value::String(row.color.clone()));
    out
}

/// Issue labels in prefetch order: `label_issue.all()` walks the through
/// rows in `IssueLabel.Meta.ordering` (`-created_at`) and dereferences
/// each label — no label-side filter, no label-side ordering.
async fn fetch_issue_labels(
    pool: &PgPool,
    issue_id: Uuid,
) -> Result<Vec<LabelLiteRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT l.id, l.name, l.color FROM labels l \
         JOIN issue_labels il ON il.label_id = l.id \
         WHERE il.issue_id = $1 AND il.deleted_at IS NULL ORDER BY il.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| LabelLiteRow {
            id: row.get("id"),
            name: row.get("name"),
            color: row.get("color"),
        })
        .collect())
}

/// Issue assignees in prefetch order (`IssueAssignee.Meta.ordering`,
/// `-created_at`), with the `UserLiteSerializer` columns per row.
async fn fetch_issue_assignees(pool: &PgPool, issue_id: Uuid) -> Result<Vec<UserRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, u.avatar_asset_id, \
         u.display_name FROM users u \
         JOIN issue_assignees ia ON ia.assignee_id = u.id \
         WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL ORDER BY ia.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| UserRow {
            id: row.get("id"),
            first_name: row.get("first_name"),
            last_name: row.get("last_name"),
            email: row.try_get("email").unwrap_or(None),
            avatar: row.get("avatar"),
            avatar_asset_id: row.try_get("avatar_asset_id").unwrap_or(None),
            display_name: row.get("display_name"),
        })
        .collect())
}

/// Render `IssueExpandSerializer` in golden key order: `id`, the four
/// resolving declared fields (`labels`, `assignees`, `state`,
/// `description`; `cycle`/`module` always skip per QUIRK-ND-1), then the
/// model fields minus `workpad`. `expand` selects object vs id-string
/// lists for labels/assignees — the top-level fan-out call always
/// expands, the nested `issue_detail` never does (QUIRK-ND-5).
#[allow(clippy::too_many_arguments)]
pub fn render_issue(
    row: &IssueRow,
    labels: Value,
    assignees: Value,
    state: Value,
) -> Map<String, Value> {
    use base64::Engine;
    let mut out = Map::with_capacity(36);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert("labels".to_owned(), labels);
    out.insert("assignees".to_owned(), assignees);
    out.insert("state".to_owned(), state);
    out.insert("description".to_owned(), row.description_json.clone());
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert(
        "point".to_owned(),
        row.point.map_or(Value::Null, Value::from),
    );
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert("description_json".to_owned(), row.description_json.clone());
    out.insert(
        "description_html".to_owned(),
        Value::String(row.description_html.clone()),
    );
    out.insert(
        "description_stripped".to_owned(),
        opt_str(&row.description_stripped),
    );
    out.insert(
        "description_binary".to_owned(),
        row.description_binary
            .as_ref()
            .map_or(Value::Null, |bytes| {
                Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
            }),
    );
    out.insert("priority".to_owned(), Value::String(row.priority.clone()));
    out.insert(
        "complexity_score".to_owned(),
        Value::from(row.complexity_score),
    );
    out.insert("start_date".to_owned(), opt_date(&row.start_date));
    out.insert("target_date".to_owned(), opt_date(&row.target_date));
    out.insert("sequence_id".to_owned(), Value::from(row.sequence_id));
    out.insert("sort_order".to_owned(), float_value(row.sort_order));
    out.insert("completed_at".to_owned(), opt_datetime(&row.completed_at));
    out.insert("archived_at".to_owned(), opt_date(&row.archived_at));
    out.insert("is_draft".to_owned(), Value::from(row.is_draft));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert(
        "git_work_branch".to_owned(),
        Value::String(row.git_work_branch.clone()),
    );
    out.insert("created_via".to_owned(), opt_str(&row.created_via));
    out.insert("agent_executor".to_owned(), opt_str(&row.agent_executor));
    out.insert("work_type".to_owned(), opt_str(&row.work_type));
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert("parent".to_owned(), opt_uuid(&row.parent_id));
    out.insert(
        "estimate_point".to_owned(),
        opt_uuid(&row.estimate_point_id),
    );
    out.insert("type".to_owned(), opt_uuid(&row.type_id));
    out.insert("assigned_pod".to_owned(), opt_uuid(&row.assigned_pod_id));
    out
}

// ---------------------------------------------------------------------------
// cycle (CycleSerializer, all annotation fields skip)
// ---------------------------------------------------------------------------

/// `cycles` columns `CycleSerializer` reads, in render order.
pub struct CycleRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: String,
    pub description: String,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub progress_snapshot: Value,
    pub archived_at: Option<DateTime<Utc>>,
    pub logo_props: Value,
    pub timezone: String,
    pub version: i32,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub owned_by_id: Option<Uuid>,
}

async fn fetch_cycle(pool: &PgPool, id: Uuid) -> Result<Option<CycleRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, created_at, updated_at, deleted_at, name, description, start_date, \
         end_date, view_props, sort_order, external_source, external_id, progress_snapshot, \
         archived_at, logo_props, timezone, version, created_by_id, updated_by_id, \
         project_id, workspace_id, owned_by_id \
         FROM cycles WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| CycleRow {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        name: row.get("name"),
        description: row.get("description"),
        start_date: row.try_get("start_date").unwrap_or(None),
        end_date: row.try_get("end_date").unwrap_or(None),
        view_props: row.get("view_props"),
        sort_order: row.get("sort_order"),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        progress_snapshot: row.get("progress_snapshot"),
        archived_at: row.try_get("archived_at").unwrap_or(None),
        logo_props: row.get("logo_props"),
        timezone: row.get("timezone"),
        version: row.get("version"),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        owned_by_id: row.try_get("owned_by_id").unwrap_or(None),
    }))
}

/// Render `CycleSerializer`: `id` plus the model fields — every declared
/// annotation field skips (QUIRK-ND-2).
pub fn render_cycle(row: &CycleRow) -> Map<String, Value> {
    let mut out = Map::with_capacity(22);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    out.insert("start_date".to_owned(), opt_datetime(&row.start_date));
    out.insert("end_date".to_owned(), opt_datetime(&row.end_date));
    out.insert("view_props".to_owned(), row.view_props.clone());
    out.insert("sort_order".to_owned(), float_value(row.sort_order));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert(
        "progress_snapshot".to_owned(),
        row.progress_snapshot.clone(),
    );
    out.insert("archived_at".to_owned(), opt_datetime(&row.archived_at));
    out.insert("logo_props".to_owned(), row.logo_props.clone());
    out.insert("timezone".to_owned(), Value::String(row.timezone.clone()));
    out.insert("version".to_owned(), Value::from(row.version));
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert("owned_by".to_owned(), opt_uuid(&row.owned_by_id));
    out
}

// ---------------------------------------------------------------------------
// module (ModuleSerializer, members appended by to_representation)
// ---------------------------------------------------------------------------

/// `modules` columns `ModuleSerializer` reads, in render order.
pub struct ModuleRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: String,
    pub description: String,
    pub description_text: Option<String>,
    pub description_html: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub status: String,
    pub view_props: Value,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub archived_at: Option<DateTime<Utc>>,
    pub logo_props: Value,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub lead_id: Option<Uuid>,
}

async fn fetch_module(pool: &PgPool, id: Uuid) -> Result<Option<ModuleRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, created_at, updated_at, deleted_at, name, description, description_text, \
         description_html, start_date, target_date, status, view_props, sort_order, \
         external_source, external_id, archived_at, logo_props, created_by_id, updated_by_id, \
         project_id, workspace_id, lead_id \
         FROM modules WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ModuleRow {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        name: row.get("name"),
        description: row.get("description"),
        description_text: row.try_get("description_text").unwrap_or(None),
        description_html: row.try_get("description_html").unwrap_or(None),
        start_date: row.try_get("start_date").unwrap_or(None),
        target_date: row.try_get("target_date").unwrap_or(None),
        status: row.get("status"),
        view_props: row.get("view_props"),
        sort_order: row.get("sort_order"),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        archived_at: row.try_get("archived_at").unwrap_or(None),
        logo_props: row.get("logo_props"),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        lead_id: row.try_get("lead_id").unwrap_or(None),
    }))
}

/// Module member ids: `instance.members.all()` is a `User` queryset in the
/// target model's ordering (`-created_at`), rendered as id strings.
async fn fetch_module_members(pool: &PgPool, module_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT u.id FROM users u \
         JOIN module_members mm ON mm.member_id = u.id \
         WHERE mm.module_id = $1 ORDER BY u.created_at DESC",
    )
    .bind(module_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|row| row.get("id")).collect())
}

/// Render `ModuleSerializer`: `id`, the model fields (the write-only
/// `members` input and the annotation fields never render), then the
/// `members` id list appended by `to_representation`.
pub fn render_module(row: &ModuleRow, members: &[Uuid]) -> Map<String, Value> {
    let mut out = Map::with_capacity(23);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    out.insert(
        "description_text".to_owned(),
        opt_str(&row.description_text),
    );
    out.insert(
        "description_html".to_owned(),
        opt_str(&row.description_html),
    );
    out.insert("start_date".to_owned(), opt_date(&row.start_date));
    out.insert("target_date".to_owned(), opt_date(&row.target_date));
    out.insert("status".to_owned(), Value::String(row.status.clone()));
    out.insert("view_props".to_owned(), row.view_props.clone());
    out.insert("sort_order".to_owned(), float_value(row.sort_order));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert("archived_at".to_owned(), opt_datetime(&row.archived_at));
    out.insert("logo_props".to_owned(), row.logo_props.clone());
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert("lead".to_owned(), opt_uuid(&row.lead_id));
    out.insert(
        "members".to_owned(),
        Value::Array(
            members
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    out
}

// ---------------------------------------------------------------------------
// cycle_issue / module_issue (link serializers, counts skip)
// ---------------------------------------------------------------------------

/// Shared shape of the `cycle_issues` / `module_issues` link rows.
pub struct LinkRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub first_id: Uuid,
    pub second_id: Uuid,
}

async fn fetch_link(
    pool: &PgPool,
    table: &str,
    first_col: &str,
    second_col: &str,
    id: Uuid,
) -> Result<Option<LinkRow>, sqlx::Error> {
    let sql = format!(
        "SELECT id, created_at, updated_at, deleted_at, created_by_id, updated_by_id, \
         project_id, workspace_id, {first_col}, {second_col} FROM {table} \
         WHERE id = $1 AND deleted_at IS NULL"
    );
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(&sql).bind(id).fetch_optional(pool).await?;
    Ok(row.map(|row| LinkRow {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        first_id: row.get(first_col),
        second_id: row.get(second_col),
    }))
}

/// Render a link row: `id`, audit columns, then the two link ends in
/// model order (`issue, cycle` / `module, issue`; `sub_issues_count`
/// skips per QUIRK-ND-2).
pub fn render_link(row: &LinkRow, first_name: &str, second_name: &str) -> Map<String, Value> {
    let mut out = Map::with_capacity(10);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert(first_name.to_owned(), uuid_value(&row.first_id));
    out.insert(second_name.to_owned(), uuid_value(&row.second_id));
    out
}

// ---------------------------------------------------------------------------
// issue_comment (url computed, is_member skips, two fields excluded)
// ---------------------------------------------------------------------------

/// `issue_comments` columns `IssueCommentSerializer` reads
/// (`comment_stripped` / `comment_json` excluded).
pub struct IssueCommentRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub comment_html: String,
    pub attachments: Vec<String>,
    pub labels: Vec<String>,
    pub access: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub speaker_type: String,
    pub speaker_label: String,
    pub speaker_agent_run_id: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub description_id: Option<Uuid>,
    pub issue_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
}

async fn fetch_issue_comment(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<IssueCommentRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, created_at, updated_at, deleted_at, comment_html, attachments, labels, \
         access, external_source, external_id, speaker_type, speaker_label, \
         speaker_agent_run_id, edited_at, created_by_id, updated_by_id, project_id, \
         workspace_id, description_id, issue_id, actor_id, parent_id \
         FROM issue_comments WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| IssueCommentRow {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        comment_html: row.get("comment_html"),
        attachments: row.get("attachments"),
        labels: row.get("labels"),
        access: row.get("access"),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        speaker_type: row.get("speaker_type"),
        speaker_label: row.get("speaker_label"),
        speaker_agent_run_id: row.try_get("speaker_agent_run_id").unwrap_or(None),
        edited_at: row.try_get("edited_at").unwrap_or(None),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        description_id: row.try_get("description_id").unwrap_or(None),
        issue_id: row.get("issue_id"),
        actor_id: row.try_get("actor_id").unwrap_or(None),
        parent_id: row.try_get("parent_id").unwrap_or(None),
    }))
}

/// Render `IssueCommentSerializer`: `id`, the computed `url` (omitted —
/// never null — when unresolvable per QUIRK-ND-3; `is_member` skips),
/// then the model fields minus the two excluded.
pub fn render_issue_comment(row: &IssueCommentRow, url: Option<&str>) -> Map<String, Value> {
    let mut out = Map::with_capacity(23);
    out.insert("id".to_owned(), uuid_value(&row.id));
    if let Some(url) = url {
        out.insert("url".to_owned(), Value::String(url.to_owned()));
    }
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert(
        "comment_html".to_owned(),
        Value::String(row.comment_html.clone()),
    );
    out.insert(
        "attachments".to_owned(),
        Value::Array(
            row.attachments
                .iter()
                .map(|a| Value::String(a.clone()))
                .collect(),
        ),
    );
    out.insert(
        "labels".to_owned(),
        Value::Array(
            row.labels
                .iter()
                .map(|l| Value::String(l.clone()))
                .collect(),
        ),
    );
    out.insert("access".to_owned(), Value::String(row.access.clone()));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert(
        "speaker_type".to_owned(),
        Value::String(row.speaker_type.clone()),
    );
    out.insert(
        "speaker_label".to_owned(),
        Value::String(row.speaker_label.clone()),
    );
    out.insert(
        "speaker_agent_run_id".to_owned(),
        opt_uuid(&row.speaker_agent_run_id),
    );
    out.insert("edited_at".to_owned(), opt_datetime(&row.edited_at));
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert("description".to_owned(), opt_uuid(&row.description_id));
    out.insert("issue".to_owned(), uuid_value(&row.issue_id));
    out.insert("actor".to_owned(), opt_uuid(&row.actor_id));
    out.insert("parent".to_owned(), opt_uuid(&row.parent_id));
    out
}

async fn project_identifier(pool: &PgPool, id: Uuid) -> Result<Option<String>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT identifier FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|row| row.try_get("identifier").ok()))
}

async fn issue_sequence(pool: &PgPool, id: Uuid) -> Result<Option<i32>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query("SELECT sequence_id FROM issues WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|row| row.try_get("sequence_id").ok()))
}

// ---------------------------------------------------------------------------
// intake_issue (nested full issue without expand)
// ---------------------------------------------------------------------------

/// `intake_issues` columns `IntakeIssueSerializer` reads, in render order.
pub struct IntakeIssueRow {
    pub id: Uuid,
    pub intake_id: Uuid,
    pub issue_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub status: i32,
    pub snoozed_till: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub source_email: Option<String>,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub extra: Value,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub duplicate_to_id: Option<Uuid>,
}

async fn fetch_intake_issue(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<IntakeIssueRow>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id, intake_id, issue_id, created_at, updated_at, deleted_at, status, \
         snoozed_till, source, source_email, external_source, external_id, extra, \
         created_by_id, updated_by_id, project_id, workspace_id, duplicate_to_id \
         FROM intake_issues WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| IntakeIssueRow {
        id: row.get("id"),
        intake_id: row.get("intake_id"),
        issue_id: row.get("issue_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.try_get("deleted_at").unwrap_or(None),
        status: row.get("status"),
        snoozed_till: row.try_get("snoozed_till").unwrap_or(None),
        source: row.try_get("source").unwrap_or(None),
        source_email: row.try_get("source_email").unwrap_or(None),
        external_source: row.try_get("external_source").unwrap_or(None),
        external_id: row.try_get("external_id").unwrap_or(None),
        extra: row.get("extra"),
        created_by_id: row.try_get("created_by_id").unwrap_or(None),
        updated_by_id: row.try_get("updated_by_id").unwrap_or(None),
        project_id: row.get("project_id"),
        workspace_id: row.get("workspace_id"),
        duplicate_to_id: row.try_get("duplicate_to_id").unwrap_or(None),
    }))
}

/// Render `IntakeIssueSerializer`: `id`, the nested `issue_detail`
/// (unexpanded), `inbox` (the intake id as UUID string), then the model
/// fields in model order.
pub fn render_intake_issue(
    row: &IntakeIssueRow,
    issue_detail: Map<String, Value>,
) -> Map<String, Value> {
    let mut out = Map::with_capacity(20);
    out.insert("id".to_owned(), uuid_value(&row.id));
    out.insert("issue_detail".to_owned(), Value::Object(issue_detail));
    out.insert("inbox".to_owned(), uuid_value(&row.intake_id));
    out.insert(
        "created_at".to_owned(),
        Value::String(drf_datetime(&row.created_at)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(drf_datetime(&row.updated_at)),
    );
    out.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at));
    out.insert("status".to_owned(), Value::from(row.status));
    out.insert("snoozed_till".to_owned(), opt_datetime(&row.snoozed_till));
    out.insert("source".to_owned(), opt_str(&row.source));
    out.insert("source_email".to_owned(), opt_str(&row.source_email));
    out.insert("external_source".to_owned(), opt_str(&row.external_source));
    out.insert("external_id".to_owned(), opt_str(&row.external_id));
    out.insert("extra".to_owned(), row.extra.clone());
    out.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    out.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    out.insert("project".to_owned(), uuid_value(&row.project_id));
    out.insert("workspace".to_owned(), uuid_value(&row.workspace_id));
    out.insert("intake".to_owned(), uuid_value(&row.intake_id));
    out.insert("issue".to_owned(), uuid_value(&row.issue_id));
    out.insert("duplicate_to".to_owned(), opt_uuid(&row.duplicate_to_id));
    out
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// How a DB failure surfaces to the fan-out: Python's broad `except`
/// logs it and returns (never the silent path, never a retry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "model data store failed: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// Assemble one issue rendering: the row plus labels/assignees/state in
/// `expand` mode (`expand=true`: objects; `false`: id-string lists).
async fn issue_value(pool: &PgPool, row: &IssueRow, expand: bool) -> Result<Value, StoreError> {
    let map_store = |e: sqlx::Error| StoreError(e.to_string());
    let labels = fetch_issue_labels(pool, row.id).await.map_err(map_store)?;
    let assignees = fetch_issue_assignees(pool, row.id)
        .await
        .map_err(map_store)?;
    let (labels_value, assignees_value) = if expand {
        let mut expanded_assignees = Vec::with_capacity(assignees.len());
        for assignee in &assignees {
            let url = image_url(pool, assignee.avatar_asset_id, &assignee.avatar)
                .await
                .map_err(map_store)?;
            expanded_assignees.push(Value::Object(render_user(assignee, url.as_deref())));
        }
        (
            Value::Array(
                labels
                    .iter()
                    .map(|l| Value::Object(render_label_lite(l)))
                    .collect(),
            ),
            Value::Array(expanded_assignees),
        )
    } else {
        (
            Value::Array(
                labels
                    .iter()
                    .map(|l| Value::String(l.id.to_string()))
                    .collect(),
            ),
            Value::Array(
                assignees
                    .iter()
                    .map(|a| Value::String(a.id.to_string()))
                    .collect(),
            ),
        )
    };
    let state_value = match row.state_id {
        None => Value::Null,
        Some(sid) => match fetch_state_lite(pool, sid).await.map_err(map_store)? {
            None => Value::Null,
            Some(state) => Value::Object(render_state_lite(&state)),
        },
    };
    Ok(Value::Object(render_issue(
        row,
        labels_value,
        assignees_value,
        state_value,
    )))
}

/// Live `get_model_data(event, event_id)` (`webhook_task.py:143-187`,
/// `many=False` — the only mode the webhook path calls).
///
/// `web_base` is the resolved `web_base_url()` (or `None` when
/// unconfigured); it feeds the comment `url` only. Unknown events and
/// garbage ids are [`ModelDataError`] (logged, swallowed); a missed row
/// is [`ModelDataError::NotFound`] (silent); a store failure is
/// [`StoreError`] (logged, swallowed).
pub async fn get_model_data(
    pool: &PgPool,
    event: &str,
    event_id: &str,
    web_base: Option<&str>,
) -> Result<Value, ModelDataError> {
    // Mirror the Python check order (model first): an unknown key reports
    // the model even for a garbage id.
    if pidash_services::tasks_webhooks::webhook_data::binding_for(event).is_none() {
        return Err(ModelDataError::UnknownModel(event.to_owned()));
    }
    let id =
        Uuid::parse_str(event_id).map_err(|_| ModelDataError::InvalidId(event_id.to_owned()))?;
    let map_store = |e: sqlx::Error| ModelDataError::Store(e.to_string());
    let missing = || ModelDataError::NotFound(event.to_owned(), event_id.to_owned());
    match event {
        "user" => {
            let row = fetch_user(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            let url = image_url(pool, row.avatar_asset_id, &row.avatar)
                .await
                .map_err(map_store)?;
            Ok(Value::Object(render_user(&row, url.as_deref())))
        }
        "project" => {
            let row = fetch_project(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            let cover = row.cover_image.clone().unwrap_or_default();
            let url = image_url(pool, row.cover_image_asset_id, &cover)
                .await
                .map_err(map_store)?;
            Ok(Value::Object(render_project(&row, url.as_deref())))
        }
        "issue" => {
            let row = fetch_issue(pool, id, true)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            issue_value(pool, &row, true)
                .await
                .map_err(|e| ModelDataError::Store(e.to_string()))
        }
        "cycle" => {
            let row = fetch_cycle(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            Ok(Value::Object(render_cycle(&row)))
        }
        "module" => {
            let row = fetch_module(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            let members = fetch_module_members(pool, id).await.map_err(map_store)?;
            Ok(Value::Object(render_module(&row, &members)))
        }
        "cycle_issue" => {
            let row = fetch_link(pool, "cycle_issues", "issue_id", "cycle_id", id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            Ok(Value::Object(render_link(&row, "issue", "cycle")))
        }
        "module_issue" => {
            let row = fetch_link(pool, "module_issues", "module_id", "issue_id", id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            Ok(Value::Object(render_link(&row, "module", "issue")))
        }
        "issue_comment" => {
            let row = fetch_issue_comment(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            // `obj.workspace.slug if obj.workspace_id else None`, per part.
            let slug = workspace_slug(pool, row.workspace_id)
                .await
                .map_err(map_store)?;
            let identifier = project_identifier(pool, row.project_id)
                .await
                .map_err(map_store)?;
            let sequence = issue_sequence(pool, row.issue_id)
                .await
                .map_err(map_store)?;
            let url = issue_web_url(web_base, slug.as_deref(), identifier.as_deref(), sequence);
            Ok(Value::Object(render_issue_comment(&row, url.as_deref())))
        }
        "intake_issue" => {
            let row = fetch_intake_issue(pool, id)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            let issue = fetch_issue(pool, row.issue_id, false)
                .await
                .map_err(map_store)?
                .ok_or_else(missing)?;
            let detail = issue_value(pool, &issue, false)
                .await
                .map_err(|e| ModelDataError::Store(e.to_string()))?;
            let detail = detail.as_object().cloned().unwrap_or_default();
            Ok(Value::Object(render_intake_issue(&row, detail)))
        }
        _ => Err(ModelDataError::UnknownModel(event.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_webhooks/fx-web-05-model-data.json` —
    /// Django-shell serialization of seeded rows (fixed UUIDs, T0/T1).
    static FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-web-05-model-data.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn golden(event: &str) -> Value {
        let parsed = fixture();
        let raw = parsed["events"][event]["json"]
            .as_str()
            .expect("golden json");
        serde_json::from_str(raw).expect("golden parses")
    }

    fn golden_keys(event: &str) -> Vec<String> {
        fixture()["events"][event]["keys"]
            .as_array()
            .expect("golden keys")
            .iter()
            .map(|k| k.as_str().expect("key").to_owned())
            .collect()
    }

    fn assert_render(event: &str, rendered: Map<String, Value>) {
        let expected = golden(event);
        let order: Vec<&str> = rendered.keys().map(String::as_str).collect();
        let expected_order = golden_keys(event);
        assert_eq!(order, expected_order.as_slice(), "{event} key order");
        assert_eq!(Value::Object(rendered), expected, "{event} values");
    }

    fn uid(value: &Value) -> Uuid {
        value.as_str().expect("uuid").parse().expect("uuid parses")
    }

    fn uid_opt(value: &Value) -> Option<Uuid> {
        match value {
            Value::Null => None,
            s => Some(uid(s)),
        }
    }

    fn str_opt(value: &Value) -> Option<String> {
        match value {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => panic!("expected string-or-null, got {other}"),
        }
    }

    fn dt(value: &Value) -> DateTime<Utc> {
        value
            .as_str()
            .expect("datetime")
            .parse()
            .expect("datetime parses")
    }

    fn dt_opt(value: &Value) -> Option<DateTime<Utc>> {
        match value {
            Value::Null => None,
            s => Some(dt(s)),
        }
    }

    fn date_opt(value: &Value) -> Option<NaiveDate> {
        match value {
            Value::Null => None,
            Value::String(s) => Some(s.parse().expect("date parses")),
            other => panic!("expected date-or-null, got {other}"),
        }
    }

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 15, 10, 20, 30).unwrap()
    }

    fn t1() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 16, 11, 21, 31).unwrap()
    }

    #[test]
    fn user_renders_golden_in_meta_fields_order() {
        let g = golden("user");
        let row = UserRow {
            id: uid(&g["id"]),
            first_name: g["first_name"].as_str().unwrap().to_owned(),
            last_name: g["last_name"].as_str().unwrap().to_owned(),
            email: str_opt(&g["email"]),
            avatar: g["avatar"].as_str().unwrap().to_owned(),
            avatar_asset_id: None,
            display_name: g["display_name"].as_str().unwrap().to_owned(),
        };
        assert_render(
            "user",
            render_user(&row, str_opt(&g["avatar_url"]).as_deref()),
        );
        // The doubled `email` Meta.fields entry collapses to one key.
        assert_eq!(
            golden_keys("user")
                .iter()
                .filter(|k| k.as_str() == "email")
                .count(),
            1
        );
    }

    #[test]
    fn user_bare_avatar_renders_empty_and_null() {
        let parsed = fixture();
        let raw = parsed["user_bare_json"].as_str().expect("bare json");
        let bare: Value = serde_json::from_str(raw).expect("bare parses");
        let row = UserRow {
            id: uid(&bare["id"]),
            first_name: bare["first_name"].as_str().unwrap().to_owned(),
            last_name: bare["last_name"].as_str().unwrap().to_owned(),
            email: str_opt(&bare["email"]),
            avatar: String::new(),
            avatar_asset_id: None,
            display_name: bare["display_name"].as_str().unwrap().to_owned(),
        };
        let rendered = Value::Object(render_user(&row, None));
        assert_eq!(rendered, bare);
        assert_eq!(rendered["avatar"], Value::String(String::new()));
        assert_eq!(rendered["avatar_url"], Value::Null);
    }

    fn project_row(g: &Value) -> ProjectRow {
        ProjectRow {
            id: uid(&g["id"]),
            cover_image_asset_id: uid_opt(&g["cover_image_asset"]),
            cover_image: str_opt(&g["cover_image"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            name: g["name"].as_str().unwrap().to_owned(),
            description: g["description"].as_str().unwrap().to_owned(),
            description_text: str_opt(&g["description_text"]),
            description_html: str_opt(&g["description_html"]),
            network: g["network"].as_i64().unwrap() as i32,
            identifier: g["identifier"].as_str().unwrap().to_owned(),
            emoji: str_opt(&g["emoji"]),
            icon_prop: match &g["icon_prop"] {
                Value::Null => None,
                v => Some(v.clone()),
            },
            module_view: g["module_view"].as_bool().unwrap(),
            cycle_view: g["cycle_view"].as_bool().unwrap(),
            issue_views_view: g["issue_views_view"].as_bool().unwrap(),
            page_view: g["page_view"].as_bool().unwrap(),
            intake_view: g["intake_view"].as_bool().unwrap(),
            is_time_tracking_enabled: g["is_time_tracking_enabled"].as_bool().unwrap(),
            is_issue_type_enabled: g["is_issue_type_enabled"].as_bool().unwrap(),
            is_default: g["is_default"].as_bool().unwrap(),
            guest_view_all_features: g["guest_view_all_features"].as_bool().unwrap(),
            members_can_edit_states: g["members_can_edit_states"].as_bool().unwrap(),
            archive_in: g["archive_in"].as_i64().unwrap() as i32,
            close_in: g["close_in"].as_i64().unwrap() as i32,
            logo_props: g["logo_props"].clone(),
            archived_at: dt_opt(&g["archived_at"]),
            timezone: g["timezone"].as_str().unwrap().to_owned(),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            repo_url: g["repo_url"].as_str().unwrap().to_owned(),
            base_branch: g["base_branch"].as_str().unwrap().to_owned(),
            agent_default_interval_seconds: g["agent_default_interval_seconds"].as_i64().unwrap()
                as i32,
            agent_default_max_ticks: g["agent_default_max_ticks"].as_i64().unwrap() as i32,
            agent_review_default_interval_seconds: g["agent_review_default_interval_seconds"]
                .as_i64()
                .unwrap() as i32,
            agent_test_default_interval_seconds: g["agent_test_default_interval_seconds"]
                .as_i64()
                .unwrap() as i32,
            agent_ticking_enabled: g["agent_ticking_enabled"].as_bool().unwrap(),
            default_agent_executor: g["default_agent_executor"].as_str().unwrap().to_owned(),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            workspace_id: uid(&g["workspace"]),
            default_assignee_id: uid_opt(&g["default_assignee"]),
            project_lead_id: uid_opt(&g["project_lead"]),
            estimate_id: uid_opt(&g["estimate"]),
            default_state_id: uid_opt(&g["default_state"]),
        }
    }

    #[test]
    fn project_renders_golden_with_cover_url_only() {
        let g = golden("project");
        let row = project_row(&g);
        assert_render(
            "project",
            render_project(&row, str_opt(&g["cover_image_url"]).as_deref()),
        );
        // QUIRK-ND-2: the annotation siblings skip; only the resolving
        // declared field renders.
        for skipped in [
            "total_members",
            "total_cycles",
            "total_modules",
            "is_member",
            "sort_order",
            "member_role",
            "is_deployed",
        ] {
            assert!(g.get(skipped).is_none(), "{skipped} must skip");
        }
    }

    fn issue_row(g: &Value) -> IssueRow {
        IssueRow {
            id: uid(&g["id"]),
            state_id: g["state"].get("id").map(uid),
            description_json: g["description_json"].clone(),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            point: g["point"].as_i64().map(|p| p as i32),
            name: g["name"].as_str().unwrap().to_owned(),
            description_html: g["description_html"].as_str().unwrap().to_owned(),
            description_stripped: str_opt(&g["description_stripped"]),
            description_binary: None,
            priority: g["priority"].as_str().unwrap().to_owned(),
            complexity_score: g["complexity_score"].as_i64().unwrap() as i32,
            start_date: date_opt(&g["start_date"]),
            target_date: date_opt(&g["target_date"]),
            sequence_id: g["sequence_id"].as_i64().unwrap() as i32,
            sort_order: g["sort_order"].as_f64().unwrap(),
            completed_at: dt_opt(&g["completed_at"]),
            archived_at: date_opt(&g["archived_at"]),
            is_draft: g["is_draft"].as_bool().unwrap(),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            git_work_branch: g["git_work_branch"].as_str().unwrap().to_owned(),
            created_via: str_opt(&g["created_via"]),
            agent_executor: str_opt(&g["agent_executor"]),
            work_type: str_opt(&g["work_type"]),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            parent_id: uid_opt(&g["parent"]),
            estimate_point_id: uid_opt(&g["estimate_point"]),
            type_id: uid_opt(&g["type"]),
            assigned_pod_id: uid_opt(&g["assigned_pod"]),
        }
    }

    #[test]
    fn issue_renders_golden_expanded_without_cycle_or_module() {
        let g = golden("issue");
        let row = issue_row(&g);
        assert_render(
            "issue",
            render_issue(
                &row,
                g["labels"].clone(),
                g["assignees"].clone(),
                g["state"].clone(),
            ),
        );
        // QUIRK-ND-1: cycle/module always skip (reverse-FK manager).
        assert!(g.get("cycle").is_none());
        assert!(g.get("module").is_none());
        // StateLite / LabelLite shapes pinned inside the expansion.
        assert_eq!(
            g["state"],
            serde_json::json!({
                "id": g["state"]["id"],
                "name": "Golden State",
                "color": "#ff0000",
                "group": "backlog",
            })
        );
        assert_eq!(g["labels"][0]["name"], serde_json::json!("golden-label"));
    }

    #[test]
    fn binary_field_renders_base64() {
        // QUIRK-ND-4: b"\x00\xffABC\x7f\x80" -> 'AP9BQkN/gA=='.
        let mut row = issue_row(&golden("issue"));
        row.description_binary = Some(vec![0x00, 0xff, b'A', b'B', b'C', 0x7f, 0x80]);
        let rendered = render_issue(
            &row,
            Value::Array(vec![]),
            Value::Array(vec![]),
            Value::Null,
        );
        assert_eq!(
            rendered["description_binary"],
            serde_json::json!("AP9BQkN/gA==")
        );
    }

    #[test]
    fn cycle_renders_golden() {
        let g = golden("cycle");
        let row = CycleRow {
            id: uid(&g["id"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            name: g["name"].as_str().unwrap().to_owned(),
            description: g["description"].as_str().unwrap().to_owned(),
            start_date: dt_opt(&g["start_date"]),
            end_date: dt_opt(&g["end_date"]),
            view_props: g["view_props"].clone(),
            sort_order: g["sort_order"].as_f64().unwrap(),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            progress_snapshot: g["progress_snapshot"].clone(),
            archived_at: dt_opt(&g["archived_at"]),
            logo_props: g["logo_props"].clone(),
            timezone: g["timezone"].as_str().unwrap().to_owned(),
            version: g["version"].as_i64().unwrap() as i32,
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            owned_by_id: uid_opt(&g["owned_by"]),
        };
        assert_render("cycle", render_cycle(&row));
    }

    #[test]
    fn module_renders_golden_with_trailing_members() {
        let g = golden("module");
        let row = ModuleRow {
            id: uid(&g["id"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            name: g["name"].as_str().unwrap().to_owned(),
            description: g["description"].as_str().unwrap().to_owned(),
            description_text: str_opt(&g["description_text"]),
            description_html: str_opt(&g["description_html"]),
            start_date: date_opt(&g["start_date"]),
            target_date: date_opt(&g["target_date"]),
            status: g["status"].as_str().unwrap().to_owned(),
            view_props: g["view_props"].clone(),
            sort_order: g["sort_order"].as_f64().unwrap(),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            archived_at: dt_opt(&g["archived_at"]),
            logo_props: g["logo_props"].clone(),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            lead_id: uid_opt(&g["lead"]),
        };
        let members: Vec<Uuid> = g["members"].as_array().unwrap().iter().map(uid).collect();
        assert_render("module", render_module(&row, &members));
        assert_eq!(golden_keys("module").last().unwrap(), "members");
    }

    fn link_row(g: &Value, first: &str, second: &str) -> LinkRow {
        LinkRow {
            id: uid(&g["id"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            first_id: uid(&g[first]),
            second_id: uid(&g[second]),
        }
    }

    #[test]
    fn link_rows_render_goldens() {
        let cycle_issue = golden("cycle_issue");
        assert_render(
            "cycle_issue",
            render_link(&link_row(&cycle_issue, "issue", "cycle"), "issue", "cycle"),
        );
        let module_issue = golden("module_issue");
        assert_render(
            "module_issue",
            render_link(
                &link_row(&module_issue, "module", "issue"),
                "module",
                "issue",
            ),
        );
        assert!(cycle_issue.get("sub_issues_count").is_none());
    }

    #[test]
    fn comment_renders_golden_with_url() {
        let g = golden("issue_comment");
        let row = IssueCommentRow {
            id: uid(&g["id"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            comment_html: g["comment_html"].as_str().unwrap().to_owned(),
            attachments: g["attachments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_owned())
                .collect(),
            labels: g["labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap().to_owned())
                .collect(),
            access: g["access"].as_str().unwrap().to_owned(),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            speaker_type: g["speaker_type"].as_str().unwrap().to_owned(),
            speaker_label: g["speaker_label"].as_str().unwrap().to_owned(),
            speaker_agent_run_id: uid_opt(&g["speaker_agent_run_id"]),
            edited_at: dt_opt(&g["edited_at"]),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            description_id: uid_opt(&g["description"]),
            issue_id: uid(&g["issue"]),
            actor_id: uid_opt(&g["actor"]),
            parent_id: uid_opt(&g["parent"]),
        };
        assert_render(
            "issue_comment",
            render_issue_comment(&row, g["url"].as_str()),
        );
        assert!(g.get("is_member").is_none());
        assert!(g.get("comment_stripped").is_none());
        assert!(g.get("comment_json").is_none());
    }

    #[test]
    fn comment_url_omitted_when_unresolvable() {
        // QUIRK-ND-3: the key is omitted, never null.
        let g = golden("issue_comment");
        let row = IssueCommentRow {
            id: uid(&g["id"]),
            created_at: t0(),
            updated_at: t1(),
            deleted_at: None,
            comment_html: String::new(),
            attachments: vec![],
            labels: vec![],
            access: "INTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            speaker_type: "human".to_owned(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            created_by_id: None,
            updated_by_id: None,
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            description_id: None,
            issue_id: uid(&g["issue"]),
            actor_id: None,
            parent_id: None,
        };
        let rendered = render_issue_comment(&row, None);
        assert!(!rendered.contains_key("url"));
    }

    #[test]
    fn intake_issue_renders_golden_with_unexpanded_detail() {
        let g = golden("intake_issue");
        let row = IntakeIssueRow {
            id: uid(&g["id"]),
            intake_id: uid(&g["intake"]),
            issue_id: uid(&g["issue"]),
            created_at: dt(&g["created_at"]),
            updated_at: dt(&g["updated_at"]),
            deleted_at: dt_opt(&g["deleted_at"]),
            status: g["status"].as_i64().unwrap() as i32,
            snoozed_till: dt_opt(&g["snoozed_till"]),
            source: str_opt(&g["source"]),
            source_email: str_opt(&g["source_email"]),
            external_source: str_opt(&g["external_source"]),
            external_id: str_opt(&g["external_id"]),
            extra: g["extra"].clone(),
            created_by_id: uid_opt(&g["created_by"]),
            updated_by_id: uid_opt(&g["updated_by"]),
            project_id: uid(&g["project"]),
            workspace_id: uid(&g["workspace"]),
            duplicate_to_id: uid_opt(&g["duplicate_to"]),
        };
        // QUIRK-ND-5: the nested detail carries id-string lists.
        let detail = g["issue_detail"].clone();
        assert!(detail["labels"][0].is_string());
        assert!(detail["assignees"][0].is_string());
        let detail_map = detail.as_object().unwrap().clone();
        assert_render("intake_issue", render_intake_issue(&row, detail_map));
    }

    #[test]
    fn drf_datetime_matches_python_isoformat_zulu() {
        assert_eq!(drf_datetime(&t0()), "2026-01-15T10:20:30Z");
        // Python prints exactly six microsecond digits, never trimmed.
        let fractional = t0() + chrono::Duration::microseconds(100_000);
        assert_eq!(drf_datetime(&fractional), "2026-01-15T10:20:30.100000Z");
        let full = t0() + chrono::Duration::microseconds(123_456);
        assert_eq!(drf_datetime(&full), "2026-01-15T10:20:30.123456Z");
    }

    #[test]
    fn web_base_and_issue_url_rules() {
        assert_eq!(
            web_base_url(Some("http://a/"), Some("http://b")),
            Some("http://a".to_owned())
        );
        assert_eq!(
            web_base_url(Some(""), Some("http://b/")),
            Some("http://b".to_owned())
        );
        assert_eq!(web_base_url(None, None), None);
        assert_eq!(web_base_url(Some(""), Some("")), None);
        assert_eq!(
            issue_web_url(Some("http://a"), Some("ws"), Some("GLD"), Some(7)),
            Some("http://a/ws/browse/GLD-7".to_owned())
        );
        assert_eq!(issue_web_url(None, Some("ws"), Some("GLD"), Some(7)), None);
        assert_eq!(
            issue_web_url(Some("http://a"), None, Some("GLD"), Some(7)),
            None
        );
        assert_eq!(
            issue_web_url(Some("http://a"), Some("ws"), Some("GLD"), None),
            None
        );
    }

    #[test]
    fn asset_url_branches() {
        let id: Uuid = "00000001-1111-4111-8111-111111111111".parse().unwrap();
        let pid: Uuid = "00000014-1111-4111-8111-111111111111".parse().unwrap();
        let iid: Uuid = "00000028-1111-4111-8111-111111111111".parse().unwrap();
        assert_eq!(
            asset_url(&id, Some("USER_AVATAR"), None, None, None),
            Some(format!("/api/assets/v2/static/{id}/"))
        );
        assert_eq!(
            asset_url(
                &id,
                Some("ISSUE_ATTACHMENT"),
                Some("ws"),
                Some(&pid),
                Some(&iid)
            ),
            Some(format!(
                "/api/assets/v2/workspaces/ws/projects/{pid}/issues/{iid}/attachments/{id}/"
            ))
        );
        assert_eq!(
            asset_url(&id, Some("PAGE_DESCRIPTION"), Some("ws"), Some(&pid), None),
            Some(format!("/api/assets/v2/workspaces/ws/projects/{pid}/{id}/"))
        );
        assert_eq!(
            asset_url(
                &id,
                Some("DRAFT_ISSUE_ATTACHMENT"),
                Some("ws"),
                Some(&pid),
                Some(&iid)
            ),
            None
        );
        assert_eq!(asset_url(&id, None, None, None, None), None);
    }

    #[test]
    fn error_messages_match_python() {
        assert_eq!(
            ModelDataError::UnknownModel("nope".to_owned()).to_string(),
            "Model not found for event: nope"
        );
        assert!(is_not_found(&ModelDataError::NotFound(
            "issue".to_owned(),
            "x".to_owned()
        )));
        assert!(!is_not_found(&ModelDataError::UnknownModel("x".to_owned())));
        assert!(!is_not_found(&ModelDataError::InvalidId("x".to_owned())));
        assert_eq!(
            fixture()["unknown_event_error"],
            serde_json::json!("ValueError: Model not found for event: nope")
        );
    }
}
