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

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, NaiveDate, Utc};
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
use pidash_services::domain::issues::to_spaced_json;

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
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT issue_id FROM issue_subscribers
           WHERE issue_id = ANY($1) AND subscriber_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(ids.as_slice())
    .bind(base.user_id)
    .fetch_all(&base.pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let subscribed: HashSet<Uuid> = rows.into_iter().map(|row| row.0).collect();
    let fresh: Vec<Uuid> = issues
        .iter()
        .map(|issue| issue.id)
        .filter(|id| !subscribed.contains(id))
        .collect();
    if !fresh.is_empty() {
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO issue_subscribers
               (id, created_at, updated_at, created_by_id, updated_by_id,
                issue_id, project_id, subscriber_id, workspace_id)
               SELECT gen_random_uuid(), $1, $1, $2, $2, issue_id, $3, $2, $4
               FROM unnest($5::uuid[]) AS t(issue_id)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(base.user_id)
        .bind(base.project_id)
        .bind(project.workspace_id)
        .bind(fresh.as_slice())
        .execute(&base.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    Ok(json_response(format!(
        "{{\"message\":\"Subscribed to {} issues\",\"subscribed\":{}}}",
        fresh.len(),
        fresh.len()
    )))
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
    let allowed: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(allowed.is_some())
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
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT p.id, p.workspace_id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(_, workspace_id)| ProjectScope { workspace_id }))
}

/// The bulk selection: live issues in the tenant (`Issue.objects` —
/// drafts, archived and triage rows included), model `-created_at` order.
struct IssueRow {
    id: Uuid,
    state_id: Option<Uuid>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    estimate_point_id: Option<Uuid>,
}

/// The bulk-selection row shape (one tuple per issue).
type BulkIssueTuple = (
    Uuid,
    Option<Uuid>,
    String,
    Option<NaiveDate>,
    Option<NaiveDate>,
    Option<Uuid>,
);

async fn fetch_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<IssueRow>, Denial> {
    let rows: Vec<BulkIssueTuple> = sqlx::query_as(
        r#"SELECT i.id, i.state_id, i.priority, i.start_date, i.target_date,
                  i.estimate_point_id
           FROM issues i JOIN workspaces w ON w.id = i.workspace_id
           WHERE w.slug = $1 AND i.project_id = $2 AND i.id = ANY($3)
           AND i.deleted_at IS NULL
           ORDER BY i.created_at DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(id, state_id, priority, start_date, target_date, estimate_point_id)| IssueRow {
                id,
                state_id,
                priority,
                start_date,
                target_date,
                estimate_point_id,
            },
        )
        .collect())
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
    if let Some(Some(state_id)) = properties.state_id {
        // `all_state_objects`: soft-deleted states count (no filter).
        let found: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM states WHERE id = $1")
            .bind(state_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if found.is_none() {
            return Err(Denial::BadError(format!("State {state_id} does not exist")));
        }
    }
    if let Some(label_ids) = &properties.label_ids {
        missing_ids_error(
            pool,
            "SELECT id FROM labels WHERE id = ANY($1) AND deleted_at IS NULL",
            label_ids,
            "Labels",
        )
        .await?;
    }
    if let Some(assignee_ids) = &properties.assignee_ids {
        // `User.objects` is the stock manager: no soft-delete filter.
        missing_ids_error(
            pool,
            "SELECT id FROM users WHERE id = ANY($1)",
            assignee_ids,
            "Users",
        )
        .await?;
    }
    if let Some(module_ids) = &properties.module_ids {
        missing_ids_error(
            pool,
            "SELECT id FROM modules WHERE id = ANY($1) AND deleted_at IS NULL",
            module_ids,
            "Modules",
        )
        .await?;
    }
    if let Some(Some(point_id)) = properties.estimate_point {
        let found: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM estimate_points WHERE id = $1 AND deleted_at IS NULL")
                .bind(point_id)
                .fetch_optional(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        if found.is_none() {
            return Err(Denial::BadError(format!(
                "Estimate point {point_id} does not exist"
            )));
        }
    }
    let mut batch = BatchCheck { cycle: None };
    if let Some(Some(cycle_id)) = properties.cycle_id {
        let row: Option<(Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(
            r#"SELECT c.id, c.end_date FROM cycles c
               JOIN workspaces w ON w.id = c.workspace_id
               WHERE w.slug = $1 AND c.project_id = $2 AND c.id = $3
               AND c.deleted_at IS NULL"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let Some((id, end_date)) = row else {
            return Err(Denial::BadError(format!(
                "Cycle {cycle_id} does not exist in this project"
            )));
        };
        if let Some(end) = end_date {
            if end < Utc::now() {
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

/// The `_missing` helper: referenced rows that do not resolve, sorted as
/// strings in the error.
async fn missing_ids_error(
    pool: &sqlx::PgPool,
    sql: &str,
    ids: &[Uuid],
    label: &str,
) -> Result<(), Denial> {
    if ids.is_empty() {
        return Ok(());
    }
    let rows: Vec<(Uuid,)> = sqlx::query_as(sql)
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let found: HashSet<Uuid> = rows.into_iter().map(|row| row.0).collect();
    let mut missing: Vec<String> = ids
        .iter()
        .filter(|id| !found.contains(id))
        .map(ToString::to_string)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort();
    missing.dedup();
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
            let state_id = properties.state_id.unwrap_or(issue.state_id);
            let priority = properties.priority.as_deref().unwrap_or(&issue.priority);
            let start_date = properties.start_date.unwrap_or(issue.start_date);
            let target_date = properties.target_date.unwrap_or(issue.target_date);
            let estimate_point_id = properties.estimate_point.unwrap_or(issue.estimate_point_id);
            sqlx::query(
                r#"UPDATE issues SET state_id = $1, priority = $2, start_date = $3,
                          target_date = $4, estimate_point_id = $5, updated_at = $6
                   WHERE id = $7"#,
            )
            .bind(state_id)
            .bind(priority)
            .bind(start_date)
            .bind(target_date)
            .bind(estimate_point_id)
            .bind(now)
            .bind(issue.id)
            .execute(&mut *txn)
            .await
            .map_err(write_denial)?;
        }
    }
    let mut live_labels: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    if let Some(label_ids) = &properties.label_ids {
        if !label_ids.is_empty() {
            live_labels = union_bridges(
                &mut txn,
                "issue_labels",
                "label_id",
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
    let mut live_assignees: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    if let Some(assignee_ids) = &properties.assignee_ids {
        if !assignee_ids.is_empty() {
            live_assignees = union_bridges(
                &mut txn,
                "issue_assignees",
                "assignee_id",
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
    let mut live_modules: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    if let Some(module_ids) = &properties.module_ids {
        if !module_ids.is_empty() {
            let added = union_module_bridges(
                &mut txn,
                &base.project_id,
                &project.workspace_id,
                &base.user_id,
                now,
                issues,
                module_ids,
            )
            .await?;
            live_modules = added.live;
            if let Some(origin) = &origin {
                for (issue_id, module_id) in &added.pairs {
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
                let ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
                sqlx::query(
                    r#"UPDATE cycle_issues SET deleted_at = $1
                       WHERE issue_id = ANY($2) AND deleted_at IS NULL"#,
                )
                .bind(now)
                .bind(ids.as_slice())
                .execute(&mut *txn)
                .await
                .map_err(write_denial)?;
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
            let previous = previous_json(
                properties,
                issue,
                &live_labels,
                &live_assignees,
                &live_modules,
            );
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

/// Union `label`/`assignee` bridge rows onto the selection, returning the
/// pre-write live rows per issue (model `-created_at` order, for the
/// `previous` payloads).
#[allow(clippy::too_many_arguments)]
async fn union_bridges(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    column: &str,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    now: DateTime<Utc>,
    issues: &[IssueRow],
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, Denial> {
    let issue_ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(&format!(
        "SELECT issue_id, {column} FROM {table} \
         WHERE issue_id = ANY($1) AND deleted_at IS NULL ORDER BY created_at DESC"
    ))
    .bind(issue_ids.as_slice())
    .fetch_all(&mut **txn)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut live: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut seen: HashSet<(Uuid, Uuid)> = HashSet::new();
    for (issue_id, other_id) in rows {
        live.entry(issue_id).or_default().push(other_id);
        seen.insert((issue_id, other_id));
    }
    let mut added_issues: Vec<Uuid> = Vec::new();
    let mut added_ids: Vec<Uuid> = Vec::new();
    for issue in issues {
        for id in ids {
            if !seen.contains(&(issue.id, *id)) {
                seen.insert((issue.id, *id));
                added_issues.push(issue.id);
                added_ids.push(*id);
            }
        }
    }
    if !added_issues.is_empty() {
        sqlx::query(&format!(
            "INSERT INTO {table} \
             (id, created_at, updated_at, created_by_id, updated_by_id, \
              issue_id, {column}, project_id, workspace_id) \
             SELECT gen_random_uuid(), $1, $1, $2, $2, issue_id, other_id, $3, $4 \
             FROM unnest($5::uuid[], $6::uuid[]) AS t(issue_id, other_id) \
             ON CONFLICT DO NOTHING"
        ))
        .bind(now)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(added_issues.as_slice())
        .bind(added_ids.as_slice())
        .execute(&mut **txn)
        .await
        .map_err(write_denial)?;
    }
    Ok(live)
}

/// A module-union run: the pre-write live rows plus the added pairs in
/// enqueue order (issues outer, modules inner).
struct ModuleUnion {
    live: HashMap<Uuid, Vec<Uuid>>,
    pairs: Vec<(Uuid, Uuid)>,
}

#[allow(clippy::too_many_arguments)]
async fn union_module_bridges(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    now: DateTime<Utc>,
    issues: &[IssueRow],
    module_ids: &[Uuid],
) -> Result<ModuleUnion, Denial> {
    let issue_ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT issue_id, module_id FROM module_issues
           WHERE issue_id = ANY($1) AND deleted_at IS NULL ORDER BY created_at DESC"#,
    )
    .bind(issue_ids.as_slice())
    .fetch_all(&mut **txn)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut live: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut seen: HashSet<(Uuid, Uuid)> = HashSet::new();
    for (issue_id, module_id) in rows {
        live.entry(issue_id).or_default().push(module_id);
        seen.insert((issue_id, module_id));
    }
    let mut pairs: Vec<(Uuid, Uuid)> = Vec::new();
    for issue in issues {
        for module_id in module_ids {
            if !seen.contains(&(issue.id, *module_id)) {
                seen.insert((issue.id, *module_id));
                pairs.push((issue.id, *module_id));
            }
        }
    }
    if !pairs.is_empty() {
        let pair_issues: Vec<Uuid> = pairs.iter().map(|pair| pair.0).collect();
        let pair_modules: Vec<Uuid> = pairs.iter().map(|pair| pair.1).collect();
        sqlx::query(
            r#"INSERT INTO module_issues
               (id, created_at, updated_at, created_by_id, updated_by_id,
                issue_id, module_id, project_id, workspace_id)
               SELECT gen_random_uuid(), $1, $1, $2, $2, issue_id, module_id, $3, $4
               FROM unnest($5::uuid[], $6::uuid[]) AS t(issue_id, module_id)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(now)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(pair_issues.as_slice())
        .bind(pair_modules.as_slice())
        .execute(&mut **txn)
        .await
        .map_err(write_denial)?;
    }
    Ok(ModuleUnion { live, pairs })
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
    let ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT id, issue_id, cycle_id FROM cycle_issues
           WHERE issue_id = ANY($1) AND deleted_at IS NULL ORDER BY created_at DESC"#,
    )
    .bind(ids.as_slice())
    .fetch_all(&mut **txn)
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
        let row: (
            Uuid,
            Uuid,
            DateTime<Utc>,
            DateTime<Utc>,
            Option<Uuid>,
            Option<Uuid>,
        ) = sqlx::query_as(
            r#"INSERT INTO cycle_issues
                   (id, created_at, updated_at, created_by_id, updated_by_id,
                    project_id, workspace_id, cycle_id, issue_id)
                   VALUES (gen_random_uuid(), $1, $2, $3, $3, $4, $5, $6, $7)
                   RETURNING id, issue_id, created_at, updated_at,
                             created_by_id, updated_by_id"#,
        )
        .bind(created_at)
        .bind(updated_at)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(target)
        .bind(issue.id)
        .fetch_one(&mut **txn)
        .await
        .map_err(write_denial)?;
        created.push(CreatedBridge {
            id: row.0,
            issue_id: row.1,
            created_at: row.2,
            updated_at: row.3,
            created_by: row.4,
            updated_by: row.5,
            project_id: *project_id,
            workspace_id: *workspace_id,
            cycle_id: *target,
        });
    }
    let mut moved_activity: Vec<Value> = Vec::new();
    for (id, issue_id, old_cycle_id) in &moved {
        moved_activity.push(moved_bridge_json(old_cycle_id, target, issue_id));
        sqlx::query("UPDATE cycle_issues SET cycle_id = $1 WHERE id = $2")
            .bind(target)
            .bind(id)
            .execute(&mut **txn)
            .await
            .map_err(write_denial)?;
    }
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
/// keys (`str()`, so stored nulls render `"None"`).
fn previous_json(
    properties: &BulkProperties,
    issue: &IssueRow,
    live_labels: &HashMap<Uuid, Vec<Uuid>>,
    live_assignees: &HashMap<Uuid, Vec<Uuid>>,
    live_modules: &HashMap<Uuid, Vec<Uuid>>,
) -> Map<String, Value> {
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
    if properties
        .label_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty())
    {
        map.insert(
            "label_ids".to_owned(),
            uuid_list_json(live_labels.get(&issue.id).map_or(&[], Vec::as_slice)),
        );
    }
    if properties
        .assignee_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty())
    {
        map.insert(
            "assignee_ids".to_owned(),
            uuid_list_json(live_assignees.get(&issue.id).map_or(&[], Vec::as_slice)),
        );
    }
    if properties
        .module_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty())
    {
        map.insert(
            "module_ids".to_owned(),
            uuid_list_json(live_modules.get(&issue.id).map_or(&[], Vec::as_slice)),
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
/// `WEB_URL` — unset everywhere is a 500 (`ImproperlyConfigured`).
fn activity_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .or_else(|| urls.web_url.clone())
        .ok_or(Denial::ServerError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::body::FilePart;

    const IID: &str = "11111111-1111-1111-1111-111111111111";

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
        let properties = BulkProperties {
            state_id: Some(None),
            priority: Some("high".to_owned()),
            label_ids: Some(vec![]),
            ..BulkProperties::default()
        };
        let applied = applied_json(&properties, None);
        assert_eq!(
            to_spaced_json(&applied).unwrap(),
            r#"{"state_id": "None", "priority": "high"}"#
        );
        let issue = IssueRow {
            id: Uuid::parse_str(IID).unwrap(),
            state_id: Some(state),
            priority: "none".to_owned(),
            start_date: None,
            target_date: None,
            estimate_point_id: None,
        };
        let previous = previous_json(
            &properties,
            &issue,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        // The empty input array is falsy: neither payload carries it.
        assert_eq!(
            to_spaced_json(&previous).unwrap(),
            format!("{{\"state_id\": \"{state}\", \"priority\": \"none\"}}")
        );
    }
}
