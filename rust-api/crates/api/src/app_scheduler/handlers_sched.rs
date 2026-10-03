//! Workspace scheduler-definition endpoints (D-36, stage 5, PIDASHCONV-633).
//!
//! Ports `WorkspaceSchedulerListEndpoint` (GET list, POST create) and
//! `WorkspaceSchedulerDetailEndpoint` (GET, PATCH, DELETE) from
//! `apps/api/pi_dash/app/views/scheduler/views.py:46-157`, routes
//! `apps/api/pi_dash/app/urls/scheduler.py:18-27`
//! (`workspaces/<slug>/schedulers/`, `.../schedulers/<uuid>/`).
//!
//! Wiring only, no new logic: gates from [`super::gate`] (PIDASHCONV-632),
//! SQL from `pidash_services::app_scheduler::queries` (PIDASHCONV-630),
//! shapes from `pidash_services::app_scheduler::shape` (PIDASHCONV-629).
//! Fixture: `rust-api/fixtures/app_scheduler/handlers/scheduler_io.golden.json`
//! (F36-10; trace: `rust-api/fixtures/app_scheduler/TRACE.md`).
//!
//! Request order per method (Django's order, preserved):
//! session auth + decorator gate ([`super::gate::resolve_gate`]: 401/403),
//! then the `SCHEDULER_ENABLED` flag guard (404 disabled body), then the
//! handler body. Unknown slugs 403 inside the gate, never 404.
//!
//! DRF field semantics (`CharField`/`BooleanField`, required/null/blank/
//! max-length/type-coercion, error accumulation in field-declaration
//! order) are composed here from [`shape`] primitives, mirroring the
//! `app_pages` handler precedent; datetimes render in the actor zone via
//! [`crate::serializer::render_datetime_in`].
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * PATCH re-renders the saved in-memory row and keeps the PRE-SAVE
//!   `_active_binding_count` annotation (no re-read; accurate because
//!   PATCH never touches bindings — `views.py:127-133`, F36-10).
//! * Create re-renders the fresh row and runs the R6 fallback count
//!   (always 0 for a new scheduler — F36-01).
//! * DELETE samples `now()` twice (`:now` for the bindings update and the
//!   scheduler `deleted_at`, `:now2` for the scheduler `updated_at` —
//!   `views.py:150-157`, F36-04 R5).
//! * Duplicate slug answers 400 `{"error": "The payload is not valid"}`,
//!   never 409: no `UniqueTogetherValidator` exists (workspace is
//!   read-only), so the partial unique index raises `IntegrityError`,
//!   mapped by `BaseAPIView.handle_exception`
//!   (`app/views/base.py:211-240`, F36-10).
//! * A non-UUID detail id matches NO Django route, so Django's
//!   `custom_404_view` answers 404 `{"error": "Page not found."}` before
//!   any view code runs (F36-10 `routing_note`). The Rust route takes the
//!   id as a string and proxies unparseable ids to Django (the
//!   `app_pages` bad-id precedent), reproducing that body byte for byte.
//!
//! Documented approximations (no contract input covers them):
//!
//! * Lone-surrogate `\u` escapes fail JSON parsing in `serde_json`, so
//!   they answer the JSON-parse 400 where Django's `json` accepts them
//!   into a `str` and the surrogate validator answers its own 400.
//! * Non-string JSON scalars coerce via Rust number formatting, which
//!   differs from Python `repr` for scientific-notation floats (same
//!   choice as the `app_pages` `char_internal` precedent).

use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use sqlx::PgPool;

use pidash_services::app_scheduler::{queries, shape};

use super::gate::{self, Gate};
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// `get_object_or_404(Scheduler, ...)` miss (404): DRF renders the
/// `Http404("No Scheduler matches the given query.")` args through
/// `exception_handler` (`{'detail': ...}`). Byte-pinned against F36-10 in
/// [`tests::not_found_body_matches_f36_10`].
pub const SCHEDULER_NOT_FOUND_BODY: &str = r#"{"detail":"No Scheduler matches the given query."}"#;
/// `handle_exception`'s `IntegrityError` branch (400,
/// `app/views/base.py:220-224`): duplicate (workspace, slug) rows land
/// here on POST and PATCH, never a 409.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Malformed-JSON 400 message (`app_pages` precedent: DRF's `ParseError`
/// prefix; the `serde_json` suffix is backend-specific and not ported).
const JSON_PARSE_ERROR: &str = "JSON parse error";

// ---------------------------------------------------------------------------
// Handler-level denial (gate denials render through `gate::Denial`)
// ---------------------------------------------------------------------------

/// What a scheduler handler answers without running the happy path.
pub enum Denial {
    /// Scoped-lookup miss: 404 [`SCHEDULER_NOT_FOUND_BODY`].
    ObjectNotFound,
    /// 400, `{"detail": ...}` (body parse errors).
    BadDetail(String),
    /// 400, `{"error": ...}` (integrity branch).
    BadError(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// Database failure: 500 [`gate::SERVER_ERROR_BODY`].
    ServerError,
}

/// `{"detail": message}` envelope (DRF `exception_handler` shape for
/// scalar details). The key is pinned against the merged gate's 401 key
/// in [`tests::bad_detail_key_matches_gate_unauthenticated_key`].
fn detail_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("detail".to_owned(), Value::String(message));
    Value::Object(body)
}

/// `{"error": message}` envelope (`handle_exception` branch shape).
fn error_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("error".to_owned(), Value::String(message));
    Value::Object(body)
}

fn json_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("scheduler-handler response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::ObjectNotFound => {
                json_response(StatusCode::NOT_FOUND, SCHEDULER_NOT_FOUND_BODY)
            }
            Denial::BadDetail(message) => {
                (StatusCode::BAD_REQUEST, Json(detail_envelope(message))).into_response()
            }
            Denial::BadError(message) => {
                (StatusCode::BAD_REQUEST, Json(error_envelope(message))).into_response()
            }
            Denial::BadJson(value) => (StatusCode::BAD_REQUEST, Json(value)).into_response(),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, gate::SERVER_ERROR_BODY)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Placeholder conversion (`:named` in PARAMS order -> `$n`)
// ---------------------------------------------------------------------------

/// Convert a [`queries`] `:named` statement to sqlx `$n` placeholders.
///
/// Numbering follows `PARAMS` order ([`queries::PLACEHOLDER_RULE`]); a
/// name bound twice (INSERT's `:now`) shares one `$n` and is bound once.
/// A `:name` only matches when followed by a non-identifier byte, so
/// `:now` never matches inside `:now2`. Stray colons (none in the
/// scheduler statements) pass through untouched.
pub fn positional(sql: &str, params: &[&str]) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len() && is_ident_byte(bytes[j]) {
                j += 1;
            }
            if j > i + 1 {
                let name = &sql[i + 1..j];
                if let Some(position) = params.iter().position(|candidate| *candidate == name) {
                    out.push('$');
                    out.push_str(&(position + 1).to_string());
                    i = j;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

// ---------------------------------------------------------------------------
// Rows (`sqlx::FromRow` maps by column name; wide selects exceed the
// 16-tuple `FromRow` impls — the `license` handler precedent)
// ---------------------------------------------------------------------------

/// One `schedulers` row in fixture SELECT order (R1/R2/R5b without the
/// annotation leg). Also the in-memory image for create/PATCH renders.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SchedulerRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub source: String,
    pub is_enabled: bool,
    pub color: String,
}

/// One annotated scheduler row (R1 list / R2 detail): the scheduler
/// columns plus `_active_binding_count` (`COUNT ... FILTER`, never NULL).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AnnotatedSchedulerRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub source: String,
    pub is_enabled: bool,
    pub color: String,
    pub _active_binding_count: i64,
}

impl AnnotatedSchedulerRow {
    fn record(&self) -> SchedulerRow {
        SchedulerRow {
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by_id: self.created_by_id,
            updated_by_id: self.updated_by_id,
            deleted_at: self.deleted_at,
            id: self.id,
            workspace_id: self.workspace_id,
            slug: self.slug.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            prompt: self.prompt.clone(),
            source: self.source.clone(),
            is_enabled: self.is_enabled,
            color: self.color.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation (`SchedulerSerializer`, `scheduler.py:74-112`)
// ---------------------------------------------------------------------------

/// Writable scheduler columns in `Meta.fields` declaration order: error
/// objects preserve this order (DRF iterates `self.fields`).
const WRITABLE_FIELDS: &[&str] = &[
    "slug",
    "name",
    "description",
    "prompt",
    "color",
    "is_enabled",
];

const REQUIRED_MESSAGE: &str = "This field is required.";
const NULL_MESSAGE: &str = "This field may not be null.";
const INVALID_STRING_MESSAGE: &str = "Not a valid string.";
const BLANK_MESSAGE: &str = "This field may not be blank.";
const INVALID_BOOLEAN_MESSAGE: &str = "Must be a valid boolean.";
const NULL_CHARACTERS_MESSAGE: &str = "Null characters are not allowed.";

fn max_length_message(max_length: usize) -> String {
    format!("Ensure this field has no more than {max_length} characters.")
}

/// A validated scheduler write: all six writable columns resolved
/// (provided values, or model defaults on create / current values on
/// PATCH). PATCH writes every column (`save()` semantics, R4).
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulerWrite {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub color: String,
    pub is_enabled: bool,
}

/// Mirror DRF `CharField` input handling (`fields.py`
/// `CharField.run_validation` + `to_internal_value` + the
/// `max_length`/`ProhibitNullCharacters` validators): `null` fails,
/// bools/composites fail, numbers coerce via `str()`, the value strips
/// (`trim_whitespace`), blank (empty or whitespace-only) fails unless
/// `allow_blank`, then `max_length` (in CHARS, on the stripped value)
/// and the NUL check accumulate in validator order.
fn char_field(
    value: &Value,
    allow_blank: bool,
    max_length: Option<usize>,
) -> Result<String, Vec<String>> {
    if value.is_null() {
        return Err(vec![NULL_MESSAGE.to_owned()]);
    }
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(vec![INVALID_STRING_MESSAGE.to_owned()]),
    };
    if raw.is_empty() || raw.trim().is_empty() {
        if !allow_blank {
            return Err(vec![BLANK_MESSAGE.to_owned()]);
        }
        return Ok(String::new());
    }
    let stripped = raw.trim().to_owned();
    let mut errors = Vec::new();
    if let Some(max) = max_length {
        if stripped.chars().count() > max {
            errors.push(max_length_message(max));
        }
    }
    if stripped.contains('\0') {
        errors.push(NULL_CHARACTERS_MESSAGE.to_owned());
    }
    if errors.is_empty() {
        Ok(stripped)
    } else {
        Err(errors)
    }
}

/// Mirror DRF `BooleanField.to_internal_value` (`fields.py`): strings
/// compare lowercase-first against the TRUE/FALSE sets, `1`/`0`
/// (and `1.0`/`0.0`, by float equality) coerce, everything else fails.
/// `is_enabled` is not nullable, so `null` fails above the sets.
fn bool_field(value: &Value) -> Result<bool, Vec<String>> {
    if value.is_null() {
        return Err(vec![NULL_MESSAGE.to_owned()]);
    }
    let invalid = || vec![INVALID_BOOLEAN_MESSAGE.to_owned()];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                match int {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(invalid()),
                }
            } else if let Some(int) = number.as_u64() {
                match int {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(invalid()),
                }
            } else if let Some(float) = number.as_f64() {
                if float == 1.0 {
                    Ok(true)
                } else if float == 0.0 {
                    Ok(false)
                } else {
                    Err(invalid())
                }
            } else {
                Err(invalid())
            }
        }
        Value::String(text) => match text.to_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

/// Non-object top-level body (`serializers.py`): `null` fails the
/// null check and `Serializer.errors` rewrites the lone null-code
/// failure to `{"non_field_errors": ["No data provided"]}`; every other
/// non-dict answers `{"non_field_errors": ["Invalid data. Expected a
/// dictionary, but got {datatype}."]}` (`Serializer.to_internal_value`,
/// datatype via [`shape::json_type_name`]).
fn non_dict_body(body: &Value) -> Value {
    let message = if body.is_null() {
        "No data provided".to_owned()
    } else {
        format!(
            "Invalid data. Expected a dictionary, but got {}.",
            shape::json_type_name(body)
        )
    };
    let mut errors = Map::new();
    errors.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(message)]),
    );
    Value::Object(errors)
}

/// Validate one present field value into its write slot, collecting the
/// field's errors (`validate_color` runs only when the `CharField` half
/// passes, like DRF's `validate_<field>` hook).
fn validate_field(
    field: &str,
    value: &Value,
    write: &mut SchedulerWrite,
) -> Result<(), Vec<String>> {
    match field {
        "slug" => char_field(value, false, Some(64)).map(|slug| write.slug = slug),
        "name" => char_field(value, false, Some(255)).map(|name| write.name = name),
        "description" => {
            char_field(value, true, None).map(|description| write.description = description)
        }
        "prompt" => char_field(value, false, None).map(|prompt| write.prompt = prompt),
        "color" => {
            let stripped = char_field(value, false, Some(7))?;
            shape::validate_color(&stripped)
                .map(|color| write.color = color)
                .map_err(|message| vec![message.to_owned()])
        }
        "is_enabled" => bool_field(value).map(|flag| write.is_enabled = flag),
        _ => Ok(()),
    }
}

/// Validate a create body (`SchedulerSerializer(data)`,
/// `views.py:79-80`): unknown and read-only keys ignored, every missing
/// required key (`slug`, `name`, `prompt`) fails, missing optionals take
/// their model defaults (`description` `""`, `color` `"#3b82f6"`,
/// `is_enabled` true). All field errors accumulate in declaration order.
pub fn validate_create_body(body: &Value) -> Result<SchedulerWrite, Value> {
    let object = match body.as_object() {
        Some(map) => map,
        None => return Err(non_dict_body(body)),
    };
    let mut write = SchedulerWrite {
        slug: String::new(),
        name: String::new(),
        description: String::new(),
        prompt: String::new(),
        color: "#3b82f6".to_owned(),
        is_enabled: true,
    };
    let mut errors = Map::new();
    for &field in WRITABLE_FIELDS {
        match object.get(field) {
            Some(value) => {
                if let Err(messages) = validate_field(field, value, &mut write) {
                    errors.insert(
                        field.to_owned(),
                        Value::Array(messages.into_iter().map(Value::String).collect()),
                    );
                }
            }
            None => {
                if matches!(field, "slug" | "name" | "prompt") {
                    errors.insert(
                        field.to_owned(),
                        Value::Array(vec![Value::String(REQUIRED_MESSAGE.to_owned())]),
                    );
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(write)
    } else {
        Err(Value::Object(errors))
    }
}

/// Validate a PATCH body (`SchedulerSerializer(scheduler, data,
/// partial=True)`, `views.py:127-128`): only present keys validate;
/// provided values merge over the current row for the full-column R4
/// write. The response re-renders this merged image (see
/// [`scheduler_patch`]).
pub fn validate_patch_body(body: &Value, existing: &SchedulerRow) -> Result<SchedulerWrite, Value> {
    let object = match body.as_object() {
        Some(map) => map,
        None => return Err(non_dict_body(body)),
    };
    let mut write = SchedulerWrite {
        slug: existing.slug.clone(),
        name: existing.name.clone(),
        description: existing.description.clone(),
        prompt: existing.prompt.clone(),
        color: existing.color.clone(),
        is_enabled: existing.is_enabled,
    };
    let mut errors = Map::new();
    for &field in WRITABLE_FIELDS {
        if let Some(value) = object.get(field) {
            if let Err(messages) = validate_field(field, value, &mut write) {
                errors.insert(
                    field.to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(write)
    } else {
        Err(Value::Object(errors))
    }
}

// ---------------------------------------------------------------------------
// Render (`SchedulerSerializer` read half, `scheduler.py:74-112`)
// ---------------------------------------------------------------------------

/// Render one scheduler row in `Meta.fields` order (F36-01):
/// `active_binding_count` is the resolved count (annotation or R6
/// fallback, decided by the caller per [`shape::resolve_active_binding_count`]
/// semantics), datetimes render in the actor zone, `color` renders
/// as stored (no normalization on read).
pub fn render_scheduler_row(row: &SchedulerRow, active_binding_count: i64, tz: &Tz) -> Value {
    let mut rendered = Map::new();
    rendered.insert("id".to_owned(), Value::String(row.id.to_string()));
    rendered.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    rendered.insert("slug".to_owned(), Value::String(row.slug.clone()));
    rendered.insert("name".to_owned(), Value::String(row.name.clone()));
    rendered.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    rendered.insert("prompt".to_owned(), Value::String(row.prompt.clone()));
    rendered.insert("color".to_owned(), Value::String(row.color.clone()));
    rendered.insert("source".to_owned(), Value::String(row.source.clone()));
    rendered.insert("is_enabled".to_owned(), Value::Bool(row.is_enabled));
    rendered.insert(
        "active_binding_count".to_owned(),
        Value::Number(active_binding_count.into()),
    );
    rendered.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.created_at, tz)),
    );
    rendered.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.updated_at, tz)),
    );
    Value::Object(rendered)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `request.data` (`app_pages` precedent): empty bodies validate as
/// `{}`; anything else must parse as JSON.
async fn read_json_body(req: Request) -> Result<Value, Denial> {
    let (_parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Err(Denial::ServerError),
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes).map_err(|_| Denial::BadDetail(JSON_PARSE_ERROR.to_owned()))
}

/// Whether a sqlx failure is an integrity violation (unique, FK,
/// not-null, check — SQLSTATE class `23`), which Django's
/// `handle_exception` answers 400 for (`app_pages` precedent).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"))
}

/// `GET /api/workspaces/<slug>/schedulers/` (`views.py:55-72`): any
/// workspace member lists the workspace's schedulers ordered by name,
/// each with its annotated `active_binding_count`.
pub async fn scheduler_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let gate = match gate::resolve_gate(&state, &Gate::WorkspaceOpen, &slug, None, extension).await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::ensure_feature_enabled(state.settings()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let sql = positional(queries::SCHEDULER_LIST_SQL, queries::SCHEDULER_LIST_PARAMS);
    let rows: Vec<AnnotatedSchedulerRow> = match sqlx::query_as(&sql)
        .bind(gate.workspace_id)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let rendered: Vec<Value> = rows
        .iter()
        .map(|row| {
            render_scheduler_row(
                &row.record(),
                shape::resolve_active_binding_count(Some(row._active_binding_count), 0),
                &gate.timezone,
            )
        })
        .collect();
    (StatusCode::OK, Json(Value::Array(rendered))).into_response()
}

/// `POST /api/workspaces/<slug>/schedulers/` (`views.py:75-85`):
/// workspace admins create a scheduler; the 201 re-renders the fresh
/// row with the R6 fallback count.
pub async fn scheduler_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let gate = match gate::resolve_gate(&state, &Gate::WorkspaceAdmin, &slug, None, extension).await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::ensure_feature_enabled(state.settings()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let write = match validate_create_body(&body) {
        Ok(write) => write,
        Err(errors) => return Denial::BadJson(errors).into_response(),
    };
    // R3 (`:79-84`): auto id/timestamps, `created_by` the request user
    // (crum), `updated_by`/`deleted_at` NULL, `source` the `'builtin'`
    // literal. One `:now` sample feeds both timestamps (queries-layer
    // note: Django samples per field, unobservable at ~1us).
    let id = uuid::Uuid::new_v4();
    let now = Utc::now();
    let sql = positional(
        queries::SCHEDULER_INSERT_SQL,
        queries::SCHEDULER_INSERT_PARAMS,
    );
    let insert = sqlx::query(&sql)
        .bind(id)
        .bind(now)
        .bind(gate.user_id)
        .bind(gate.workspace_id)
        .bind(&write.slug)
        .bind(&write.name)
        .bind(&write.description)
        .bind(&write.prompt)
        .bind(write.is_enabled)
        .bind(&write.color)
        .execute(pool)
        .await;
    if let Err(error) = insert {
        if is_integrity_error(&error) {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
        return Denial::ServerError.into_response();
    }
    // The create response serializes the fresh un-annotated row, so the
    // count falls back to the R6 query (always 0 for a new scheduler).
    let count_sql = positional(
        queries::ACTIVE_BINDING_COUNT_SQL,
        queries::ACTIVE_BINDING_COUNT_PARAMS,
    );
    let fallback: Option<(i64,)> = match sqlx::query_as(&count_sql)
        .bind(id)
        .fetch_optional(pool)
        .await
    {
        Ok(count) => count,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let count = shape::resolve_active_binding_count(None, fallback.map(|row| row.0).unwrap_or(0));
    let row = SchedulerRow {
        created_at: now,
        updated_at: now,
        created_by_id: Some(gate.user_id),
        updated_by_id: None,
        deleted_at: None,
        id,
        workspace_id: gate.workspace_id,
        slug: write.slug,
        name: write.name,
        description: write.description,
        prompt: write.prompt,
        source: "builtin".to_owned(),
        is_enabled: write.is_enabled,
        color: write.color,
    };
    (
        StatusCode::CREATED,
        Json(render_scheduler_row(&row, count, &gate.timezone)),
    )
        .into_response()
}

/// `GET /api/workspaces/<slug>/schedulers/<uuid>/` (`views.py:95-111`):
/// workspace admins read one scheduler with its annotated count.
pub async fn scheduler_detail(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(scheduler_id) = scheduler_raw.parse::<uuid::Uuid>() else {
        return crate::edge::proxy(State(state), req).await;
    };
    let gate = match gate::resolve_gate(&state, &Gate::WorkspaceAdmin, &slug, None, extension).await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::ensure_feature_enabled(state.settings()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let sql = positional(
        queries::SCHEDULER_DETAIL_SQL,
        queries::SCHEDULER_DETAIL_PARAMS,
    );
    let row: Option<AnnotatedSchedulerRow> = match sqlx::query_as(&sql)
        .bind(scheduler_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(row) = row else {
        return Denial::ObjectNotFound.into_response();
    };
    let rendered = render_scheduler_row(
        &row.record(),
        shape::resolve_active_binding_count(Some(row._active_binding_count), 0),
        &gate.timezone,
    );
    (StatusCode::OK, Json(rendered)).into_response()
}

/// `PATCH /api/workspaces/<slug>/schedulers/<uuid>/`
/// (`views.py:114-133`): partial validate, full-column save, 200 with
/// the saved in-memory row (the pre-save annotation kept).
pub async fn scheduler_patch(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(scheduler_id) = scheduler_raw.parse::<uuid::Uuid>() else {
        return crate::edge::proxy(State(state), req).await;
    };
    let gate = match gate::resolve_gate(&state, &Gate::WorkspaceAdmin, &slug, None, extension).await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::ensure_feature_enabled(state.settings()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let sql = positional(
        queries::SCHEDULER_DETAIL_SQL,
        queries::SCHEDULER_DETAIL_PARAMS,
    );
    let row: Option<AnnotatedSchedulerRow> = match sqlx::query_as(&sql)
        .bind(scheduler_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(row) = row else {
        return Denial::ObjectNotFound.into_response();
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let record = row.record();
    let write = match validate_patch_body(&body, &record) {
        Ok(write) => write,
        Err(errors) => return Denial::BadJson(errors).into_response(),
    };
    // R4 (`:127-133`): `save()` rewrites every writable column with the
    // merged values and refreshes `updated_at`/`updated_by_id`.
    let now = Utc::now();
    let patch_sql = positional(
        queries::SCHEDULER_PATCH_SQL,
        queries::SCHEDULER_PATCH_PARAMS,
    );
    let update = sqlx::query(&patch_sql)
        .bind(now)
        .bind(gate.user_id)
        .bind(&write.slug)
        .bind(&write.name)
        .bind(&write.description)
        .bind(&write.prompt)
        .bind(&write.color)
        .bind(write.is_enabled)
        .bind(scheduler_id)
        .execute(pool)
        .await;
    if let Err(error) = update {
        if is_integrity_error(&error) {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
        return Denial::ServerError.into_response();
    }
    let saved = SchedulerRow {
        updated_at: now,
        updated_by_id: Some(gate.user_id),
        slug: write.slug,
        name: write.name,
        description: write.description,
        prompt: write.prompt,
        color: write.color,
        is_enabled: write.is_enabled,
        ..record
    };
    let rendered = render_scheduler_row(
        &saved,
        shape::resolve_active_binding_count(Some(row._active_binding_count), 0),
        &gate.timezone,
    );
    (StatusCode::OK, Json(rendered)).into_response()
}

/// `DELETE /api/workspaces/<slug>/schedulers/<uuid>/`
/// (`views.py:136-157`): one transaction soft-deletes the still-active
/// bindings inline and then the scheduler; 204 with an empty body.
pub async fn scheduler_delete(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(scheduler_id) = scheduler_raw.parse::<uuid::Uuid>() else {
        return crate::edge::proxy(State(state), req).await;
    };
    let gate = match gate::resolve_gate(&state, &Gate::WorkspaceAdmin, &slug, None, extension).await
    {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate::ensure_feature_enabled(state.settings()) {
        return denial.into_response();
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // R5 lookup (`:139-141`): the un-annotated row; only existence
    // matters (404 before the transaction opens).
    let lookup_sql = positional(
        queries::SCHEDULER_DELETE_LOOKUP_SQL,
        queries::SCHEDULER_DELETE_LOOKUP_PARAMS,
    );
    let row: Option<SchedulerRow> = match sqlx::query_as(&lookup_sql)
        .bind(scheduler_id)
        .bind(&slug)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if row.is_none() {
        return Denial::ObjectNotFound.into_response();
    }
    // `transaction.atomic()` (`:151-156`): two separate `now()` samples
    // (ported quirk 5), bindings first so no active binding survives.
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let now = Utc::now();
    let bindings_sql = positional(
        queries::SCHEDULER_DELETE_BINDINGS_SQL,
        queries::SCHEDULER_DELETE_BINDINGS_PARAMS,
    );
    if sqlx::query(&bindings_sql)
        .bind(now)
        .bind(scheduler_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let now2 = Utc::now();
    let soft_delete_sql = positional(
        queries::SCHEDULER_SOFT_DELETE_SQL,
        queries::SCHEDULER_SOFT_DELETE_PARAMS,
    );
    if sqlx::query(&soft_delete_sql)
        .bind(now)
        .bind(now2)
        .bind(gate.user_id)
        .bind(scheduler_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// A scheduler path: the owned methods serve from Rust, everything else
/// falls through to Django (its 405s and DRF metadata live there).
/// OPTIONS proxies too: DRF answers metadata (401 anon / 200 authed)
/// where axum would 405.
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
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Scheduler-definition routes (`app/urls/scheduler.py:18-27`).
/// Sibling D-36 handler files expose their own `routes()`; the module
/// `routes()` merges them (merges keep both sides).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/schedulers/",
            owned(
                get(scheduler_list).post(scheduler_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/schedulers/{scheduler_id}/",
            owned(
                get(scheduler_detail)
                    .patch(scheduler_patch)
                    .delete(scheduler_delete),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE_SCHEDULER_IO: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/handlers/scheduler_io.golden.json"
    );
    const FIXTURE_SCHEDULER_SHAPES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/serializers/scheduler_shapes.golden.json"
    );

    fn fixture_json(path: &str) -> Value {
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn errors_for(body: Value) -> Map<String, Value> {
        match validate_create_body(&body) {
            Ok(_) => panic!("expected validation errors for {body}"),
            Err(Value::Object(map)) => map,
            Err(other) => panic!("expected error object, got {other}"),
        }
    }

    fn sample_row() -> SchedulerRow {
        SchedulerRow {
            created_at: "2024-05-06T07:08:09Z".parse().expect("ts"),
            updated_at: "2024-05-06T07:08:09Z".parse().expect("ts"),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: "0d065023-aa70-4d7d-85da-029d7169843e"
                .parse()
                .expect("uuid"),
            workspace_id: "6f819bce-fba0-4ef5-9b16-187eea21a6a1"
                .parse()
                .expect("uuid"),
            slug: "nightly".to_owned(),
            name: "Nightly".to_owned(),
            description: "d".to_owned(),
            prompt: "Scan.".to_owned(),
            source: "builtin".to_owned(),
            is_enabled: true,
            color: "#10B981".to_owned(),
        }
    }

    // -- F36-10 bodies ----------------------------------------------------

    /// Every `missing` body in F36-10, byte for byte.
    #[test]
    fn not_found_body_matches_f36_10() {
        let fixture = fixture_json(FIXTURE_SCHEDULER_IO);
        let actions = fixture["actions"].as_array().expect("actions array");
        let mut checked = 0;
        for action in actions {
            if let Some(missing) = action.get("missing") {
                let expected = serde_json::to_string(&missing["body"]).expect("body JSON");
                assert_eq!(SCHEDULER_NOT_FOUND_BODY, expected);
                assert_eq!(missing["status"], 404);
                checked += 1;
            }
        }
        assert_eq!(checked, 3, "detail GET + PATCH + DELETE miss bodies");
    }

    /// The `BadDetail` envelope key is mechanically tied to the merged
    /// gate's 401 key (no hand-typed key literal is trusted).
    #[test]
    fn bad_detail_key_matches_gate_unauthenticated_key() {
        let gate_body: Value =
            serde_json::from_str(gate::UNAUTHENTICATED_BODY).expect("gate 401 JSON");
        let gate_key = gate_body
            .as_object()
            .expect("gate 401 object")
            .keys()
            .next()
            .expect("gate 401 key")
            .clone();
        let mine = detail_envelope("probe".to_owned());
        let mine_key = mine
            .as_object()
            .expect("envelope object")
            .keys()
            .next()
            .expect("envelope key")
            .clone();
        assert_eq!(mine_key, gate_key);
    }

    /// The integrity body is byte-identical to the reviewed `app_pages`
    /// precedent const (and therefore to the contract pin).
    #[test]
    fn invalid_payload_body_matches_precedent() {
        assert_eq!(INVALID_PAYLOAD_BODY, crate::app_pages::INVALID_PAYLOAD_BODY);
    }

    // -- Placeholder conversion --------------------------------------------

    #[test]
    fn positional_numbers_params_in_order() {
        assert_eq!(positional(":a :b :a", &["a", "b"]), "$1 $2 $1");
    }

    #[test]
    fn positional_never_matches_inside_longer_names() {
        assert_eq!(positional(":now :now2", &["now", "now2"]), "$1 $2");
        assert_eq!(positional(":now2 :now", &["now", "now2"]), "$2 $1");
    }

    #[test]
    fn positional_leaves_stray_colons_untouched() {
        assert_eq!(positional("a::b 'x:y'", &["q"]), "a::b 'x:y'");
    }

    /// Every scheduler statement converts with no `:named` remnant, and
    /// the highest `$n` equals the PARAMS length.
    #[test]
    fn positional_converts_all_scheduler_statements() {
        let statements = [
            (queries::SCHEDULER_LIST_SQL, queries::SCHEDULER_LIST_PARAMS),
            (
                queries::SCHEDULER_DETAIL_SQL,
                queries::SCHEDULER_DETAIL_PARAMS,
            ),
            (
                queries::SCHEDULER_DELETE_LOOKUP_SQL,
                queries::SCHEDULER_DELETE_LOOKUP_PARAMS,
            ),
            (
                queries::ACTIVE_BINDING_COUNT_SQL,
                queries::ACTIVE_BINDING_COUNT_PARAMS,
            ),
            (
                queries::SCHEDULER_INSERT_SQL,
                queries::SCHEDULER_INSERT_PARAMS,
            ),
            (
                queries::SCHEDULER_PATCH_SQL,
                queries::SCHEDULER_PATCH_PARAMS,
            ),
            (
                queries::SCHEDULER_DELETE_BINDINGS_SQL,
                queries::SCHEDULER_DELETE_BINDINGS_PARAMS,
            ),
            (
                queries::SCHEDULER_SOFT_DELETE_SQL,
                queries::SCHEDULER_SOFT_DELETE_PARAMS,
            ),
        ];
        for (sql, params) in statements {
            let converted = positional(sql, params);
            assert!(!converted.contains(':'), "no :named remnant in {converted}");
            for (index, name) in params.iter().enumerate() {
                let marker = format!("${}", index + 1);
                assert!(
                    converted.contains(&marker),
                    "{name} became {marker} in {converted}"
                );
            }
        }
    }

    /// Full converted INSERT text: repeated `:now` shares `$2`, the
    /// `'builtin'` literal survives untouched.
    #[test]
    fn positional_insert_text() {
        let converted = positional(
            queries::SCHEDULER_INSERT_SQL,
            queries::SCHEDULER_INSERT_PARAMS,
        );
        assert!(converted.contains(
            "VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, 'builtin', $9, $10);"
        ));
    }

    // -- Create validation ---------------------------------------------------

    #[test]
    fn create_valid_minimal_takes_model_defaults() {
        let write = validate_create_body(&json!({"slug": "s", "name": "N", "prompt": "x"}))
            .expect("minimal create validates");
        assert_eq!(
            write,
            SchedulerWrite {
                slug: "s".to_owned(),
                name: "N".to_owned(),
                description: String::new(),
                prompt: "x".to_owned(),
                color: "#3b82f6".to_owned(),
                is_enabled: true,
            }
        );
    }

    #[test]
    fn create_valid_full_canonicalizes() {
        let write = validate_create_body(&json!({
            "slug": "  nightly ",
            "name": "Nightly",
            "description": "d",
            "prompt": "Scan.",
            "color": "  #ABCDEF  ",
            "is_enabled": false,
        }))
        .expect("full create validates");
        assert_eq!(write.slug, "nightly");
        assert_eq!(write.color, "#abcdef");
        assert!(!write.is_enabled);
    }

    #[test]
    fn create_missing_required_accumulates_in_field_order() {
        let errors = errors_for(json!({}));
        let keys: Vec<&String> = errors.keys().collect();
        assert_eq!(keys, vec!["slug", "name", "prompt"]);
        for key in keys {
            assert_eq!(
                errors[key],
                json!(["This field is required."]),
                "{key} required message"
            );
        }
    }

    #[test]
    fn create_null_rejected_on_every_writable_field() {
        for field in [
            "slug",
            "name",
            "description",
            "prompt",
            "color",
            "is_enabled",
        ] {
            let mut body = Map::new();
            body.insert(field.to_owned(), Value::Null);
            // Fill the other required fields so only this field errors.
            for required in ["slug", "name", "prompt"] {
                if required != field {
                    body.insert(required.to_owned(), Value::String("v".to_owned()));
                }
            }
            let errors = errors_for(Value::Object(body));
            assert_eq!(
                errors[field],
                json!(["This field may not be null."]),
                "{field} null message"
            );
        }
    }

    #[test]
    fn create_blank_rules_match_allow_blank() {
        // slug/name/prompt/color reject blank; description coerces to "".
        for field in ["slug", "name", "prompt", "color"] {
            let errors = errors_for(json!({"slug": "s", "name": "n", "prompt": "p", field: ""}));
            assert_eq!(
                errors[field],
                json!(["This field may not be blank."]),
                "{field} blank message"
            );
        }
        let write = validate_create_body(&json!({
            "slug": "s", "name": "n", "prompt": "p", "description": "   ",
        }))
        .expect("whitespace description coerces");
        assert_eq!(write.description, "");
    }

    #[test]
    fn create_max_length_counts_chars_not_bytes() {
        let emoji_slug: String = "e".repeat(63) + "\u{1F600}";
        assert_eq!(emoji_slug.chars().count(), 64);
        validate_create_body(&json!({"slug": emoji_slug, "name": "n", "prompt": "p"}))
            .expect("64 chars (67 bytes) fit max_length 64");
        let long_slug = "s".repeat(65);
        let errors = errors_for(json!({"slug": long_slug, "name": "n", "prompt": "p"}));
        assert_eq!(
            errors["slug"],
            json!(["Ensure this field has no more than 64 characters."])
        );
        let long_name = "n".repeat(256);
        let errors = errors_for(json!({"slug": "s", "name": long_name, "prompt": "p"}));
        assert_eq!(
            errors["name"],
            json!(["Ensure this field has no more than 255 characters."])
        );
    }

    /// The exact contract pin (`test_create_bad_color`).
    #[test]
    fn create_bad_color_body_is_contract_exact() {
        let errors = errors_for(json!({
            "slug": "bad", "name": "Bad", "prompt": "x", "color": "red",
        }));
        assert_eq!(
            Value::Object(errors),
            json!({"color": ["color must be a 7-character hex string like '#3b82f6'"]})
        );
    }

    /// F36-01 color cases, including the max_length/blank preemptions.
    #[test]
    fn create_color_cases_match_f36_01() {
        let cases = [
            ("#10B981", Ok("#10b981")),
            ("  #ABCDEF  ", Ok("#abcdef")),
            (
                "red",
                Err("color must be a 7-character hex string like '#3b82f6'"),
            ),
            (
                "#12345",
                Err("color must be a 7-character hex string like '#3b82f6'"),
            ),
            (
                "#12345678",
                Err("Ensure this field has no more than 7 characters."),
            ),
            ("", Err("This field may not be blank.")),
        ];
        for (input, expected) in cases {
            let result = validate_create_body(&json!({
                "slug": "s", "name": "n", "prompt": "p", "color": input,
            }));
            match expected {
                Ok(color) => {
                    assert_eq!(result.expect("valid color").color, color, "color {input:?}")
                }
                Err(message) => {
                    let errors = errors_for(json!({
                        "slug": "s", "name": "n", "prompt": "p", "color": input,
                    }));
                    assert_eq!(errors["color"], json!([message]), "color {input:?}");
                    assert!(result.is_err());
                }
            }
        }
    }

    #[test]
    fn create_boolean_sets_match_drf() {
        let truthy: Vec<Value> = vec![
            json!(true),
            json!(1),
            json!(1.0),
            json!("t"),
            json!("T"),
            json!("y"),
            json!("YES"),
            json!("true"),
            json!("True"),
            json!("on"),
            json!("1"),
        ];
        for value in truthy {
            let write = validate_create_body(&json!({
                "slug": "s", "name": "n", "prompt": "p", "is_enabled": value,
            }))
            .expect("truthy coerces");
            assert!(write.is_enabled, "is_enabled {value}");
        }
        let falsy: Vec<Value> = vec![
            json!(false),
            json!(0),
            json!(0.0),
            json!("f"),
            json!("N"),
            json!("no"),
            json!("FALSE"),
            json!("off"),
            json!("0"),
        ];
        for value in falsy {
            let write = validate_create_body(&json!({
                "slug": "s", "name": "n", "prompt": "p", "is_enabled": value,
            }))
            .expect("falsy coerces");
            assert!(!write.is_enabled, "is_enabled {value}");
        }
        for value in [
            json!(2),
            json!(0.5),
            json!("maybe"),
            json!(""),
            json!([]),
            json!({}),
        ] {
            let errors = errors_for(json!({
                "slug": "s", "name": "n", "prompt": "p", "is_enabled": value,
            }));
            assert_eq!(
                errors["is_enabled"],
                json!(["Must be a valid boolean."]),
                "is_enabled {value}"
            );
        }
    }

    #[test]
    fn create_scalar_coercion_matches_charfield() {
        let write = validate_create_body(&json!({"slug": 5, "name": "n", "prompt": "p"}))
            .expect("int coerces");
        assert_eq!(write.slug, "5");
        let write = validate_create_body(&json!({"slug": "s", "name": 5.5, "prompt": "p"}))
            .expect("float coerces");
        assert_eq!(write.name, "5.5");
        for (field, value) in [
            ("slug", json!(true)),
            ("slug", json!([1])),
            ("name", json!({"a": 1})),
        ] {
            let errors = errors_for(json!({"slug": "s", "name": "n", "prompt": "p", field: value}));
            assert_eq!(
                errors[field],
                json!(["Not a valid string."]),
                "{field} {value}"
            );
        }
    }

    #[test]
    fn create_nul_rejected_after_max_length() {
        let errors = errors_for(json!({
            "slug": "a\0b", "name": "n", "prompt": "p",
        }));
        assert_eq!(errors["slug"], json!(["Null characters are not allowed."]));
        let long_nul = "s".repeat(70) + "\0";
        let errors = errors_for(json!({"slug": long_nul, "name": "n", "prompt": "p"}));
        assert_eq!(
            errors["slug"],
            json!([
                "Ensure this field has no more than 64 characters.",
                "Null characters are not allowed."
            ])
        );
    }

    #[test]
    fn create_read_only_and_unknown_keys_ignored() {
        let write = validate_create_body(&json!({
            "slug": "s",
            "name": "n",
            "prompt": "p",
            "id": "00000000-0000-0000-0000-000000000000",
            "workspace": "00000000-0000-0000-0000-000000000000",
            "source": "manifest",
            "active_binding_count": 42,
            "created_at": "2020-01-01T00:00:00Z",
            "updated_at": "2020-01-01T00:00:00Z",
            "unknown_key": [1, 2, 3],
        }))
        .expect("read-only + unknown keys ignored");
        assert_eq!(write.slug, "s");
        assert_eq!(write.color, "#3b82f6");
        assert!(write.is_enabled);
    }

    #[test]
    fn create_non_dict_bodies_name_the_datatype() {
        for (body, datatype) in [
            (json!([1, 2]), "list"),
            (json!("x"), "str"),
            (json!(5), "int"),
            (json!(5.5), "float"),
            (json!(true), "bool"),
        ] {
            let errors = errors_for(body);
            assert_eq!(
                Value::Object(errors),
                json!({"non_field_errors": [
                    format!("Invalid data. Expected a dictionary, but got {datatype}.")
                ]}),
                "datatype {datatype}"
            );
        }
        // `null` is the lone-null edge: `Serializer.errors` rewrites it.
        assert_eq!(
            Value::Object(errors_for(json!(null))),
            json!({"non_field_errors": ["No data provided"]})
        );
    }

    #[test]
    fn create_multi_field_errors_follow_declaration_order() {
        let errors = errors_for(json!({
            "is_enabled": "nope",
            "color": "red",
            "name": 5,
        }));
        let keys: Vec<&String> = errors.keys().collect();
        assert_eq!(keys, vec!["slug", "prompt", "color", "is_enabled"]);
    }

    /// Single-field error serialization equals the mandated
    /// [`shape::field_error_body`] builder output.
    #[test]
    fn single_field_errors_match_shape_builder() {
        let errors = errors_for(json!({
            "slug": "bad", "name": "Bad", "prompt": "x", "color": "red",
        }));
        let rendered = serde_json::to_string(&Value::Object(errors)).expect("errors JSON");
        assert_eq!(
            rendered,
            shape::field_error_body(
                "color",
                "color must be a 7-character hex string like '#3b82f6'"
            )
        );
    }

    // -- PATCH validation -----------------------------------------------------

    #[test]
    fn patch_partial_merges_over_current_row() {
        let write = validate_patch_body(
            &json!({"name": "Renamed", "color": "#FF0000", "is_enabled": false}),
            &sample_row(),
        )
        .expect("patch validates");
        assert_eq!(
            write,
            SchedulerWrite {
                slug: "nightly".to_owned(),
                name: "Renamed".to_owned(),
                description: "d".to_owned(),
                prompt: "Scan.".to_owned(),
                color: "#ff0000".to_owned(),
                is_enabled: false,
            }
        );
    }

    #[test]
    fn patch_empty_body_keeps_everything() {
        let existing = sample_row();
        let write = validate_patch_body(&json!({}), &existing).expect("empty patch validates");
        assert_eq!(write.slug, existing.slug);
        assert_eq!(write.name, existing.name);
        assert_eq!(write.description, existing.description);
        assert_eq!(write.prompt, existing.prompt);
        assert_eq!(write.color, existing.color);
        assert_eq!(write.is_enabled, existing.is_enabled);
    }

    #[test]
    fn patch_validates_only_present_keys() {
        // Missing required keys are fine on PATCH; a present bad key fails.
        validate_patch_body(&json!({"name": "x"}), &sample_row()).expect("partial ok");
        let errors = match validate_patch_body(&json!({"color": "nope"}), &sample_row()) {
            Ok(_) => panic!("expected color error"),
            Err(Value::Object(map)) => map,
            Err(other) => panic!("expected error object, got {other}"),
        };
        assert!(errors.contains_key("color"));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn patch_non_dict_rejected() {
        for (body, message) in [
            (
                json!([1]),
                "Invalid data. Expected a dictionary, but got list.",
            ),
            (json!(null), "No data provided"),
        ] {
            let errors = match validate_patch_body(&body, &sample_row()) {
                Ok(_) => panic!("expected non-dict error for {body}"),
                Err(Value::Object(map)) => map,
                Err(other) => panic!("expected error object, got {other}"),
            };
            assert_eq!(
                Value::Object(errors),
                json!({"non_field_errors": [message]}),
                "body {body}"
            );
        }
    }

    // -- Render ---------------------------------------------------------------

    /// Output key order equals the `Meta.fields` list order (F36-01).
    #[test]
    fn render_key_order_matches_serializer_fields() {
        let rendered = render_scheduler_row(&sample_row(), 3, &chrono_tz::UTC);
        let keys: Vec<&String> = rendered
            .as_object()
            .expect("render object")
            .keys()
            .collect();
        let expected: Vec<&str> = shape::SCHEDULER_SERIALIZER_FIELDS.to_vec();
        assert_eq!(keys, expected.iter().collect::<Vec<_>>());
    }

    /// The F36-01 render example, byte for byte (color as stored,
    /// datetimes with Z, ids as UUID strings).
    #[test]
    fn render_example_matches_f36_01() {
        let fixture = fixture_json(FIXTURE_SCHEDULER_SHAPES);
        let example = &fixture["render_example"];
        let row = SchedulerRow {
            created_at: example["created_at"]
                .as_str()
                .expect("created_at")
                .parse()
                .expect("ts"),
            updated_at: example["updated_at"]
                .as_str()
                .expect("updated_at")
                .parse()
                .expect("ts"),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: example["id"].as_str().expect("id").parse().expect("uuid"),
            workspace_id: example["workspace"]
                .as_str()
                .expect("workspace")
                .parse()
                .expect("uuid"),
            slug: example["slug"].as_str().expect("slug").to_owned(),
            name: example["name"].as_str().expect("name").to_owned(),
            description: example["description"]
                .as_str()
                .expect("description")
                .to_owned(),
            prompt: example["prompt"].as_str().expect("prompt").to_owned(),
            source: example["source"].as_str().expect("source").to_owned(),
            is_enabled: example["is_enabled"].as_bool().expect("is_enabled"),
            color: example["color"].as_str().expect("color").to_owned(),
        };
        let count = example["active_binding_count"].as_i64().expect("count");
        let rendered = render_scheduler_row(&row, count, &chrono_tz::UTC);
        // Values byte for byte (key order is pinned separately by
        // `render_key_order_matches_serializer_fields`: the fixture file
        // stores the example alphabetically, the wire emits fields order).
        let rendered_obj = rendered.as_object().expect("render object");
        let example_obj = example.as_object().expect("example object");
        assert_eq!(rendered_obj.len(), example_obj.len());
        for (key, expected) in example_obj {
            assert_eq!(&rendered_obj[key], expected, "field {key}");
        }
    }

    #[test]
    fn render_datetimes_follow_actor_zone() {
        let rendered = render_scheduler_row(&sample_row(), 0, &chrono_tz::UTC);
        assert_eq!(rendered["created_at"], json!("2024-05-06T07:08:09Z"));
        let eastern: Tz = "America/New_York".parse().expect("tz");
        let rendered = render_scheduler_row(&sample_row(), 0, &eastern);
        assert_eq!(rendered["created_at"], json!("2024-05-06T03:08:09-04:00"));
    }
}
