#![forbid(unsafe_code)]
//! Bulk issue endpoints (PIDASHCONV-1677).
//!
//! Ports `BulkOperationIssuesEndpoint` and `BulkSubscribeIssuesEndpoint`
//! (`app/views/issue/bulk_operations.py`) with their serializers
//! (`app/serializers/bulk.py`). Bulk update validates the whole batch,
//! then applies inside one transaction — scalars replace, array
//! properties union — enqueueing module, cycle and issue activities;
//! bulk subscribe is idempotent and needs only project membership.
//!
//! Validation mirrors the DRF serializers byte for byte (probed live on
//! Django main): nested error shapes, the bare-string unknown-property
//! error, `UUID(int=...)` coercion for JSON ints/bools, HTML `getlist`
//! / `parse_html_dict` form semantics, and declaration-ordered field
//! errors. Every probe case is pinned in the unit tests below and in
//! `contract-tests/app_issues/test_issue_bulk.py`.

use std::collections::{BTreeMap, HashSet};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, NaiveDate, Utc};
use pidash_db::entity::{
    cycle_issues, cycles, estimate_points, issue_assignees, issue_labels, issue_subscribers,
    issues, labels, module_issues, modules, projects, states, users, workspaces,
};
use pidash_db::orm_tx::TxConn;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult,
    JoinType, QueryFilter, QueryOrder, QuerySelect, RelationTrait, RuntimeErr, Set,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::handlers_archive::{archive_allow, entity_allowed};
use super::handlers_core::parse_drf_date;
use super::{actor_user_id, json_response, resolve_project_id, Denial, HandlerResult};
use crate::app_modules::handlers_modules::enqueue_task;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_request_bytes_spans, JNum, JObject, JVal, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::response::json_response as status_response;
use pidash_services::domain::activity::ISSUE_ACTIVITY_TASK;
use pidash_services::domain::issues::{
    bulk_update_issue_scalars, insert_bulk_assignees, to_spaced_json, BulkAssigneeUnion,
    BulkIssueScalars,
};

/// Bulk path in `app/urls/issue.py:119-123` form.
pub const BULK_OPERATION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-operation-issues/";
/// Bulk path in `app/urls/issue.py:124-128` form.
pub const BULK_SUBSCRIBE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-subscribe-issues/";

/// DRF's default `PermissionDenied` body (the entity/lite denials carry
/// no custom message).
const ENTITY_DENIED_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// The view-body project miss (`bulk_operations.py:101,363`).
const PROJECT_MISSING_BODY: &str = r#"{"error":"Project not found"}"#;
/// `handle_exception`'s `IntegrityError` branch.
const PAYLOAD_INVALID_MESSAGE: &str = "The payload is not valid";

// Serializer message texts (`rest_framework/fields.py`, `serializers.py`,
// `app/serializers/bulk.py`; all probed live).
const REQUIRED_MESSAGE: &str = "This field is required.";
const NULL_MESSAGE: &str = "This field may not be null.";
const UUID_MESSAGE: &str = "Must be a valid UUID.";
const DATE_MESSAGE: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
const MIN_LENGTH_MESSAGE: &str = "Ensure this field has at least 1 elements.";
const NO_DATA_MESSAGE: &str = "No data provided";
const BULK_EDITED_MESSAGE: &str = "This property cannot be bulk-edited.";
const EMPTY_PROPERTIES_MESSAGE: &str = "Properties must name at least one bulk-editable field.";
const DATE_ORDER_MESSAGE: &str = "Start date cannot exceed target date";
const NON_FIELD_ERRORS: &str = "non_field_errors";

/// Batch-validation messages (`bulk_operations.py:113-147`).
const COMPLETED_CYCLE_MESSAGE: &str =
    "The Cycle has already been completed so no new issues can be added";

/// The bulk-editable property names, in serializer declaration order
/// (field errors render in this order).
const PROPERTY_FIELDS: [&str; 9] = [
    "state_id",
    "priority",
    "label_ids",
    "assignee_ids",
    "start_date",
    "target_date",
    "module_ids",
    "cycle_id",
    "estimate_point",
];

/// `priority` choices (`PriorityChoices`).
const PRIORITIES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];

/// Body spec for the serializer views: `issue_ids` and the nested UUID
/// lists use HTML `getlist` semantics (texts then files, indexed keys
/// assembled), every other key reads its last value. No blank-skipping:
/// an allow-null scalar keeps form `''` (which maps to `None`) and a
/// blank `properties.priority` still counts the properties dict as
/// present (the nested `validate()` then reports it empty) — both
/// probed live.
const BULK_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[
        "issue_ids",
        "properties.label_ids",
        "properties.assignee_ids",
        "properties.module_ids",
    ],
    skip_blank_fields: &[],
};

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST .../bulk-operation-issues/`: gates, serializer validation, the
/// project lookup, whole-batch reference checks, then the transactional
/// apply with module/cycle/issue activity fan-out.
pub async fn bulk_operation(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> HandlerResult {
    let base = bulk_base(&state, &slug, &project_raw, extension).await?;
    // `ProjectEntityPermission` runs in `initial()`, before the view body
    // (and its decorator), so its 403 carries DRF's default detail body.
    if !entity_allowed(&base.pool, &slug, &base.project_id, &base.user_id).await? {
        return Ok(status_response(StatusCode::FORBIDDEN, ENTITY_DENIED_BODY));
    }
    archive_allow(&base.pool, &base.slug, &base.project_id, &base.user_id).await?;
    let (data, is_html) = request_data(&headers, &body)?;
    let (ids, properties) = match validate_operation_body(&data, is_html) {
        Ok(valid) => valid,
        Err(body) => return Ok(json_response_400(body)),
    };
    let project = match fetch_project(&base.pool, &slug, &base.project_id).await? {
        Some(project) => project,
        None => {
            return Ok(status_response(StatusCode::NOT_FOUND, PROJECT_MISSING_BODY));
        }
    };
    let issues = fetch_bulk_issues(&base.pool, &slug, &base.project_id, &ids).await?;
    let batch = check_batch(&base.pool, &slug, &base.project_id, &properties, &issues).await?;
    apply_bulk_operation(&state, &base, &project, &properties, &issues, &batch).await
}

/// `POST .../bulk-subscribe-issues/`: the lite gate, id validation, the
/// project lookup, then the idempotent insert (no transaction, no
/// activity — `bulk_operations.py:345-395`).
pub async fn bulk_subscribe(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> HandlerResult {
    let base = bulk_base(&state, &slug, &project_raw, extension).await?;
    if !lite_allowed(&base.pool, &slug, &base.project_id, &base.user_id).await? {
        return Ok(status_response(StatusCode::FORBIDDEN, ENTITY_DENIED_BODY));
    }
    let (data, _) = request_data(&headers, &body)?;
    let ids = match validate_subscribe_body(&data) {
        Ok(ids) => ids,
        Err(body) => return Ok(json_response_400(body)),
    };
    let project = match fetch_project(&base.pool, &slug, &base.project_id).await? {
        Some(project) => project,
        None => {
            return Ok(status_response(StatusCode::NOT_FOUND, PROJECT_MISSING_BODY));
        }
    };
    let issues = fetch_bulk_issues(&base.pool, &slug, &base.project_id, &ids).await?;
    let fresh = subscribe_issues(&base, &project, &issues).await?;
    Ok(json_response(format!(
        "{{\"message\":\"Subscribed to {} issues\",\"subscribed\":{}}}",
        fresh.len(),
        fresh.len()
    )))
}

/// The idempotent subscribe: the selection minus the actor's live
/// subscriptions, inserted with `bulk_create(..., ignore_conflicts=True)`
/// (no transaction, no activity). Returns the fresh issue ids.
async fn subscribe_issues(
    base: &BulkBase,
    project: &ProjectScope,
    issues: &[IssueRow],
) -> Result<Vec<Uuid>, Denial> {
    let db = orm(&base.pool);
    let rows: Vec<Uuid> = issue_subscribers::Entity::find()
        .select_only()
        .column(issue_subscribers::Column::IssueId)
        .filter(issue_subscribers::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(issue_subscribers::Column::SubscriberId.eq(base.user_id))
        .filter(issue_subscribers::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let subscribed: HashSet<Uuid> = rows.into_iter().collect();
    let fresh: Vec<Uuid> = issues
        .iter()
        .map(|issue| issue.id)
        .filter(|id| !subscribed.contains(id))
        .collect();
    if !fresh.is_empty() {
        let now: DateTimeWithTimeZone = Utc::now().into();
        // `bulk_create(..., ignore_conflicts=True)`.
        for chunk in fresh.chunks(INSERT_CHUNK) {
            let rows = chunk
                .iter()
                .map(|&issue_id| issue_subscribers::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    created_by_id: Set(Some(base.user_id)),
                    updated_by_id: Set(Some(base.user_id)),
                    issue_id: Set(issue_id),
                    project_id: Set(base.project_id),
                    subscriber_id: Set(base.user_id),
                    workspace_id: Set(project.workspace_id),
                    deleted_at: Set(None),
                });
            issue_subscribers::Entity::insert_many(rows)
                .on_conflict(OnConflict::new().do_nothing().to_owned())
                .exec_without_returning(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
        }
    }
    Ok(fresh)
}

/// Rows per bridge `INSERT` statement: 9 binds a row stays well under
/// Postgres' 65,535-bind ceiling (the old `unnest` form bound two
/// arrays, so it had no ceiling to hit).
const INSERT_CHUNK: usize = 5_000;

/// A SeaORM connection over the request pool (no I/O).
fn orm(pool: &sqlx::PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// Session auth + the project-kwarg rewrite, before any permission or
/// tenant lookup.
struct BulkBase {
    pool: sqlx::PgPool,
    slug: String,
    project_id: Uuid,
    user_id: Uuid,
}

async fn bulk_base(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<BulkBase, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    Ok(BulkBase {
        pool,
        slug: slug.to_owned(),
        project_id,
        user_id,
    })
}

/// `ProjectLitePermission`: any active project membership, any role
/// (guests subscribe; outsiders 403).
async fn lite_allowed(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<bool, Denial> {
    let role = super::access::project_role(&orm(pool), slug, *project_id, *user_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(role.is_some())
}

/// The view-body project lookup (`bulk_operations.py:99-101,361-363`):
/// soft-deleted projects miss even when the membership passed the gate.
struct ProjectScope {
    workspace_id: Uuid,
}

async fn fetch_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Option<ProjectScope>, Denial> {
    let workspace_id: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::WorkspaceId)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(projects::Column::Id.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(workspace_id.map(|workspace_id| ProjectScope { workspace_id }))
}

/// The bulk selection: live issues in the tenant (`Issue.objects` —
/// drafts, archived and triage rows included), model `-created_at` order.
#[derive(Debug, FromQueryResult)]
struct IssueRow {
    id: Uuid,
    state_id: Option<Uuid>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    estimate_point_id: Option<Uuid>,
}

fn bulk_issues_query(
    slug: &str,
    project_id: Uuid,
    ids: &[Uuid],
) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::StateId,
            issues::Column::Priority,
            issues::Column::StartDate,
            issues::Column::TargetDate,
            issues::Column::EstimatePointId,
        ])
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .filter(issues::Column::DeletedAt.is_null())
        .order_by_desc(issues::Column::CreatedAt)
}

async fn fetch_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<IssueRow>, Denial> {
    bulk_issues_query(slug, *project_id, ids)
        .into_model::<IssueRow>()
        .all(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)
}

/// A 400 with a pre-rendered JSON body.
fn json_response_400(body: String) -> axum::response::Response {
    status_response(StatusCode::BAD_REQUEST, &body)
}

// ---------------------------------------------------------------------------
// `request.data`: JSON and HTML inputs as one owned value
// ---------------------------------------------------------------------------

/// A request-data value: JSON values convert losslessly, form values map
/// through DRF's HTML rules (`getlist`, `parse_html_dict`, last-wins
/// scalars). Files stay marked because they fail UUID/date fields no
/// matter their name while echoing the filename in choice errors
/// (`str(UploadedFile)`, probed live).
#[derive(Debug, Clone, PartialEq)]
enum DVal {
    Null,
    Bool(bool),
    Num(JNum),
    Str(String),
    List(Vec<DVal>),
    Dict(BTreeMap<String, DVal>),
    File { filename: String },
}

impl DVal {
    /// The Python `type(x).__name__` for `invalid`/`not_a_list`
    /// messages. Files never reach those positions (they only occupy
    /// scalar slots and list items), so the arm is unreachable.
    fn type_name(&self) -> &'static str {
        match self {
            DVal::Null => "NoneType",
            DVal::Bool(_) => "bool",
            DVal::Num(number) => {
                if number.is_float() {
                    "float"
                } else {
                    "int"
                }
            }
            DVal::Str(_) => "str",
            DVal::List(_) => "list",
            DVal::Dict(_) => "dict",
            DVal::File { .. } => "str",
        }
    }

    /// Python `str()` for choice-error echoes. Nested strings render
    /// with single quotes (probed: `"['high']"`, `"{'a': 1}"`); strings
    /// needing quote-switching or backslash escapes (no real client
    /// sends them) take the simple single-quoted shape.
    fn py_str(&self) -> String {
        match self {
            DVal::Null => "None".to_owned(),
            DVal::Bool(true) => "True".to_owned(),
            DVal::Bool(false) => "False".to_owned(),
            DVal::Num(number) => number.py_string(),
            DVal::Str(text) => text.clone(),
            DVal::List(items) => {
                let inner: Vec<String> = items.iter().map(DVal::py_repr).collect();
                format!("[{}]", inner.join(", "))
            }
            DVal::Dict(map) => {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(key, value)| format!("{}: {}", Self::py_repr_str(key), value.py_repr()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            DVal::File { filename } => filename.clone(),
        }
    }

    /// Python `repr()` for values nested inside a `str()` container.
    fn py_repr(&self) -> String {
        match self {
            DVal::Str(text) => Self::py_repr_str(text),
            DVal::List(_) | DVal::Dict(_) => self.py_str(),
            other => other.py_str(),
        }
    }

    fn py_repr_str(text: &str) -> String {
        format!("'{text}'")
    }
}

/// `request.data` through the shared negotiator plus the `bool` the
/// blank rules need (form `''` maps to `None`/skip; JSON `''` runs the
/// field parser and 400s).
fn request_data(headers: &HeaderMap, body: &axum::body::Bytes) -> Result<(DVal, bool), Denial> {
    match shared_body::negotiate_body(headers, body, &BULK_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok((DVal::Dict(BTreeMap::new()), false)),
        Ok(shared_body::NegotiatedBody::JsonText { text, surr }) => {
            match parse_request_bytes_spans(text.as_bytes(), &surr) {
                Ok(value) => Ok((dval_from_jval(&value), false)),
                Err(JsonFail::Message(reason)) => {
                    Err(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{reason}")))
                }
                Err(JsonFail::Recursion) => Err(Denial::ServerError),
            }
        }
        Ok(shared_body::NegotiatedBody::Form {
            map,
            files,
            surr: _,
        }) => Ok((dval_from_form(&map, &files), true)),
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

fn dval_from_jval(value: &JVal) -> DVal {
    match value {
        JVal::Null => DVal::Null,
        JVal::Bool(flag) => DVal::Bool(*flag),
        JVal::Num(number) => DVal::Num(number.clone()),
        JVal::Str(text) => DVal::Str(text.to_lossy_string()),
        JVal::Array(items) => DVal::List(items.iter().map(dval_from_jval).collect()),
        JVal::Object(object) => DVal::Dict(dict_from_jobject(object)),
    }
}

fn dict_from_jobject(object: &JObject) -> BTreeMap<String, DVal> {
    object
        .iter()
        .map(|(key, value)| (key.to_lossy_string(), dval_from_jval(value)))
        .collect()
}

/// The HTML-input shape: `issue_ids` reads `getlist` (texts then files);
/// `properties` collects `properties.`-prefixed keys (`parse_html_dict` —
/// a flat `properties` key is ignored, an empty collection is the
/// required-400); nested lists read `getlist`, nested scalars read the
/// last value with files winning over texts.
fn dval_from_form(map: &Map<String, Value>, files: &shared_body::FilesMap) -> DVal {
    const PREFIX: &str = "properties.";
    let mut top = BTreeMap::new();
    let mut ids: Vec<DVal> = Vec::new();
    let mut ids_present = false;
    if let Some(value) = map.get("issue_ids") {
        ids_present = true;
        push_text_list_value(&mut ids, value);
    }
    if let Some(parts) = files.get("issue_ids") {
        ids_present = true;
        ids.extend(parts.iter().map(|part| DVal::File {
            filename: part.filename.clone(),
        }));
    }
    if ids_present {
        top.insert("issue_ids".to_owned(), DVal::List(ids));
    }
    let mut sub = BTreeMap::new();
    let mut dotted: BTreeMap<String, (Option<&Value>, &[shared_body::FilePart])> = BTreeMap::new();
    for (key, value) in map {
        if let Some(name) = key.strip_prefix(PREFIX) {
            dotted.insert(name.to_owned(), (Some(value), &[]));
        }
    }
    for (key, parts) in files {
        if let Some(name) = key.strip_prefix(PREFIX) {
            dotted
                .entry(name.to_owned())
                .and_modify(|entry| entry.1 = parts.as_slice())
                .or_insert((None, parts.as_slice()));
        }
    }
    for (name, (text, parts)) in &dotted {
        if matches!(name.as_str(), "label_ids" | "assignee_ids" | "module_ids") {
            let mut items: Vec<DVal> = Vec::new();
            if let Some(value) = text {
                push_text_list_value(&mut items, value);
            }
            items.extend(parts.iter().map(|part| DVal::File {
                filename: part.filename.clone(),
            }));
            sub.insert(name.clone(), DVal::List(items));
        } else if let Some(last) = parts.last() {
            sub.insert(
                name.clone(),
                DVal::File {
                    filename: last.filename.clone(),
                },
            );
        } else if let Some(value) = text {
            sub.insert(name.clone(), dval_from_text_value(value));
        }
    }
    if !sub.is_empty() {
        top.insert("properties".to_owned(), DVal::Dict(sub));
    }
    DVal::Dict(top)
}

/// One form text value as a scalar (single texts arrive verbatim; a lone
/// array member is defensive — the spec projects only list fields).
fn dval_from_text_value(value: &Value) -> DVal {
    match value {
        Value::String(text) => DVal::Str(text.clone()),
        Value::Array(items) => DVal::List(items.iter().map(dval_from_text_value).collect()),
        Value::Null => DVal::Null,
        Value::Bool(flag) => DVal::Bool(*flag),
        Value::Number(_) => DVal::Str(value.to_string()),
        Value::Object(_) => DVal::Str(value.to_string()),
    }
}

/// Append form texts that belong in a `getlist` list.
fn push_text_list_value(out: &mut Vec<DVal>, value: &Value) {
    match value {
        Value::Array(items) => out.extend(items.iter().map(dval_from_text_value)),
        scalar => out.push(dval_from_text_value(scalar)),
    }
}

// ---------------------------------------------------------------------------
// Serializer validation (`app/serializers/bulk.py`)
// ---------------------------------------------------------------------------

/// Validated `properties`: `Some` marks the key present, the inner
/// `Option` carries explicit nulls (clears) for the nullable fields.
#[derive(Debug, Default, PartialEq)]
struct BulkProperties {
    state_id: Option<Option<Uuid>>,
    priority: Option<String>,
    label_ids: Option<Vec<Uuid>>,
    assignee_ids: Option<Vec<Uuid>>,
    start_date: Option<Option<NaiveDate>>,
    target_date: Option<Option<NaiveDate>>,
    module_ids: Option<Vec<Uuid>>,
    cycle_id: Option<Option<Uuid>>,
    estimate_point: Option<Option<Uuid>>,
}

impl BulkProperties {
    fn is_empty(&self) -> bool {
        self.state_id.is_none()
            && self.priority.is_none()
            && self.label_ids.is_none()
            && self.assignee_ids.is_none()
            && self.start_date.is_none()
            && self.target_date.is_none()
            && self.module_ids.is_none()
            && self.cycle_id.is_none()
            && self.estimate_point.is_none()
    }
}

fn single_error(message: &str) -> Value {
    Value::Array(vec![Value::String(message.to_owned())])
}

fn non_field_errors(message: String) -> Value {
    let mut map = Map::new();
    map.insert(NON_FIELD_ERRORS.to_owned(), single_error(&message));
    Value::Object(map)
}

fn invalid_data_body(type_name: &str) -> String {
    let mut map = Map::new();
    map.insert(
        NON_FIELD_ERRORS.to_owned(),
        single_error(&format!(
            "Invalid data. Expected a dictionary, but got {type_name}."
        )),
    );
    Value::Object(map).to_string()
}

/// The top-level bulk-operation body: `issue_ids` then `properties`, in
/// serializer field order.
fn validate_operation_body(
    data: &DVal,
    is_html: bool,
) -> Result<(Vec<Uuid>, BulkProperties), String> {
    match data {
        DVal::Null => Err(non_field_errors(NO_DATA_MESSAGE.to_owned()).to_string()),
        DVal::Dict(map) => {
            let mut errors = Map::new();
            let ids = match validate_ids_field(map.get("issue_ids")) {
                Ok(ids) => Some(ids),
                Err(error) => {
                    errors.insert("issue_ids".to_owned(), error);
                    None
                }
            };
            let properties = match validate_properties_field(map.get("properties"), is_html) {
                Ok(properties) => Some(properties),
                Err(error) => {
                    errors.insert("properties".to_owned(), error);
                    None
                }
            };
            match (ids, properties) {
                (Some(ids), Some(properties)) if errors.is_empty() => Ok((ids, properties)),
                _ => Err(Value::Object(errors).to_string()),
            }
        }
        other => Err(invalid_data_body(other.type_name())),
    }
}

/// The subscribe body carries only `issue_ids` (`BulkSubscribeSerializer`).
fn validate_subscribe_body(data: &DVal) -> Result<Vec<Uuid>, String> {
    match data {
        DVal::Null => Err(non_field_errors(NO_DATA_MESSAGE.to_owned()).to_string()),
        DVal::Dict(map) => match validate_ids_field(map.get("issue_ids")) {
            Ok(ids) => Ok(ids),
            Err(error) => {
                let mut errors = Map::new();
                errors.insert("issue_ids".to_owned(), error);
                Err(Value::Object(errors).to_string())
            }
        },
        other => Err(invalid_data_body(other.type_name())),
    }
}

/// `issue_ids = ListField(child=UUIDField(), min_length=1)`.
fn validate_ids_field(value: Option<&DVal>) -> Result<Vec<Uuid>, Value> {
    let Some(value) = value else {
        return Err(single_error(REQUIRED_MESSAGE));
    };
    match value {
        DVal::Null => Err(single_error(NULL_MESSAGE)),
        DVal::List(items) => {
            if items.is_empty() {
                return Err(single_error(MIN_LENGTH_MESSAGE));
            }
            let mut errors = Map::new();
            let mut ids = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                match parse_uuid_item(item) {
                    Ok(id) => ids.push(id),
                    Err(message) => {
                        errors.insert(index.to_string(), single_error(&message));
                    }
                }
            }
            if errors.is_empty() {
                Ok(ids)
            } else {
                Err(Value::Object(errors))
            }
        }
        other => Err(single_error(&format!(
            "Expected a list of items but got type \"{}\".",
            other.type_name()
        ))),
    }
}

/// A UUID list item: strings parse as hex (all four spellings), JSON
/// ints/bools fold through `UUID(int=...)`, null reports separately.
fn parse_uuid_item(item: &DVal) -> Result<Uuid, String> {
    match item {
        DVal::Null => Err(NULL_MESSAGE.to_owned()),
        DVal::Str(text) => Uuid::parse_str(text).map_err(|_| UUID_MESSAGE.to_owned()),
        DVal::Bool(flag) => Ok(Uuid::from_u128(u128::from(*flag))),
        DVal::Num(number) => uuid_from_json_int(number).ok_or_else(|| UUID_MESSAGE.to_owned()),
        DVal::List(_) | DVal::Dict(_) | DVal::File { .. } => Err(UUID_MESSAGE.to_owned()),
    }
}

/// `UUID(int=n)` for JSON numbers: plain ints in `u128` range (plus the
/// `-0` spelling, which folds to zero); floats never coerce.
fn uuid_from_json_int(number: &JNum) -> Option<Uuid> {
    if number.is_float() {
        return None;
    }
    if number.text() == "-0" {
        return Some(Uuid::from_u128(0));
    }
    number.to_u128().map(Uuid::from_u128)
}

/// The nested `properties` serializer: unknown keys first (bare-string
/// errors, sorted), then fields in declaration order, then
/// `validate()` (non-empty, serializer-level date order).
fn validate_properties_field(value: Option<&DVal>, is_html: bool) -> Result<BulkProperties, Value> {
    let Some(value) = value else {
        return Err(single_error(REQUIRED_MESSAGE));
    };
    match value {
        DVal::Null => Err(single_error(NULL_MESSAGE)),
        DVal::Dict(map) => {
            let unknown: Vec<&String> = map
                .keys()
                .filter(|key| !PROPERTY_FIELDS.contains(&key.as_str()))
                .collect();
            if !unknown.is_empty() {
                let mut errors = Map::new();
                for key in unknown {
                    errors.insert(
                        (*key).clone(),
                        Value::String(BULK_EDITED_MESSAGE.to_owned()),
                    );
                }
                return Err(Value::Object(errors));
            }
            let mut errors = Map::new();
            let mut properties = BulkProperties::default();
            if let Some(value) = map.get("state_id") {
                match parse_uuid_field(value, is_html) {
                    Ok(parsed) => properties.state_id = Some(parsed),
                    Err(error) => {
                        errors.insert("state_id".to_owned(), error);
                    }
                }
            }
            if let Some(value) = map.get("priority") {
                match parse_priority_field(value, is_html) {
                    Ok(parsed) => {
                        if let Some(priority) = parsed {
                            properties.priority = Some(priority);
                        }
                    }
                    Err(error) => {
                        errors.insert("priority".to_owned(), error);
                    }
                }
            }
            for name in ["label_ids", "assignee_ids", "module_ids"] {
                if let Some(value) = map.get(name) {
                    match parse_uuid_list(value) {
                        Ok(parsed) => match name {
                            "label_ids" => properties.label_ids = Some(parsed),
                            "assignee_ids" => properties.assignee_ids = Some(parsed),
                            _ => properties.module_ids = Some(parsed),
                        },
                        Err(error) => {
                            errors.insert(name.to_owned(), error);
                        }
                    }
                }
            }
            if let Some(value) = map.get("start_date") {
                match parse_date_field(value, is_html) {
                    Ok(parsed) => properties.start_date = Some(parsed),
                    Err(error) => {
                        errors.insert("start_date".to_owned(), error);
                    }
                }
            }
            if let Some(value) = map.get("target_date") {
                match parse_date_field(value, is_html) {
                    Ok(parsed) => properties.target_date = Some(parsed),
                    Err(error) => {
                        errors.insert("target_date".to_owned(), error);
                    }
                }
            }
            if let Some(value) = map.get("cycle_id") {
                match parse_uuid_field(value, is_html) {
                    Ok(parsed) => properties.cycle_id = Some(parsed),
                    Err(error) => {
                        errors.insert("cycle_id".to_owned(), error);
                    }
                }
            }
            if let Some(value) = map.get("estimate_point") {
                match parse_uuid_field(value, is_html) {
                    Ok(parsed) => properties.estimate_point = Some(parsed),
                    Err(error) => {
                        errors.insert("estimate_point".to_owned(), error);
                    }
                }
            }
            if !errors.is_empty() {
                return Err(Value::Object(errors));
            }
            if properties.is_empty() {
                return Err(non_field_errors(EMPTY_PROPERTIES_MESSAGE.to_owned()));
            }
            if let (Some(Some(start)), Some(Some(target))) =
                (properties.start_date, properties.target_date)
            {
                if start > target {
                    return Err(non_field_errors(DATE_ORDER_MESSAGE.to_owned()));
                }
            }
            Ok(properties)
        }
        other => Err(non_field_errors(format!(
            "Invalid data. Expected a dictionary, but got {}.",
            other.type_name()
        ))),
    }
}

/// A nullable UUID property: form blanks clear (`allow_null`), JSON
/// blanks run the parser and 400.
fn parse_uuid_field(value: &DVal, is_html: bool) -> Result<Option<Uuid>, Value> {
    match value {
        DVal::Null => Ok(None),
        DVal::Str(text) if is_html && text.is_empty() => Ok(None),
        DVal::Str(text) => Uuid::parse_str(text)
            .map(Some)
            .map_err(|_| single_error(UUID_MESSAGE)),
        DVal::Bool(flag) => Ok(Some(Uuid::from_u128(u128::from(*flag)))),
        DVal::Num(number) => uuid_from_json_int(number)
            .map(Some)
            .ok_or_else(|| single_error(UUID_MESSAGE)),
        DVal::List(_) | DVal::Dict(_) | DVal::File { .. } => Err(single_error(UUID_MESSAGE)),
    }
}

/// A nullable date property (`DateField`, default ISO input formats).
fn parse_date_field(value: &DVal, is_html: bool) -> Result<Option<NaiveDate>, Value> {
    match value {
        DVal::Null => Ok(None),
        DVal::Str(text) if is_html && text.is_empty() => Ok(None),
        DVal::Str(text) => parse_drf_date(text)
            .map(Some)
            .ok_or_else(|| single_error(DATE_MESSAGE)),
        _ => Err(single_error(DATE_MESSAGE)),
    }
}

/// The priority choice: form blanks skip the field (not required, no
/// `allow_blank`); anything else echoes Python-`str()` on mismatch.
fn parse_priority_field(value: &DVal, is_html: bool) -> Result<Option<String>, Value> {
    match value {
        DVal::Null => Err(single_error(NULL_MESSAGE)),
        DVal::Str(text) if is_html && text.is_empty() => Ok(None),
        DVal::Str(text) => {
            if PRIORITIES.contains(&text.as_str()) {
                Ok(Some(text.clone()))
            } else {
                Err(single_error(&format!("\"{text}\" is not a valid choice.")))
            }
        }
        other => Err(single_error(&format!(
            "\"{}\" is not a valid choice.",
            other.py_str()
        ))),
    }
}

/// A UUID list property (no `min_length`, so `[]` validates and no-ops).
fn parse_uuid_list(value: &DVal) -> Result<Vec<Uuid>, Value> {
    match value {
        DVal::Null => Err(single_error(NULL_MESSAGE)),
        DVal::List(items) => {
            let mut errors = Map::new();
            let mut ids = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                match parse_uuid_item(item) {
                    Ok(id) => ids.push(id),
                    Err(message) => {
                        errors.insert(index.to_string(), single_error(&message));
                    }
                }
            }
            if errors.is_empty() {
                Ok(ids)
            } else {
                Err(Value::Object(errors))
            }
        }
        other => Err(single_error(&format!(
            "Expected a list of items but got type \"{}\".",
            other.type_name()
        ))),
    }
}

// ---------------------------------------------------------------------------
// Whole-batch reference checks (`bulk_operations.py:105-147`)
// ---------------------------------------------------------------------------

/// The resolved batch: the target cycle row when `cycle_id` names one.
struct BatchCheck {
    cycle: Option<BatchCycle>,
}

struct BatchCycle {
    id: Uuid,
}

/// Every reference resolves before anything writes (in view order: state,
/// labels, assignees, modules, estimate, cycle, merged dates).
async fn check_batch(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    properties: &BulkProperties,
    issues: &[IssueRow],
) -> Result<BatchCheck, Denial> {
    let db = orm(pool);
    let server = |_: DbErr| Denial::ServerError;
    if let Some(Some(state_id)) = properties.state_id {
        // `all_state_objects`: soft-deleted states count (no filter).
        let found: Option<Uuid> = states::Entity::find()
            .select_only()
            .column(states::Column::Id)
            .filter(states::Column::Id.eq(state_id))
            .into_tuple()
            .one(&db)
            .await
            .map_err(server)?;
        if found.is_none() {
            return Err(Denial::BadError(format!("State {state_id} does not exist")));
        }
    }
    if let Some(label_ids) = properties
        .label_ids
        .as_deref()
        .filter(|ids| !ids.is_empty())
    {
        let found: Vec<Uuid> = labels::Entity::find()
            .select_only()
            .column(labels::Column::Id)
            .filter(labels::Column::Id.is_in(label_ids.iter().copied()))
            .filter(labels::Column::DeletedAt.is_null())
            .into_tuple()
            .all(&db)
            .await
            .map_err(server)?;
        missing_ids_error(label_ids, &found, "Labels")?;
    }
    if let Some(assignee_ids) = properties
        .assignee_ids
        .as_deref()
        .filter(|ids| !ids.is_empty())
    {
        // `User.objects` is the stock manager: no soft-delete filter.
        let found: Vec<Uuid> = users::Entity::find()
            .select_only()
            .column(users::Column::Id)
            .filter(users::Column::Id.is_in(assignee_ids.iter().copied()))
            .into_tuple()
            .all(&db)
            .await
            .map_err(server)?;
        missing_ids_error(assignee_ids, &found, "Users")?;
    }
    if let Some(module_ids) = properties
        .module_ids
        .as_deref()
        .filter(|ids| !ids.is_empty())
    {
        let found: Vec<Uuid> = modules::Entity::find()
            .select_only()
            .column(modules::Column::Id)
            .filter(modules::Column::Id.is_in(module_ids.iter().copied()))
            .filter(modules::Column::DeletedAt.is_null())
            .into_tuple()
            .all(&db)
            .await
            .map_err(server)?;
        missing_ids_error(module_ids, &found, "Modules")?;
    }
    if let Some(Some(point_id)) = properties.estimate_point {
        let found: Option<Uuid> = estimate_points::Entity::find()
            .select_only()
            .column(estimate_points::Column::Id)
            .filter(estimate_points::Column::Id.eq(point_id))
            .filter(estimate_points::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(server)?;
        if found.is_none() {
            return Err(Denial::BadError(format!(
                "Estimate point {point_id} does not exist"
            )));
        }
    }
    let mut batch = BatchCheck { cycle: None };
    if let Some(Some(cycle_id)) = properties.cycle_id {
        let row: Option<(Uuid, Option<DateTimeWithTimeZone>)> = cycles::Entity::find()
            .select_only()
            .columns([cycles::Column::Id, cycles::Column::EndDate])
            .join(JoinType::InnerJoin, cycles::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(cycles::Column::ProjectId.eq(*project_id))
            .filter(cycles::Column::Id.eq(cycle_id))
            .filter(cycles::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(server)?;
        let Some((id, end_date)) = row else {
            return Err(Denial::BadError(format!(
                "Cycle {cycle_id} does not exist in this project"
            )));
        };
        if let Some(end) = end_date {
            if end.with_timezone(&Utc) < Utc::now() {
                return Err(Denial::BadError(COMPLETED_CYCLE_MESSAGE.to_owned()));
            }
        }
        batch.cycle = Some(BatchCycle { id });
    }
    for issue in issues {
        let start = properties.start_date.unwrap_or(issue.start_date);
        let target = properties.target_date.unwrap_or(issue.target_date);
        if let (Some(start), Some(target)) = (start, target) {
            if start > target {
                return Err(Denial::BadError(DATE_ORDER_MESSAGE.to_owned()));
            }
        }
    }
    Ok(batch)
}

/// The `_missing` helper: referenced ids with no `found` row, sorted as
/// strings in the error.
fn missing_ids_error(ids: &[Uuid], found: &[Uuid], label: &str) -> Result<(), Denial> {
    let found: HashSet<&Uuid> = found.iter().collect();
    let mut missing: Vec<String> = ids
        .iter()
        .filter(|id| !found.contains(id))
        .map(ToString::to_string)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort();
    Err(Denial::BadError(format!(
        "{label} do not exist: {}",
        missing.join(", ")
    )))
}

// ---------------------------------------------------------------------------
// Transactional apply (`bulk_operations.py:149-342`)
// ---------------------------------------------------------------------------

/// Map a write failure: constraint violations are the invalid-payload
/// 400 (`handle_exception`'s `IntegrityError` branch); everything else
/// 500s. Callers must let the transaction roll back (early return).
fn write_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(database) = &error {
        if let Some(code) = database.code() {
            if code == "23505" || code == "23503" {
                return Denial::BadError(PAYLOAD_INVALID_MESSAGE.to_owned());
            }
        }
    }
    Denial::ServerError
}

/// [`write_denial`] for a SeaORM failure: the wrapped sqlx error decides.
fn orm_write_denial(error: DbErr) -> Denial {
    match error {
        DbErr::Exec(RuntimeErr::SqlxError(inner)) | DbErr::Query(RuntimeErr::SqlxError(inner)) => {
            write_denial(inner)
        }
        _ => Denial::ServerError,
    }
}

/// One shared `now` for the row writes (the view's `timezone.now()`);
/// the epoch shares it (whole seconds either way).
async fn apply_bulk_operation(
    state: &AppState,
    base: &BulkBase,
    project: &ProjectScope,
    properties: &BulkProperties,
    issues: &[IssueRow],
    batch: &BatchCheck,
) -> HandlerResult {
    let now = Utc::now();
    let epoch = now.timestamp();
    // The origin resolves only when an activity can fire: the per-issue
    // loop runs for any non-empty selection, and a cycle set fires even
    // for an empty one (Django resolves `base_host` per enqueue, so a
    // run with no enqueues never 500s on empty URLs).
    let cycle_set = batch.cycle.as_ref().map(|cycle| cycle.id);
    let origin = if !issues.is_empty() || cycle_set.is_some() {
        Some(activity_origin(state)?)
    } else {
        None
    };
    let mut txn = base.pool.begin().await.map_err(|_| Denial::ServerError)?;
    if !issues.is_empty() {
        for issue in issues {
            bulk_update_issue_scalars(
                &pidash_db::orm_tx::TxConn::new(&mut txn),
                &BulkIssueScalars {
                    id: issue.id,
                    state_id: properties.state_id.unwrap_or(issue.state_id),
                    priority: properties
                        .priority
                        .clone()
                        .unwrap_or_else(|| issue.priority.clone()),
                    start_date: properties.start_date.unwrap_or(issue.start_date),
                    target_date: properties.target_date.unwrap_or(issue.target_date),
                    estimate_point_id: properties.estimate_point.unwrap_or(issue.estimate_point_id),
                },
                now,
            )
            .await
            .map_err(write_denial)?;
        }
    }
    if let Some(label_ids) = &properties.label_ids {
        if !label_ids.is_empty() {
            union_labels(
                &mut txn,
                &base.project_id,
                &project.workspace_id,
                &base.user_id,
                now,
                issues,
                label_ids,
            )
            .await?;
        }
    }
    if let Some(assignee_ids) = &properties.assignee_ids {
        if !assignee_ids.is_empty() {
            union_assignees(
                &mut txn,
                &base.project_id,
                &project.workspace_id,
                &base.user_id,
                now,
                issues,
                assignee_ids,
            )
            .await?;
        }
    }
    if let Some(module_ids) = &properties.module_ids {
        if !module_ids.is_empty() {
            let pairs = union_module_bridges(
                &mut txn,
                &base.project_id,
                &project.workspace_id,
                &base.user_id,
                now,
                issues,
                module_ids,
            )
            .await?;
            if let Some(origin) = &origin {
                for (issue_id, module_id) in &pairs {
                    let mut requested = Map::new();
                    requested.insert("module_id".to_owned(), Value::String(module_id.to_string()));
                    enqueue_task(
                        &base.pool,
                        ISSUE_ACTIVITY_TASK,
                        activity_kwargs(
                            "module.activity.created",
                            &base.user_id,
                            Some(issue_id),
                            &base.project_id,
                            epoch,
                            origin,
                            &to_spaced_json(&requested).ok_or(Denial::ServerError)?,
                            &Value::Null,
                        ),
                    )
                    .await;
                }
            }
        }
    }
    if let Some(cycle_opt) = properties.cycle_id {
        match cycle_opt {
            None => {
                if let Some(origin) = &origin {
                    for issue in issues {
                        let mut requested = Map::new();
                        requested.insert("cycle_id".to_owned(), Value::Null);
                        requested.insert(
                            "issues".to_owned(),
                            Value::Array(vec![Value::String(issue.id.to_string())]),
                        );
                        enqueue_task(
                            &base.pool,
                            ISSUE_ACTIVITY_TASK,
                            activity_kwargs(
                                "cycle.activity.deleted",
                                &base.user_id,
                                Some(&issue.id),
                                &base.project_id,
                                epoch,
                                origin,
                                &to_spaced_json(&requested).ok_or(Denial::ServerError)?,
                                &Value::Null,
                            ),
                        )
                        .await;
                    }
                }
                clear_cycle_bridges(&mut txn, issues, now).await?;
            }
            Some(_) => {
                let Some(target) = cycle_set else {
                    return Err(Denial::ServerError);
                };
                apply_cycle_set(
                    &base.pool,
                    &mut txn,
                    &base.project_id,
                    &project.workspace_id,
                    &base.user_id,
                    epoch,
                    origin.as_deref().ok_or(Denial::ServerError)?,
                    issues,
                    &target,
                )
                .await?;
            }
        }
    }
    if let Some(origin) = &origin {
        for issue in issues {
            let applied = applied_json(properties, cycle_set);
            let previous = previous_json(properties, issue);
            enqueue_task(
                &base.pool,
                ISSUE_ACTIVITY_TASK,
                activity_kwargs(
                    "issue.activity.updated",
                    &base.user_id,
                    Some(&issue.id),
                    &base.project_id,
                    epoch,
                    origin,
                    &to_spaced_json(&applied).ok_or(Denial::ServerError)?,
                    &Value::String(to_spaced_json(&previous).ok_or(Denial::ServerError)?),
                ),
            )
            .await;
        }
    }
    txn.commit().await.map_err(write_denial)?;
    Ok(json_response(format!(
        "{{\"message\":\"{} issues were updated\",\"updated\":{}}}",
        issues.len(),
        issues.len()
    )))
}

/// The union's new `(issue_id, other_id)` pairs in enqueue order (issues
/// outer, ids inner): every pair not already live, each once.
fn union_pairs(
    existing: Vec<(Uuid, Uuid)>,
    issues: &[IssueRow],
    ids: &[Uuid],
) -> Vec<(Uuid, Uuid)> {
    let mut seen: HashSet<(Uuid, Uuid)> = existing.into_iter().collect();
    let mut pairs: Vec<(Uuid, Uuid)> = Vec::new();
    for issue in issues {
        for id in ids {
            if seen.insert((issue.id, *id)) {
                pairs.push((issue.id, *id));
            }
        }
    }
    pairs
}

/// Union `label` bridge rows onto the selection (the view's `live` set,
/// membership only), `bulk_create(..., ignore_conflicts=True)`.
#[allow(clippy::too_many_arguments)]
async fn union_labels(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    now: DateTime<Utc>,
    issues: &[IssueRow],
    ids: &[Uuid],
) -> Result<(), Denial> {
    let conn = TxConn::new(txn);
    let existing: Vec<(Uuid, Uuid)> = issue_labels::Entity::find()
        .select_only()
        .columns([issue_labels::Column::IssueId, issue_labels::Column::LabelId])
        .filter(issue_labels::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(issue_labels::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    let pairs = union_pairs(existing, issues, ids);
    let now: DateTimeWithTimeZone = now.into();
    for chunk in pairs.chunks(INSERT_CHUNK) {
        let rows = chunk
            .iter()
            .map(|&(issue_id, label_id)| issue_labels::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(now),
                updated_at: Set(now),
                created_by_id: Set(Some(*user_id)),
                updated_by_id: Set(Some(*user_id)),
                issue_id: Set(issue_id),
                label_id: Set(label_id),
                project_id: Set(*project_id),
                workspace_id: Set(*workspace_id),
                deleted_at: Set(None),
            });
        issue_labels::Entity::insert_many(rows)
            .on_conflict(OnConflict::new().do_nothing().to_owned())
            .exec_without_returning(&conn)
            .await
            .map_err(orm_write_denial)?;
    }
    Ok(())
}

/// Union `assignee` bridge rows onto the selection: the live read here,
/// the write through `domain::issues::insert_bulk_assignees`
/// (`issue_assignees` is a design §4 table).
#[allow(clippy::too_many_arguments)]
async fn union_assignees(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    now: DateTime<Utc>,
    issues: &[IssueRow],
    ids: &[Uuid],
) -> Result<(), Denial> {
    let conn = TxConn::new(txn);
    let existing: Vec<(Uuid, Uuid)> = issue_assignees::Entity::find()
        .select_only()
        .columns([
            issue_assignees::Column::IssueId,
            issue_assignees::Column::AssigneeId,
        ])
        .filter(issue_assignees::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    let pairs = union_pairs(existing, issues, ids);
    if !pairs.is_empty() {
        insert_bulk_assignees(
            &conn,
            &BulkAssigneeUnion {
                pairs: &pairs,
                project_id: *project_id,
                workspace_id: *workspace_id,
                actor_id: *user_id,
                now,
            },
        )
        .await
        .map_err(orm_write_denial)?;
    }
    Ok(())
}

/// A module-union run: the added pairs in enqueue order (issues outer,
/// modules inner).
#[allow(clippy::too_many_arguments)]
async fn union_module_bridges(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    now: DateTime<Utc>,
    issues: &[IssueRow],
    module_ids: &[Uuid],
) -> Result<Vec<(Uuid, Uuid)>, Denial> {
    let conn = TxConn::new(txn);
    let existing: Vec<(Uuid, Uuid)> = module_issues::Entity::find()
        .select_only()
        .columns([
            module_issues::Column::IssueId,
            module_issues::Column::ModuleId,
        ])
        .filter(module_issues::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(module_issues::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    let pairs = union_pairs(existing, issues, module_ids);
    let now: DateTimeWithTimeZone = now.into();
    for chunk in pairs.chunks(INSERT_CHUNK) {
        let rows = chunk
            .iter()
            .map(|&(issue_id, module_id)| module_issues::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(now),
                updated_at: Set(now),
                created_by_id: Set(Some(*user_id)),
                updated_by_id: Set(Some(*user_id)),
                issue_id: Set(issue_id),
                module_id: Set(module_id),
                project_id: Set(*project_id),
                workspace_id: Set(*workspace_id),
                deleted_at: Set(None),
            });
        module_issues::Entity::insert_many(rows)
            .on_conflict(OnConflict::new().do_nothing().to_owned())
            .exec_without_returning(&conn)
            .await
            .map_err(orm_write_denial)?;
    }
    Ok(pairs)
}

/// The cycle-set leg: live rows pointing elsewhere move (a `cycle_id`-only
/// update — `bulk_update` skips `auto_now`), rows missing entirely insert
/// one per issue with per-row timestamps, and one `cycle.activity.created`
/// carries both lists.
#[allow(clippy::too_many_arguments)]
async fn apply_cycle_set(
    pool: &sqlx::PgPool,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    epoch: i64,
    origin: &str,
    issues: &[IssueRow],
    target: &Uuid,
) -> Result<(), Denial> {
    let (created, moved) =
        write_cycle_set(txn, project_id, workspace_id, user_id, issues, target).await?;
    let moved_activity: Vec<Value> = moved
        .iter()
        .map(|(_, issue_id, old_cycle_id)| moved_bridge_json(old_cycle_id, target, issue_id))
        .collect();
    let mut requested = Map::new();
    requested.insert(
        "cycles_list".to_owned(),
        Value::Array(
            issues
                .iter()
                .map(|issue| Value::String(issue.id.to_string()))
                .collect(),
        ),
    );
    let mut current = Map::new();
    current.insert(
        "updated_cycle_issues".to_owned(),
        Value::Array(moved_activity),
    );
    current.insert(
        "created_cycle_issues".to_owned(),
        Value::String(serialize_cycle_issues(&created).ok_or(Denial::ServerError)?),
    );
    enqueue_task(
        pool,
        ISSUE_ACTIVITY_TASK,
        activity_kwargs(
            "cycle.activity.created",
            user_id,
            None,
            project_id,
            epoch,
            origin,
            &to_spaced_json(&requested).ok_or(Denial::ServerError)?,
            &Value::String(to_spaced_json(&current).ok_or(Denial::ServerError)?),
        ),
    )
    .await;
    Ok(())
}

/// The cycle-set rows inside the transaction: read the live bridges
/// (`-created_at`), insert one bridge per issue with none, then move the
/// bridges pointing elsewhere. Returns the created rows and the moved
/// `(bridge_id, issue_id, old_cycle_id)` triples, in that order.
#[allow(clippy::type_complexity)]
async fn write_cycle_set(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    issues: &[IssueRow],
    target: &Uuid,
) -> Result<(Vec<CreatedBridge>, Vec<(Uuid, Uuid, Uuid)>), Denial> {
    let conn = TxConn::new(txn);
    let rows: Vec<(Uuid, Uuid, Uuid)> = cycle_issues::Entity::find()
        .select_only()
        .columns([
            cycle_issues::Column::Id,
            cycle_issues::Column::IssueId,
            cycle_issues::Column::CycleId,
        ])
        .filter(cycle_issues::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(cycle_issues::Column::DeletedAt.is_null())
        .order_by_desc(cycle_issues::Column::CreatedAt)
        .into_tuple()
        .all(&conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut staying: HashSet<Uuid> = HashSet::new();
    let mut moved: Vec<(Uuid, Uuid, Uuid)> = Vec::new();
    for (id, issue_id, cycle_id) in &rows {
        if cycle_id == target {
            staying.insert(*issue_id);
        } else {
            moved.push((*id, *issue_id, *cycle_id));
        }
    }
    let moved_ids: HashSet<Uuid> = moved.iter().map(|row| row.1).collect();
    let mut created: Vec<CreatedBridge> = Vec::new();
    for issue in issues {
        if staying.contains(&issue.id) || moved_ids.contains(&issue.id) {
            continue;
        }
        // `bulk_create` stamps each object with its own `now()` calls
        // (`auto_now_add` + `auto_now`), so rows carry distinct
        // timestamps — and the serialize below reads them back.
        let created_at = Utc::now();
        let updated_at = Utc::now();
        // `insert` reads the stored row back (`RETURNING`), so the
        // serialize sees the database's microsecond timestamps.
        let row = cycle_issues::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(created_at.into()),
            updated_at: Set(updated_at.into()),
            created_by_id: Set(Some(*user_id)),
            updated_by_id: Set(Some(*user_id)),
            project_id: Set(*project_id),
            workspace_id: Set(*workspace_id),
            cycle_id: Set(*target),
            issue_id: Set(issue.id),
            deleted_at: Set(None),
        }
        .insert(&conn)
        .await
        .map_err(orm_write_denial)?;
        created.push(CreatedBridge {
            id: row.id,
            issue_id: row.issue_id,
            created_at: row.created_at.with_timezone(&Utc),
            updated_at: row.updated_at.with_timezone(&Utc),
            created_by: row.created_by_id,
            updated_by: row.updated_by_id,
            project_id: *project_id,
            workspace_id: *workspace_id,
            cycle_id: *target,
        });
    }
    for (id, _, _) in &moved {
        cycle_issues::Entity::update_many()
            .col_expr(cycle_issues::Column::CycleId, Expr::value(*target))
            .filter(cycle_issues::Column::Id.eq(*id))
            .exec(&conn)
            .await
            .map_err(orm_write_denial)?;
    }
    Ok((created, moved))
}

/// The cycle-clear leg (`cycle_id: null`): soft-delete every live bridge
/// of the selection with the request's shared `now`.
async fn clear_cycle_bridges(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    issues: &[IssueRow],
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    cycle_issues::Entity::update_many()
        .col_expr(
            cycle_issues::Column::DeletedAt,
            Expr::value(DateTimeWithTimeZone::from(now)),
        )
        .filter(cycle_issues::Column::IssueId.is_in(issues.iter().map(|issue| issue.id)))
        .filter(cycle_issues::Column::DeletedAt.is_null())
        .exec(&TxConn::new(txn))
        .await
        .map_err(orm_write_denial)?;
    Ok(())
}

/// A freshly inserted cycle bridge, as the activity serialize needs it.
struct CreatedBridge {
    id: Uuid,
    issue_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    cycle_id: Uuid,
}

fn moved_bridge_json(old_cycle_id: &Uuid, new_cycle_id: &Uuid, issue_id: &Uuid) -> Value {
    let mut map = Map::new();
    map.insert(
        "old_cycle_id".to_owned(),
        Value::String(old_cycle_id.to_string()),
    );
    map.insert(
        "new_cycle_id".to_owned(),
        Value::String(new_cycle_id.to_string()),
    );
    map.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    Value::Object(map)
}

/// `django_serialize("json", created)`: spaced separators, `model`/`pk`/
/// `fields` in model field order, datetimes truncated to millis (bare
/// when the microsecond is zero).
fn serialize_cycle_issues(rows: &[CreatedBridge]) -> Option<String> {
    let mut bridges: Vec<Value> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut fields = Map::new();
        fields.insert(
            "created_at".to_owned(),
            Value::String(render_activity_millis(&row.created_at)),
        );
        fields.insert(
            "updated_at".to_owned(),
            Value::String(render_activity_millis(&row.updated_at)),
        );
        fields.insert(
            "created_by".to_owned(),
            row.created_by
                .map_or(Value::Null, |id| Value::String(id.to_string())),
        );
        fields.insert(
            "updated_by".to_owned(),
            row.updated_by
                .map_or(Value::Null, |id| Value::String(id.to_string())),
        );
        fields.insert("deleted_at".to_owned(), Value::Null);
        fields.insert(
            "project".to_owned(),
            Value::String(row.project_id.to_string()),
        );
        fields.insert(
            "workspace".to_owned(),
            Value::String(row.workspace_id.to_string()),
        );
        fields.insert("issue".to_owned(), Value::String(row.issue_id.to_string()));
        fields.insert("cycle".to_owned(), Value::String(row.cycle_id.to_string()));
        let mut bridge = Map::new();
        bridge.insert(
            "model".to_owned(),
            Value::String("db.cycleissue".to_owned()),
        );
        bridge.insert("pk".to_owned(), Value::String(row.id.to_string()));
        bridge.insert("fields".to_owned(), Value::Object(fields));
        bridges.push(Value::Object(bridge));
    }
    to_spaced_json(&bridges)
}

/// `DjangoJSONEncoder` datetimes: millis truncation, `Z` suffix, no
/// fraction when the microsecond is zero.
fn render_activity_millis(moment: &DateTime<Utc>) -> String {
    let base = moment.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = moment.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}Z")
    } else {
        format!("{base}.{:03}Z", micros / 1000)
    }
}

/// The `applied` payload: validated values in view order, stringified
/// (`str()`, so explicit nulls render `"None"`).
fn applied_json(properties: &BulkProperties, cycle: Option<Uuid>) -> Map<String, Value> {
    let mut map = Map::new();
    if let Some(value) = &properties.state_id {
        map.insert("state_id".to_owned(), opt_uuid_json(value));
    }
    if let Some(priority) = &properties.priority {
        map.insert("priority".to_owned(), Value::String(priority.clone()));
    }
    if let Some(value) = &properties.start_date {
        map.insert("start_date".to_owned(), opt_date_json(value));
    }
    if let Some(value) = &properties.target_date {
        map.insert("target_date".to_owned(), opt_date_json(value));
    }
    if let Some(value) = &properties.estimate_point {
        map.insert("estimate_point".to_owned(), opt_uuid_json(value));
    }
    // Empty arrays are falsy (`properties.get(...)`), so they leave
    // both payloads untouched.
    if let Some(ids) = &properties.label_ids {
        if !ids.is_empty() {
            map.insert("label_ids".to_owned(), uuid_list_json(ids));
        }
    }
    if let Some(ids) = &properties.assignee_ids {
        if !ids.is_empty() {
            map.insert("assignee_ids".to_owned(), uuid_list_json(ids));
        }
    }
    if let Some(ids) = &properties.module_ids {
        if !ids.is_empty() {
            map.insert("module_ids".to_owned(), uuid_list_json(ids));
        }
    }
    if let Some(cycle_id) = cycle {
        map.insert("cycle_id".to_owned(), Value::String(cycle_id.to_string()));
    }
    map
}

/// The per-issue `previous` payload: pre-update values for the requested
/// scalar keys only (`str()`, so stored nulls render `"None"`). Array
/// properties never land here — the view builds `_bulk_previous` before
/// the unions run, so a label-only op carries `{}`.
fn previous_json(properties: &BulkProperties, issue: &IssueRow) -> Map<String, Value> {
    let mut map = Map::new();
    if properties.state_id.is_some() {
        map.insert("state_id".to_owned(), opt_uuid_json(&issue.state_id));
    }
    if properties.priority.is_some() {
        map.insert("priority".to_owned(), Value::String(issue.priority.clone()));
    }
    if properties.start_date.is_some() {
        map.insert("start_date".to_owned(), opt_date_json(&issue.start_date));
    }
    if properties.target_date.is_some() {
        map.insert("target_date".to_owned(), opt_date_json(&issue.target_date));
    }
    if properties.estimate_point.is_some() {
        map.insert(
            "estimate_point".to_owned(),
            opt_uuid_json(&issue.estimate_point_id),
        );
    }
    map
}

fn opt_uuid_json(value: &Option<Uuid>) -> Value {
    value.map_or(Value::String("None".to_owned()), |id| {
        Value::String(id.to_string())
    })
}

fn opt_date_json(value: &Option<NaiveDate>) -> Value {
    value.map_or(Value::String("None".to_owned()), |date| {
        Value::String(date.to_string())
    })
}

fn uuid_list_json(ids: &[Uuid]) -> Value {
    Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect())
}

/// The `issue_activity` kwargs shape all three bulk legs share.
#[allow(clippy::too_many_arguments)]
fn activity_kwargs(
    activity_type: &str,
    actor_id: &Uuid,
    issue_id: Option<&Uuid>,
    project_id: &Uuid,
    epoch: i64,
    origin: &str,
    requested_data: &str,
    current_instance: &Value,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert(
        "issue_id".to_owned(),
        issue_id.map_or(Value::Null, |id| Value::String(id.to_string())),
    );
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("current_instance".to_owned(), current_instance.clone());
    kwargs
}

/// `base_host(request, is_app=True)`: `APP_BASE_URL` when set, else
/// `WEB_URL` — unset everywhere is a 500 (`ImproperlyConfigured`). Empty
/// strings count as unset (Django truthiness).
fn activity_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.web_url.as_deref().filter(|s| !s.is_empty()))
        .map(str::to_owned)
        .ok_or(Denial::ServerError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::body::FilePart;

    const IID: &str = "11111111-1111-1111-1111-111111111111";

    fn issue_row(id: Uuid) -> IssueRow {
        IssueRow {
            id,
            state_id: None,
            priority: "none".to_owned(),
            start_date: None,
            target_date: None,
            estimate_point_id: None,
        }
    }

    /// The bulk selection keeps the old statement's tenant join, scope
    /// and model `-created_at` order.
    #[test]
    fn bulk_issue_statement_shape() {
        use sea_orm::QueryTrait;
        let issue = Uuid::from_u128(1);
        let sql = bulk_issues_query("acme", Uuid::from_u128(2), &[issue])
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(sql.starts_with(
            r#"SELECT "issues"."id", "issues"."state_id", "issues"."priority", "issues"."start_date", "issues"."target_date", "issues"."estimate_point_id" FROM "issues" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id""#
        ), "{sql}");
        assert!(
            sql.contains(&format!(r#""issues"."id" IN ('{issue}')"#)),
            "{sql}"
        );
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#));
        assert!(
            sql.ends_with(r#"ORDER BY "issues"."created_at" DESC"#),
            "{sql}"
        );
    }

    /// Union pairs: issues outer, ids inner, live pairs and repeats skipped.
    #[test]
    fn union_pairs_skip_live_and_repeated_pairs() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let (x, y) = (Uuid::from_u128(10), Uuid::from_u128(11));
        let pairs = union_pairs(vec![(a, x)], &[issue_row(a), issue_row(b)], &[x, y, y]);
        assert_eq!(pairs, vec![(a, y), (b, x), (b, y)]);
    }

    #[test]
    fn missing_ids_sort_as_strings() {
        let (a, b, c) = (Uuid::from_u128(3), Uuid::from_u128(1), Uuid::from_u128(2));
        assert!(missing_ids_error(&[a, b], &[a, b], "Labels").is_ok());
        match missing_ids_error(&[a, b, c], &[c], "Labels") {
            Err(Denial::BadError(message)) => {
                assert_eq!(message, format!("Labels do not exist: {b}, {a}"))
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The bulk reads and writes against constraint-free temp tables on a
    /// one-connection pool.
    #[tokio::test]
    async fn live_bulk_rows() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "workspaces",
            "projects",
            "project_members",
            "issues",
            "issue_subscribers",
            "states",
            "labels",
            "users",
            "modules",
            "estimate_points",
            "cycles",
            "issue_labels",
            "issue_assignees",
            "module_issues",
            "cycle_issues",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        sqlx::query("ALTER TABLE cycle_issues ADD PRIMARY KEY (id)")
            .execute(&pool)
            .await
            .expect("pk");
        let id = Uuid::new_v4;
        let (ws, project, gone_project, actor, outsider) = (id(), id(), id(), id(), id());
        let (old, new, deleted) = (id(), id(), id());
        let (state, label, module, point, open_cycle, done_cycle, other_cycle) =
            (id(), id(), id(), id(), id(), id(), id());
        let old_bridge = id();
        let issue = |iid: Uuid, age: i32, gone: bool| {
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, priority, start_date, created_at, \
                 deleted_at) VALUES ('{iid}', '{ws}', '{project}', 'high', '2026-01-02', \
                 now() - interval '{age} min', {})",
                if gone { "now()" } else { "NULL" }
            )
        };
        let bridge = |table: &str, column: &str, issue: Uuid, other: Uuid| {
            format!(
                "INSERT INTO {table} (id, created_at, updated_at, issue_id, {column}, project_id, \
                 workspace_id, deleted_at) VALUES ('{}', now(), now(), '{issue}', '{other}', \
                 '{project}', '{ws}', NULL)",
                id()
            )
        };
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, workspace_id, deleted_at) VALUES \
                 ('{project}', '{ws}', NULL), ('{gone_project}', '{ws}', now())"
            ),
            format!(
                "INSERT INTO project_members (id, member_id, project_id, workspace_id, role, \
                 is_active, deleted_at) VALUES ('{}', '{actor}', '{project}', '{ws}', 5, true, NULL)",
                id()
            ),
            issue(old, 2, false),
            issue(new, 1, false),
            issue(deleted, 0, true),
            // A soft-deleted state still resolves (`all_state_objects`).
            format!("INSERT INTO states (id, deleted_at) VALUES ('{state}', now())"),
            format!("INSERT INTO labels (id, deleted_at) VALUES ('{label}', NULL)"),
            format!("INSERT INTO users (id) VALUES ('{actor}')"),
            format!("INSERT INTO modules (id, deleted_at) VALUES ('{module}', NULL)"),
            format!("INSERT INTO estimate_points (id, deleted_at) VALUES ('{point}', NULL)"),
            format!(
                "INSERT INTO cycles (id, workspace_id, project_id, end_date, deleted_at) VALUES \
                 ('{open_cycle}', '{ws}', '{project}', now() + interval '1 day', NULL), \
                 ('{done_cycle}', '{ws}', '{project}', now() - interval '1 day', NULL), \
                 ('{other_cycle}', '{ws}', '{project}', NULL, NULL)"
            ),
            bridge("issue_labels", "label_id", old, label),
            bridge("issue_assignees", "assignee_id", old, actor),
            bridge("module_issues", "module_id", new, module),
            format!(
                "INSERT INTO cycle_issues (id, created_at, updated_at, issue_id, cycle_id, \
                 project_id, workspace_id, deleted_at) VALUES ('{old_bridge}', now(), now(), \
                 '{old}', '{other_cycle}', '{project}', '{ws}', NULL)"
            ),
            format!(
                "INSERT INTO issue_subscribers (id, created_at, updated_at, issue_id, \
                 subscriber_id, project_id, workspace_id, deleted_at) VALUES ('{}', now(), now(), \
                 '{old}', '{actor}', '{project}', '{ws}', NULL)",
                id()
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }

        // Gates and the tenant reads.
        assert!(lite_allowed(&pool, "acme", &project, &actor)
            .await
            .expect("lite"));
        assert!(!lite_allowed(&pool, "acme", &project, &outsider)
            .await
            .expect("lite"));
        assert!(!lite_allowed(&pool, "other", &project, &actor)
            .await
            .expect("lite"));
        let scope = fetch_project(&pool, "acme", &project)
            .await
            .expect("project")
            .expect("live");
        assert_eq!(scope.workspace_id, ws);
        assert!(fetch_project(&pool, "acme", &gone_project)
            .await
            .expect("project")
            .is_none());
        assert!(fetch_project(&pool, "other", &project)
            .await
            .expect("project")
            .is_none());
        let issues = fetch_bulk_issues(&pool, "acme", &project, &[old, new, deleted])
            .await
            .expect("issues");
        assert_eq!(
            issues.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![new, old]
        );
        assert_eq!(issues[0].priority, "high");
        assert_eq!(issues[0].start_date, NaiveDate::from_ymd_opt(2026, 1, 2));
        assert!(fetch_bulk_issues(&pool, "other", &project, &[old])
            .await
            .expect("issues")
            .is_empty());

        // Whole-batch reference checks.
        let ok = BulkProperties {
            state_id: Some(Some(state)),
            label_ids: Some(vec![label]),
            assignee_ids: Some(vec![actor]),
            module_ids: Some(vec![module]),
            estimate_point: Some(Some(point)),
            cycle_id: Some(Some(open_cycle)),
            ..Default::default()
        };
        let batch = check_batch(&pool, "acme", &project, &ok, &issues)
            .await
            .expect("valid");
        assert_eq!(batch.cycle.map(|cycle| cycle.id), Some(open_cycle));
        let bad = |properties: BulkProperties| {
            let pool = pool.clone();
            let issues = &issues;
            async move {
                match check_batch(&pool, "acme", &project, &properties, issues).await {
                    Err(Denial::BadError(message)) => message,
                    other => panic!("unexpected {:?}", other.map(|_| ())),
                }
            }
        };
        let ghost = Uuid::new_v4();
        assert_eq!(
            bad(BulkProperties {
                state_id: Some(Some(ghost)),
                ..Default::default()
            })
            .await,
            format!("State {ghost} does not exist")
        );
        assert_eq!(
            bad(BulkProperties {
                label_ids: Some(vec![label, ghost]),
                ..Default::default()
            })
            .await,
            format!("Labels do not exist: {ghost}")
        );
        assert_eq!(
            bad(BulkProperties {
                assignee_ids: Some(vec![ghost]),
                ..Default::default()
            })
            .await,
            format!("Users do not exist: {ghost}")
        );
        assert_eq!(
            bad(BulkProperties {
                module_ids: Some(vec![ghost]),
                ..Default::default()
            })
            .await,
            format!("Modules do not exist: {ghost}")
        );
        assert_eq!(
            bad(BulkProperties {
                estimate_point: Some(Some(ghost)),
                ..Default::default()
            })
            .await,
            format!("Estimate point {ghost} does not exist")
        );
        assert_eq!(
            bad(BulkProperties {
                cycle_id: Some(Some(ghost)),
                ..Default::default()
            })
            .await,
            format!("Cycle {ghost} does not exist in this project")
        );
        assert_eq!(
            bad(BulkProperties {
                cycle_id: Some(Some(done_cycle)),
                ..Default::default()
            })
            .await,
            COMPLETED_CYCLE_MESSAGE
        );
        assert_eq!(
            bad(BulkProperties {
                target_date: Some(NaiveDate::from_ymd_opt(2026, 1, 1)),
                ..Default::default()
            })
            .await,
            DATE_ORDER_MESSAGE
        );

        // The transactional writes.
        let now = Utc::now();
        let mut txn = pool.begin().await.expect("begin");
        union_labels(&mut txn, &project, &ws, &actor, now, &issues, &[label])
            .await
            .expect("labels");
        union_assignees(&mut txn, &project, &ws, &actor, now, &issues, &[actor])
            .await
            .expect("assignees");
        let pairs = union_module_bridges(&mut txn, &project, &ws, &actor, now, &issues, &[module])
            .await
            .expect("modules");
        assert_eq!(pairs, vec![(old, module)]);
        let (created, moved) =
            write_cycle_set(&mut txn, &project, &ws, &actor, &issues, &open_cycle)
                .await
                .expect("cycle set");
        assert_eq!(moved, vec![(old_bridge, old, other_cycle)]);
        assert_eq!(created.len(), 1);
        assert_eq!(
            (
                created[0].issue_id,
                created[0].created_by,
                created[0].cycle_id
            ),
            (new, Some(actor), open_cycle)
        );
        // `RETURNING` hands back the stored (microsecond) timestamps.
        assert_eq!(created[0].created_at.timestamp_subsec_nanos() % 1000, 0);
        txn.commit().await.expect("commit");
        for (table, column, other) in [
            ("issue_labels", "label_id", label),
            ("issue_assignees", "assignee_id", actor),
            ("module_issues", "module_id", module),
            ("cycle_issues", "cycle_id", open_cycle),
        ] {
            let (count,): (i64,) = sqlx::query_as(&format!(
                "SELECT COUNT(*) FROM {table} WHERE {column} = $1 AND deleted_at IS NULL \
                 AND issue_id = ANY($2) AND project_id = $3 AND workspace_id = $4"
            ))
            .bind(other)
            .bind([old, new].as_slice())
            .bind(project)
            .bind(ws)
            .fetch_one(&pool)
            .await
            .expect("count");
            assert_eq!(count, 2, "{table}");
        }
        let mut txn = pool.begin().await.expect("begin");
        clear_cycle_bridges(&mut txn, &issues, now)
            .await
            .expect("clear");
        txn.commit().await.expect("commit");
        let (live,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM cycle_issues WHERE deleted_at IS NULL")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(live, 0);

        // Subscribe: idempotent over the live subscription.
        let base = BulkBase {
            pool: pool.clone(),
            slug: "acme".to_owned(),
            project_id: project,
            user_id: actor,
        };
        assert_eq!(
            subscribe_issues(&base, &scope, &issues)
                .await
                .expect("subscribe"),
            vec![new]
        );
        assert!(subscribe_issues(&base, &scope, &issues)
            .await
            .expect("again")
            .is_empty());

        // A unique violation is the invalid-payload 400.
        let mut txn = pool.begin().await.expect("begin");
        let duplicate = cycle_issues::ActiveModel {
            id: Set(old_bridge),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            created_by_id: Set(None),
            updated_by_id: Set(None),
            cycle_id: Set(open_cycle),
            issue_id: Set(old),
            project_id: Set(project),
            workspace_id: Set(ws),
            deleted_at: Set(None),
        }
        .insert(&TxConn::new(&mut txn))
        .await
        .expect_err("duplicate pk");
        assert!(matches!(
            orm_write_denial(duplicate),
            Denial::BadError(message) if message == PAYLOAD_INVALID_MESSAGE
        ));
    }

    fn dval_from_json(text: &str) -> DVal {
        let parsed = parse_request_bytes_spans(text.as_bytes(), &[]).expect("valid json");
        dval_from_jval(&parsed)
    }

    fn operation_body(text: &str) -> Result<(Vec<Uuid>, BulkProperties), String> {
        validate_operation_body(&dval_from_json(text), false)
    }

    fn operation_err(text: &str) -> String {
        operation_body(text).expect_err("must be a 400").to_string()
    }

    #[test]
    fn operation_validation_matrix_matches_drf_bytes() {
        let iid = IID;
        let cases: Vec<(String, String)> = vec![
            (
                r#"{}"#.to_owned(),
                r#"{"issue_ids":["This field is required."],"properties":["This field is required."]}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"]}}"#),
                r#"{"properties":["This field is required."]}"#.to_owned(),
            ),
            (
                r#"{"properties":{"priority":"high"}}"#.to_owned(),
                r#"{"issue_ids":["This field is required."]}"#.to_owned(),
            ),
            (
                r#"{"issue_ids":[],"properties":{"priority":"high"}}"#.to_owned(),
                r#"{"issue_ids":["Ensure this field has at least 1 elements."]}"#.to_owned(),
            ),
            (
                r#"{"issue_ids":null,"properties":{"priority":"high"}}"#.to_owned(),
                r#"{"issue_ids":["This field may not be null."]}"#.to_owned(),
            ),
            (
                r#"{"issue_ids":"nope","properties":{"priority":"high"}}"#.to_owned(),
                r#"{"issue_ids":["Expected a list of items but got type \"str\"."]}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}","nope"],"properties":{{"priority":"high"}}}}"#),
                r#"{"issue_ids":{"1":["Must be a valid UUID."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}",null],"properties":{{"priority":"high"}}}}"#),
                r#"{"issue_ids":{"1":["This field may not be null."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{}}}}"#),
                r#"{"properties":{"non_field_errors":["Properties must name at least one bulk-editable field."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"name":"x"}}}}"#),
                r#"{"properties":{"name":"This property cannot be bulk-edited."}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"zzz":1,"name":"x"}}}}"#),
                r#"{"properties":{"name":"This property cannot be bulk-edited.","zzz":"This property cannot be bulk-edited."}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":"str"}}"#),
                r#"{"properties":{"non_field_errors":["Invalid data. Expected a dictionary, but got str."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":["l"]}}"#),
                r#"{"properties":{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":null}}"#),
                r#"{"properties":["This field may not be null."]}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"priority":"bogus"}}}}"#),
                r#"{"properties":{"priority":["\"bogus\" is not a valid choice."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"priority":5}}}}"#),
                r#"{"properties":{"priority":["\"5\" is not a valid choice."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"priority":true}}}}"#),
                r#"{"properties":{"priority":["\"True\" is not a valid choice."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"priority":null}}}}"#),
                r#"{"properties":{"priority":["This field may not be null."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"priority":["high"]}}}}"#),
                r#"{"properties":{"priority":["\"['high']\" is not a valid choice."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"state_id":"nope"}}}}"#),
                r#"{"properties":{"state_id":["Must be a valid UUID."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"start_date":"11/10/2026"}}}}"#),
                r#"{"properties":{"start_date":["Date has wrong format. Use one of these formats instead: YYYY-MM-DD."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"label_ids":"nope"}}}}"#),
                r#"{"properties":{"label_ids":["Expected a list of items but got type \"str\"."]}}"#.to_owned(),
            ),
            (
                format!(r#"{{"issue_ids":["{iid}"],"properties":{{"label_ids":["nope"]}}}}"#),
                r#"{"properties":{"label_ids":{"0":["Must be a valid UUID."]}}}"#.to_owned(),
            ),
            (
                format!(
                    r#"{{"issue_ids":["{iid}"],"properties":{{"start_date":"2026-11-10","target_date":"2026-11-01"}}}}"#
                ),
                r#"{"properties":{"non_field_errors":["Start date cannot exceed target date"]}}"#.to_owned(),
            ),
            (
                r#"[1]"#.to_owned(),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#.to_owned(),
            ),
            (
                r#""str""#.to_owned(),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got str."]}"#.to_owned(),
            ),
            (
                r#"null"#.to_owned(),
                r#"{"non_field_errors":["No data provided"]}"#.to_owned(),
            ),
            // Field errors render in declaration order, not input order.
            (
                format!(
                    r#"{{"issue_ids":["{iid}"],"properties":{{"priority":"x","state_id":"y","label_ids":"z"}}}}"#
                ),
                concat!(
                    r#"{"properties":{"state_id":["Must be a valid UUID."],"#,
                    r#""priority":["\"x\" is not a valid choice."],"#,
                    r#""label_ids":["Expected a list of items but got type \"str\"."]}}"#,
                )
                .to_owned(),
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(operation_err(&input), expected, "{input}");
        }
    }

    #[test]
    fn operation_uuid_spellings_and_coercions_validate() {
        let iid = IID;
        let nodash = iid.replace('-', "");
        let upper = iid.to_uppercase();
        // All four `uuid.UUID` spellings validate to the same id.
        for spelling in [
            iid.to_owned(),
            nodash,
            upper,
            format!("{{{iid}}}"),
            format!("urn:uuid:{iid}"),
        ] {
            let (ids, properties) = operation_body(&format!(
                r#"{{"issue_ids":["{spelling}"],"properties":{{"priority":"low"}}}}"#
            ))
            .expect("valid");
            assert_eq!(ids, vec![Uuid::parse_str(iid).unwrap()], "{spelling}");
            assert_eq!(properties.priority.as_deref(), Some("low"));
        }
        // Ints and bools fold through `UUID(int=...)`.
        let (ids, _) =
            operation_body(r#"{"issue_ids":[123,true],"properties":{"priority":"low"}}"#)
                .expect("valid");
        assert_eq!(ids, vec![Uuid::from_u128(123), Uuid::from_u128(1)], "{iid}");
        // `-0` folds to zero; negatives, floats and huge ints 400.
        let (ids, _) =
            operation_body(r#"{"issue_ids":[-0],"properties":{"priority":"low"}}"#).expect("valid");
        assert_eq!(ids, vec![Uuid::from_u128(0)]);
        for bad in ["-5", "1.5", "340282366920938463463374607431768211456"] {
            let body = operation_err(&format!(
                r#"{{"issue_ids":[{bad}],"properties":{{"priority":"low"}}}}"#
            ));
            assert_eq!(
                body, r#"{"issue_ids":{"0":["Must be a valid UUID."]}}"#,
                "{bad}"
            );
        }
        // Explicit nulls validate as clears.
        let (_, properties) = operation_body(&format!(
            r#"{{"issue_ids":["{iid}"],"properties":{{"state_id":null,"cycle_id":null}}}}"#
        ))
        .expect("valid");
        assert_eq!(properties.state_id, Some(None));
        assert_eq!(properties.cycle_id, Some(None));
        // Extra top-level keys are ignored.
        let (ids, _) = operation_body(&format!(
            r#"{{"issue_ids":["{iid}"],"properties":{{"priority":"low"}},"foo":1}}"#
        ))
        .expect("valid");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn subscribe_validation_matches_bulk_ids() {
        let iid = IID;
        let valid = validate_subscribe_body(&dval_from_json(&format!(
            r#"{{"issue_ids":["{iid}",123]}}"#
        )))
        .expect("valid");
        assert_eq!(
            valid,
            vec![Uuid::parse_str(iid).unwrap(), Uuid::from_u128(123)]
        );
        for (input, expected) in [
            (r#"null"#, r#"{"non_field_errors":["No data provided"]}"#),
            (
                r#"[1]"#,
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#,
            ),
            (r#"{}"#, r#"{"issue_ids":["This field is required."]}"#),
            (
                r#"{"issue_ids":[]}"#,
                r#"{"issue_ids":["Ensure this field has at least 1 elements."]}"#,
            ),
            (
                r#"{"issue_ids":["nope"]}"#,
                r#"{"issue_ids":{"0":["Must be a valid UUID."]}}"#,
            ),
        ] {
            let body = validate_subscribe_body(&dval_from_json(input)).expect_err("400");
            assert_eq!(body, expected, "{input}");
        }
    }

    fn file_part(filename: &str) -> FilePart {
        FilePart {
            filename: filename.to_owned(),
            content_type: String::new(),
            bytes: Vec::new(),
            in_memory: true,
        }
    }

    fn form_data(texts: Vec<(&str, Value)>, files: Vec<(&str, Vec<FilePart>)>) -> DVal {
        let mut map = Map::new();
        for (key, value) in texts {
            map.insert(key.to_owned(), value);
        }
        let mut file_map: shared_body::FilesMap = BTreeMap::new();
        for (key, parts) in files {
            file_map.insert(key.to_owned(), parts);
        }
        dval_from_form(&map, &file_map)
    }

    fn form_operation_err(data: &DVal) -> String {
        validate_operation_body(data, true).expect_err("must be a 400")
    }

    #[test]
    fn form_shapes_follow_html_input_rules() {
        let iid = IID;
        // Present texts read `getlist`; `properties` stays required.
        let data = form_data(
            vec![(
                "issue_ids",
                Value::Array(vec![Value::String(iid.to_owned())]),
            )],
            vec![],
        );
        assert_eq!(
            form_operation_err(&data),
            r#"{"properties":["This field is required."]}"#
        );
        // A blank text item fails the child parser (not the required arm).
        let data = form_data(
            vec![(
                "issue_ids",
                Value::Array(vec![Value::String(String::new())]),
            )],
            vec![],
        );
        assert_eq!(
            form_operation_err(&data),
            concat!(
                r#"{"issue_ids":{"0":["Must be a valid UUID."]},"#,
                r#""properties":["This field is required."]}"#,
            )
        );
        // Nested scalars validate; flat `properties` keys are ignored.
        let data = form_data(
            vec![
                (
                    "issue_ids",
                    Value::Array(vec![Value::String(iid.to_owned())]),
                ),
                ("properties.priority", Value::String("high".to_owned())),
                ("properties", Value::String("x".to_owned())),
            ],
            vec![],
        );
        let (ids, properties) = validate_operation_body(&data, true).expect("valid");
        assert_eq!(ids.len(), 1);
        assert_eq!(properties.priority.as_deref(), Some("high"));
        // Unknown nested keys report through the custom shape.
        let data = form_data(
            vec![
                (
                    "issue_ids",
                    Value::Array(vec![Value::String(iid.to_owned())]),
                ),
                ("properties.name", Value::String("x".to_owned())),
            ],
            vec![],
        );
        assert_eq!(
            form_operation_err(&data),
            r#"{"properties":{"name":"This property cannot be bulk-edited."}}"#
        );
        // Blank allow-null scalars clear; blank priority skips (leaving
        // the properties dict present-but-empty).
        let data = form_data(
            vec![
                (
                    "issue_ids",
                    Value::Array(vec![Value::String(iid.to_owned())]),
                ),
                ("properties.state_id", Value::String(String::new())),
            ],
            vec![],
        );
        let (_, properties) = validate_operation_body(&data, true).expect("valid");
        assert_eq!(properties.state_id, Some(None));
        let data = form_data(
            vec![
                (
                    "issue_ids",
                    Value::Array(vec![Value::String(iid.to_owned())]),
                ),
                ("properties.priority", Value::String(String::new())),
            ],
            vec![],
        );
        assert_eq!(
            form_operation_err(&data),
            r#"{"properties":{"non_field_errors":["Properties must name at least one bulk-editable field."]}}"#
        );
        // Files join the value lists after texts; uploads fail UUID
        // fields and echo their name in choice fields.
        let data = form_data(
            vec![(
                "issue_ids",
                Value::Array(vec![Value::String(iid.to_owned())]),
            )],
            vec![("issue_ids", vec![file_part("f.txt")])],
        );
        assert_eq!(
            form_operation_err(&data),
            concat!(
                r#"{"issue_ids":{"1":["Must be a valid UUID."]},"#,
                r#""properties":["This field is required."]}"#,
            )
        );
        let data = form_data(
            vec![(
                "issue_ids",
                Value::Array(vec![Value::String(iid.to_owned())]),
            )],
            vec![("properties.priority", vec![file_part("p.txt")])],
        );
        assert_eq!(
            form_operation_err(&data),
            r#"{"properties":{"priority":["\"p.txt\" is not a valid choice."]}}"#
        );
    }

    #[test]
    fn millis_rendering_truncates_like_django() {
        use chrono::TimeZone;
        let moment = Utc.with_ymd_and_hms(2026, 10, 31, 20, 6, 50).unwrap()
            + chrono::Duration::microseconds(417_123);
        assert_eq!(render_activity_millis(&moment), "2026-10-31T20:06:50.417Z");
        let bare = Utc.with_ymd_and_hms(2026, 10, 31, 20, 6, 50).unwrap();
        assert_eq!(render_activity_millis(&bare), "2026-10-31T20:06:50Z");
    }

    #[test]
    fn cycle_serialize_matches_django_separators_and_order() {
        use chrono::TimeZone;
        let created_at = Utc.with_ymd_and_hms(2026, 10, 31, 20, 6, 50).unwrap()
            + chrono::Duration::microseconds(417_123);
        let updated_at = Utc.with_ymd_and_hms(2026, 10, 31, 20, 6, 50).unwrap()
            + chrono::Duration::microseconds(417_999);
        let bridge = CreatedBridge {
            id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000001").unwrap(),
            issue_id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000002").unwrap(),
            created_at,
            updated_at,
            created_by: Some(Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000003").unwrap()),
            updated_by: Some(Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000003").unwrap()),
            project_id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000004").unwrap(),
            workspace_id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000005").unwrap(),
            cycle_id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000006").unwrap(),
        };
        assert_eq!(
            serialize_cycle_issues(std::slice::from_ref(&bridge)).unwrap(),
            concat!(
                r#"[{"model": "db.cycleissue", "#,
                r#""pk": "aaaaaaaa-0000-4000-8000-000000000001", "#,
                r#""fields": {"created_at": "2026-10-31T20:06:50.417Z", "#,
                r#""updated_at": "2026-10-31T20:06:50.417Z", "#,
                r#""created_by": "aaaaaaaa-0000-4000-8000-000000000003", "#,
                r#""updated_by": "aaaaaaaa-0000-4000-8000-000000000003", "#,
                r#""deleted_at": null, "#,
                r#""project": "aaaaaaaa-0000-4000-8000-000000000004", "#,
                r#""workspace": "aaaaaaaa-0000-4000-8000-000000000005", "#,
                r#""issue": "aaaaaaaa-0000-4000-8000-000000000002", "#,
                r#""cycle": "aaaaaaaa-0000-4000-8000-000000000006"}}]"#,
            )
        );
        assert_eq!(serialize_cycle_issues(&[]).unwrap(), "[]");
    }

    #[test]
    fn applied_and_previous_keep_view_order_and_none_strings() {
        let state = Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000010").unwrap();
        let other = Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000011").unwrap();
        let properties = BulkProperties {
            state_id: Some(None),
            priority: Some("high".to_owned()),
            label_ids: Some(vec![]),
            assignee_ids: Some(vec![other]),
            module_ids: Some(vec![other]),
            ..BulkProperties::default()
        };
        let applied = applied_json(&properties, None);
        assert_eq!(
            to_spaced_json(&applied).unwrap(),
            format!(
                "{{\"state_id\": \"None\", \"priority\": \"high\", \
                 \"assignee_ids\": [\"{other}\"], \"module_ids\": [\"{other}\"]}}"
            )
        );
        let issue = IssueRow {
            id: Uuid::parse_str(IID).unwrap(),
            state_id: Some(state),
            priority: "none".to_owned(),
            start_date: None,
            target_date: None,
            estimate_point_id: None,
        };
        let previous = previous_json(&properties, &issue);
        // The empty input array is falsy (neither payload carries it) and
        // non-empty arrays land in `applied` only, never `previous`.
        assert_eq!(
            to_spaced_json(&previous).unwrap(),
            format!("{{\"state_id\": \"{state}\", \"priority\": \"none\"}}")
        );
    }

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    #[test]
    fn activity_origin_falls_through_empty_strings() {
        let app = "https://app.example.invalid";
        let web = "https://web.example.invalid";
        assert_eq!(
            activity_origin(&origin_state(Some(app), Some(web))).unwrap(),
            app
        );
        assert_eq!(
            activity_origin(&origin_state(None, Some(web))).unwrap(),
            web
        );
        assert_eq!(
            activity_origin(&origin_state(Some(""), Some(web))).unwrap(),
            web
        );
        assert!(activity_origin(&origin_state(Some(""), Some(""))).is_err());
        assert!(activity_origin(&origin_state(None, None)).is_err());
    }
}
