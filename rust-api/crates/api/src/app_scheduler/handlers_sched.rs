//! Workspace scheduler-definition endpoints (D-36, stage 5, PIDASHCONV-633).
//!
//! Ports `WorkspaceSchedulerListEndpoint` (GET list, POST create) and
//! `WorkspaceSchedulerDetailEndpoint` (GET, PATCH, DELETE) from
//! `apps/api/pi_dash/app/views/scheduler/views.py:46-157`, routes
//! `apps/api/pi_dash/app/urls/scheduler.py:18-27`
//! (`workspaces/<slug>/schedulers/`, `.../schedulers/<uuid>/`).
//!
//! Wiring only, no new logic: gates from [`super::gate`] (PIDASHCONV-632),
//! statements as SeaORM entities/queries (orm-migration), shapes from
//! `pidash_services::app_scheduler::shape` (PIDASHCONV-629).
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
//! * Duplicate slug answers 400 `{"slug": ["This slug is already in use
//!   in this workspace."]}` (R7 `validate_slug`, OSS `main` `3db73752`,
//!   PDASHOSS01-221): the EXISTS check runs whenever the slug's
//!   `CharField` half passed, so the conflict merges ahead of any other
//!   field errors (slug is declared first — F36-10). The `IntegrityError`
//!   → `{"error": "The payload is not valid"}` branch survives only as a
//!   check-then-write race backstop (`BaseAPIView.handle_exception`,
//!   `app/views/base.py:211-240`, still flattens Django-side races the
//!   same way).
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
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use sqlx::PgPool;

use pidash_db::entity::{scheduler_bindings, schedulers, workspaces};
use pidash_db::orm_tx;
use pidash_services::app_scheduler::shape;
use sea_orm::sea_query::{Expr, Func, JoinType};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait, Set,
};

use super::gate::{self, Gate};
use crate::middleware::SessionHandle;
use crate::state::AppState;
use crate::uuid_path::{classify_uuid_segment, UuidSegment};
use pidash_compat::response::json_response;

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// `get_object_or_404(Scheduler, ...)` miss (404): DRF renders the
/// `Http404("No Scheduler matches the given query.")` args through
/// `exception_handler` (`{'detail': ...}`). Byte-pinned against F36-10 in
/// [`tests::not_found_body_matches_f36_10`].
pub const SCHEDULER_NOT_FOUND_BODY: &str = r#"{"detail":"No Scheduler matches the given query."}"#;
/// `handle_exception`'s `IntegrityError` branch (400,
/// `app/views/base.py:220-224`): since R7 (`validate_slug`) this is only
/// the check-then-write race backstop on POST and PATCH, never the
/// first-line duplicate answer (and never a 409).
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
// Rows (`FromQueryResult` maps by column name; wide selects exceed the
// 16-tuple `FromRow` impls — the `license` handler precedent)
// ---------------------------------------------------------------------------

/// One `schedulers` row in fixture SELECT order (R1/R2/R5b without the
/// annotation leg). Also the in-memory image for create/PATCH renders.
/// Also the v1 surface's row image (struct-literal construction).
#[derive(Debug, Clone, PartialEq)]
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
/// columns decoded by name plus `_active_binding_count`
/// (`COUNT(CASE WHEN ...)`, never NULL).
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct AnnotatedSchedulerRow {
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
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
            created_at: self.created_at.with_timezone(&Utc),
            updated_at: self.updated_at.with_timezone(&Utc),
            created_by_id: self.created_by_id,
            updated_by_id: self.updated_by_id,
            deleted_at: self.deleted_at.map(|stamp| stamp.with_timezone(&Utc)),
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

/// The fourteen `schedulers` columns every read projects, in the old
/// SQL's SELECT order (decode is by name, so order is cosmetic).
fn scheduler_columns() -> [schedulers::Column; 14] {
    use schedulers::Column;
    [
        Column::CreatedAt,
        Column::UpdatedAt,
        Column::CreatedById,
        Column::UpdatedById,
        Column::DeletedAt,
        Column::Id,
        Column::WorkspaceId,
        Column::Slug,
        Column::Name,
        Column::Description,
        Column::Prompt,
        Column::Source,
        Column::IsEnabled,
        Column::Color,
    ]
}

// ---------------------------------------------------------------------------
// Statements (SeaORM fetchers; shapes pinned in tests)
// ---------------------------------------------------------------------------

/// The annotated scheduler statement base (R1/R2): the full scheduler
/// projection plus `_active_binding_count` over the bindings left join,
/// grouped by scheduler id (Postgres allows functionally dependent
/// columns on a single-PK `GROUP BY`). One statement preserves the old
/// statement count.
fn annotated_scheduler_statement() -> sea_orm::Select<schedulers::Entity> {
    schedulers::Entity::find()
        .select_only()
        .columns(scheduler_columns())
        .expr_as(
            Func::count(Expr::case(
                Expr::col((
                    scheduler_bindings::Entity,
                    scheduler_bindings::Column::DeletedAt,
                ))
                .is_null(),
                Expr::col((scheduler_bindings::Entity, scheduler_bindings::Column::Id)),
            )),
            "_active_binding_count",
        )
        .join(
            JoinType::LeftJoin,
            schedulers::Relation::SchedulerBindings.def(),
        )
        .group_by(schedulers::Column::Id)
}

/// R1 scheduler list: live schedulers of one workspace ordered by name,
/// each with its annotated count.
fn scheduler_list_statement(workspace_id: uuid::Uuid) -> sea_orm::Select<schedulers::Entity> {
    annotated_scheduler_statement()
        .filter(schedulers::Column::DeletedAt.is_null())
        .filter(schedulers::Column::WorkspaceId.eq(workspace_id))
        .order_by_asc(schedulers::Column::Name)
}

/// R2 scheduler detail: one live scheduler by id in the named
/// workspace, with its annotated count. The `INNER JOIN` on
/// `workspaces` has no `deleted_at` predicate — exactly the old SQL.
fn scheduler_detail_statement(
    scheduler_id: uuid::Uuid,
    slug: &str,
) -> sea_orm::Select<schedulers::Entity> {
    annotated_scheduler_statement()
        .join(JoinType::InnerJoin, schedulers::Relation::Workspaces.def())
        .filter(schedulers::Column::DeletedAt.is_null())
        .filter(schedulers::Column::Id.eq(scheduler_id))
        .filter(workspaces::Column::Slug.eq(slug))
}

/// R5b lookup: the un-annotated row; only existence matters (404
/// before the transaction opens). The old SQL's `ORDER BY created_at
/// DESC` is kept (unobservable for a unique-id lookup, but the
/// statement stays the same shape).
fn scheduler_delete_lookup_statement(
    scheduler_id: uuid::Uuid,
    slug: &str,
) -> sea_orm::Select<schedulers::Entity> {
    schedulers::Entity::find()
        .select_only()
        .columns(scheduler_columns())
        .join(JoinType::InnerJoin, schedulers::Relation::Workspaces.def())
        .filter(schedulers::Column::DeletedAt.is_null())
        .filter(schedulers::Column::Id.eq(scheduler_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .order_by_desc(schedulers::Column::CreatedAt)
}

/// R7 slug-unique check: one live row with this slug in the workspace,
/// optionally excluding the row being patched (the PATCH self-row
/// exclusion). Tombstoned slugs stay reusable (`deleted_at IS NULL`
/// only).
fn scheduler_slug_statement(
    slug: &str,
    workspace_id: uuid::Uuid,
    exclude_scheduler: Option<uuid::Uuid>,
) -> sea_orm::Select<schedulers::Entity> {
    let statement = schedulers::Entity::find()
        .select_only()
        .column(schedulers::Column::Id)
        .filter(schedulers::Column::DeletedAt.is_null())
        .filter(schedulers::Column::Slug.eq(slug))
        .filter(schedulers::Column::WorkspaceId.eq(workspace_id));
    let statement = match exclude_scheduler {
        Some(scheduler_id) => statement.filter(schedulers::Column::Id.ne(scheduler_id)),
        None => statement,
    };
    statement.limit(1)
}

/// R4 scheduler PATCH: full-column save (every writable column plus
/// the `updated_at`/`updated_by_id` refresh).
fn scheduler_patch_statement(
    now: sea_orm::prelude::DateTimeWithTimeZone,
    user_id: uuid::Uuid,
    write: &SchedulerWrite,
    scheduler_id: uuid::Uuid,
) -> sea_orm::UpdateMany<schedulers::Entity> {
    schedulers::Entity::update_many()
        .col_expr(schedulers::Column::UpdatedAt, Expr::value(now))
        .col_expr(schedulers::Column::UpdatedById, Expr::value(user_id))
        .col_expr(schedulers::Column::Slug, Expr::value(write.slug.clone()))
        .col_expr(schedulers::Column::Name, Expr::value(write.name.clone()))
        .col_expr(
            schedulers::Column::Description,
            Expr::value(write.description.clone()),
        )
        .col_expr(
            schedulers::Column::Prompt,
            Expr::value(write.prompt.clone()),
        )
        .col_expr(schedulers::Column::Color, Expr::value(write.color.clone()))
        .col_expr(schedulers::Column::IsEnabled, Expr::value(write.is_enabled))
        .filter(schedulers::Column::Id.eq(scheduler_id))
}

/// R5a delete cascade, bindings first: `deleted_at` only — `update()`
/// bypasses `auto_now`, so `updated_at` stays untouched (ported quirk
/// 4).
fn scheduler_bindings_soft_delete_statement(
    now: sea_orm::prelude::DateTimeWithTimeZone,
    scheduler_id: uuid::Uuid,
) -> sea_orm::UpdateMany<scheduler_bindings::Entity> {
    scheduler_bindings::Entity::update_many()
        .col_expr(scheduler_bindings::Column::DeletedAt, Expr::value(now))
        .filter(scheduler_bindings::Column::SchedulerId.eq(scheduler_id))
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
}

/// R5b delete cascade, scheduler second: two separate `now()` samples
/// (ported quirk 5).
fn schedulers_soft_delete_statement(
    now: sea_orm::prelude::DateTimeWithTimeZone,
    now2: sea_orm::prelude::DateTimeWithTimeZone,
    user_id: uuid::Uuid,
    scheduler_id: uuid::Uuid,
) -> sea_orm::UpdateMany<schedulers::Entity> {
    schedulers::Entity::update_many()
        .col_expr(schedulers::Column::DeletedAt, Expr::value(now))
        .col_expr(schedulers::Column::UpdatedAt, Expr::value(now2))
        .col_expr(schedulers::Column::UpdatedById, Expr::value(user_id))
        .filter(schedulers::Column::Id.eq(scheduler_id))
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
    if raw.is_empty() || raw.trim_matches(super::is_py_strip_ws).is_empty() {
        if !allow_blank {
            return Err(vec![BLANK_MESSAGE.to_owned()]);
        }
        return Ok(String::new());
    }
    let stripped = raw.trim_matches(super::is_py_strip_ws).to_owned();
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
// Slug uniqueness (R7 `validate_slug`, `scheduler.py:113-134`)
// ---------------------------------------------------------------------------

/// `validate_slug`'s input: the body's slug iff present and
/// `CharField`-valid. DRF calls the `validate_<field>` hook only when
/// the field's own validation passed, so missing, blank, over-long, or
/// wrong-typed slugs — and non-dict bodies — never reach the EXISTS
/// check (the DB constraint stays the backstop there, as in Django).
fn candidate_slug(body: &Value) -> Option<String> {
    let slug = body.as_object()?.get("slug")?;
    char_field(slug, false, Some(64)).ok()
}

/// R7 slug-unique check, with or without the PATCH self-row exclusion.
async fn slug_in_use(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    slug: &str,
    exclude_scheduler: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let found: Option<uuid::Uuid> =
        scheduler_slug_statement(slug, *workspace_id, exclude_scheduler.copied())
            .into_tuple::<uuid::Uuid>()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(found.is_some())
}

/// The `{"slug": [...]}` conflict body, merged ahead of any other field
/// errors (slug is the first declared field, so DRF lists it first).
fn slug_conflict_body(prior: Map<String, Value>) -> Value {
    let mut body = Map::with_capacity(prior.len() + 1);
    body.insert(
        "slug".to_owned(),
        Value::Array(vec![Value::String(shape::SLUG_UNIQUE_ERROR.to_owned())]),
    );
    body.extend(prior);
    Value::Object(body)
}

/// Run R7 after pure validation. On `Ok` a conflict denies with the
/// lone slug body; on `Err` a conflict prepends `slug` to the field
/// errors. The check runs only when the slug's `CharField` half passed
/// (that's when DRF calls `validate_slug`), so missing/invalid slugs
/// and non-dict bodies pass through untouched.
async fn enforce_slug_unique(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    body: &Value,
    exclude_scheduler: Option<&uuid::Uuid>,
    validated: Result<SchedulerWrite, Value>,
) -> Result<SchedulerWrite, Denial> {
    match validated {
        Ok(write) => {
            if slug_in_use(pool, workspace_id, &write.slug, exclude_scheduler).await? {
                return Err(Denial::BadJson(slug_conflict_body(Map::new())));
            }
            Ok(write)
        }
        Err(Value::Object(prior)) => {
            if let Some(slug) = candidate_slug(body) {
                if slug_in_use(pool, workspace_id, &slug, exclude_scheduler).await? {
                    return Err(Denial::BadJson(slug_conflict_body(prior)));
                }
            }
            Err(Denial::BadJson(Value::Object(prior)))
        }
        Err(other) => Err(Denial::BadJson(other)),
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows: Vec<AnnotatedSchedulerRow> = match scheduler_list_statement(gate.workspace_id)
        .into_model::<AnnotatedSchedulerRow>()
        .all(&db)
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
    let (row, count) = match create_scheduler(pool, &gate.workspace_id, &gate.user_id, &body).await
    {
        Ok(created) => created,
        Err(denial) => return denial.into_response(),
    };
    (
        StatusCode::CREATED,
        Json(render_scheduler_row(&row, count, &gate.timezone)),
    )
        .into_response()
}

/// The create write shared by the web app and the v1 surface
/// (`scheduler/definition_writes.py::create_definition`, PDASHOSS01-318):
/// serializer validation, the R7 slug check, the R3 INSERT, then the R6
/// fallback count the 201 renders. Returns the fresh row and that count.
pub(crate) async fn create_scheduler(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    body: &Value,
) -> Result<(SchedulerRow, i64), Denial> {
    let validated = validate_create_body(body);
    let write = enforce_slug_unique(pool, workspace_id, body, None, validated).await?;
    // R3 (`:79-84`): auto id/timestamps, `created_by` the request user
    // (crum), `updated_by`/`deleted_at` NULL, `source` the `'builtin'`
    // literal. One `:now` sample feeds both timestamps (queries-layer
    // note: Django samples per field, unobservable at ~1us).
    let id = uuid::Uuid::new_v4();
    let now = Utc::now();
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = now.into();
    let db = pidash_db::pool::sea_orm_connection(pool);
    schedulers::ActiveModel {
        id: Set(id),
        created_at: Set(stamped),
        updated_at: Set(stamped),
        created_by_id: Set(Some(*user_id)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        workspace_id: Set(*workspace_id),
        slug: Set(write.slug.clone()),
        name: Set(write.name.clone()),
        description: Set(write.description.clone()),
        prompt: Set(write.prompt.clone()),
        source: Set("builtin".to_owned()),
        is_enabled: Set(write.is_enabled),
        color: Set(write.color.clone()),
    }
    .insert(&db)
    .await
    .map_err(|error| {
        if orm_tx::is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    // The create response serializes the fresh un-annotated row, so the
    // count falls back to the R6 query (always 0 for a new scheduler).
    let fallback: u64 = scheduler_bindings::Entity::find()
        .filter(scheduler_bindings::Column::SchedulerId.eq(id))
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
        .count(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let count = shape::resolve_active_binding_count(None, fallback as i64);
    let row = SchedulerRow {
        created_at: now,
        updated_at: now,
        created_by_id: Some(*user_id),
        updated_by_id: None,
        deleted_at: None,
        id,
        workspace_id: *workspace_id,
        slug: write.slug,
        name: write.name,
        description: write.description,
        prompt: write.prompt,
        source: "builtin".to_owned(),
        is_enabled: write.is_enabled,
        color: write.color,
    };
    Ok((row, count))
}

/// `GET /api/workspaces/<slug>/schedulers/<uuid>/` (`views.py:95-111`):
/// workspace admins read one scheduler with its annotated count.
pub async fn scheduler_detail(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let scheduler_id = match classify_uuid_segment(&scheduler_raw) {
        UuidSegment::Canonical(id) => id,
        // Django's `<uuid:>` converter routes only the canonical
        // spelling; non-canonical and garbage proxy to its resolver
        // 404 (PIDASHCONV-1475).
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return crate::edge::proxy(State(state), req).await;
        }
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<AnnotatedSchedulerRow> = match scheduler_detail_statement(scheduler_id, &slug)
        .into_model::<AnnotatedSchedulerRow>()
        .one(&db)
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
    let scheduler_id = match classify_uuid_segment(&scheduler_raw) {
        UuidSegment::Canonical(id) => id,
        // Django's `<uuid:>` converter routes only the canonical
        // spelling; non-canonical and garbage proxy to its resolver
        // 404 (PIDASHCONV-1475).
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return crate::edge::proxy(State(state), req).await;
        }
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<AnnotatedSchedulerRow> = match scheduler_detail_statement(scheduler_id, &slug)
        .into_model::<AnnotatedSchedulerRow>()
        .one(&db)
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
    let validated = validate_patch_body(&body, &record);
    let write = match enforce_slug_unique(
        pool,
        &gate.workspace_id,
        &body,
        Some(&scheduler_id),
        validated,
    )
    .await
    {
        Ok(write) => write,
        Err(denial) => return denial.into_response(),
    };
    // R4 (`:127-133`): `save()` rewrites every writable column with the
    // merged values and refreshes `updated_at`/`updated_by_id`.
    let now = Utc::now();
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = now.into();
    let update = scheduler_patch_statement(stamped, gate.user_id, &write, scheduler_id)
        .exec(&db)
        .await;
    if let Err(error) = update {
        if orm_tx::is_integrity_violation(&error) {
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

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77-78`): three positional args, `using` stays a null
/// kwarg — exactly as Python's `.delay(label, name, pk, using=using)`
/// call sends it with `using=None`. Fires on every instance `.delete()`
/// in these views (`Scheduler` lives in the `db` app,
/// `db/models/scheduler.py:107`).
pub(crate) fn soft_delete_message(model: &str, pk: &uuid::Uuid) -> pidash_jobs::TaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::TaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    )
}

/// Best-effort publish of [`soft_delete_message`]: without the queue
/// the response still stands.
pub(crate) async fn enqueue_soft_delete(pool: &PgPool, model: &str, pk: &uuid::Uuid) {
    let message = soft_delete_message(model, pk);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// The delete write shared by the web app and the v1 surface
/// (`scheduler/definition_writes.py::delete_definition`, PDASHOSS01-318),
/// inside the caller's transaction: two separate `now()` samples (ported
/// quirk 5), bindings first so no active binding survives, then the
/// scheduler's own soft delete. The caller commits, then enqueues the
/// related-objects sweep ([`enqueue_soft_delete`]).
pub(crate) async fn soft_delete_scheduler(
    conn: &mut sqlx::PgConnection,
    scheduler_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let now = Utc::now();
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = now.into();
    let tx_conn = orm_tx::TxConn::new(conn);
    scheduler_bindings_soft_delete_statement(stamped, *scheduler_id)
        .exec(&tx_conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    let now2 = Utc::now();
    let stamped2: sea_orm::prelude::DateTimeWithTimeZone = now2.into();
    schedulers_soft_delete_statement(stamped, stamped2, *user_id, *scheduler_id)
        .exec(&tx_conn)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `DELETE /api/workspaces/<slug>/schedulers/<uuid>/`
/// (`views.py:136-157`): one transaction soft-deletes the still-active
/// bindings inline and then the scheduler; the instance `delete()` then
/// enqueues `soft_delete_related_objects("db", "scheduler", pk,
/// using=None)` (`db/mixins.py:77-78`); 204 with an empty body.
pub async fn scheduler_delete(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let scheduler_id = match classify_uuid_segment(&scheduler_raw) {
        UuidSegment::Canonical(id) => id,
        // Django's `<uuid:>` converter routes only the canonical
        // spelling; non-canonical and garbage proxy to its resolver
        // 404 (PIDASHCONV-1475).
        UuidSegment::NonCanonical | UuidSegment::Garbage => {
            return crate::edge::proxy(State(state), req).await;
        }
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<schedulers::Model> =
        match scheduler_delete_lookup_statement(scheduler_id, &slug)
            .into_model::<schedulers::Model>()
            .one(&db)
            .await
        {
            Ok(row) => row,
            Err(_) => return Denial::ServerError.into_response(),
        };
    if row.is_none() {
        return Denial::ObjectNotFound.into_response();
    }
    // `transaction.atomic()` (`:151-156`).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if let Err(denial) = soft_delete_scheduler(&mut tx, &scheduler_id, &gate.user_id).await {
        return denial.into_response();
    }
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    // Instance `delete()` → `.delay(...)` (`db/mixins.py:77-78`): stamp,
    // then enqueue — same order as the mixin.
    enqueue_soft_delete(pool, "scheduler", &scheduler_id).await;
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
            "HEAD" => router.head(crate::edge::proxy),
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
                &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/schedulers/{scheduler_id}/",
            owned(
                get(scheduler_detail)
                    .patch(scheduler_patch)
                    .delete(scheduler_delete),
                &["POST", "PUT", "HEAD", "OPTIONS"],
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

    // -- Statement shapes (SeaORM, orm-migration.md §3.2/§3.3) ---------------

    fn scheduler_id() -> uuid::Uuid {
        "0d065023-aa70-4d7d-85da-029d7169843e"
            .parse()
            .expect("uuid")
    }

    fn workspace_id() -> uuid::Uuid {
        "6f819bce-fba0-4ef5-9b16-187eea21a6a1"
            .parse()
            .expect("uuid")
    }

    /// The R1 list keeps the old join/filter/order shape: full
    /// projection, the bindings left join, live rows only, name order,
    /// and the unique `_active_binding_count` alias.
    #[test]
    fn list_statement_pins_join_filters_and_order() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = scheduler_list_statement(workspace_id())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FROM"), "list from: {sql}");
        assert!(sql.contains("LEFT JOIN"), "list left join: {sql}");
        assert!(
            sql.contains("\"scheduler_bindings\""),
            "list bindings join: {sql}"
        );
        assert!(
            sql.contains("\"deleted_at\" IS NULL"),
            "list live rows: {sql}"
        );
        assert!(
            sql.contains("\"_active_binding_count\""),
            "list count alias: {sql}"
        );
        assert!(sql.contains("CASE"), "list filtered count: {sql}");
        assert!(sql.contains("GROUP BY"), "list group: {sql}");
        assert!(sql.contains("ORDER BY"), "list order: {sql}");
        assert!(sql.contains("\"name\""), "list orders by name: {sql}");
        assert!(
            sql.contains("\"prompt\""),
            "list keeps the wide projection: {sql}"
        );
    }

    /// The R2 detail keeps the old shape: bindings left join plus the
    /// workspace inner join (no `workspaces.deleted_at` predicate),
    /// scoped by id + slug, no ordering.
    #[test]
    fn detail_statement_pins_joins_and_scope() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = scheduler_detail_statement(scheduler_id(), "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("LEFT JOIN"), "detail left join: {sql}");
        assert!(sql.contains("INNER JOIN"), "detail inner join: {sql}");
        assert!(
            sql.contains("\"workspaces\""),
            "detail workspace join: {sql}"
        );
        assert!(
            sql.contains("\"_active_binding_count\""),
            "detail count alias: {sql}"
        );
        assert!(sql.contains("GROUP BY"), "detail group: {sql}");
        assert!(sql.contains("\"slug\""), "detail slug scope: {sql}");
        assert!(!sql.contains("ORDER BY"), "detail keeps no ordering: {sql}");
    }

    /// The R5b lookup keeps the old shape: the workspace inner join,
    /// id + slug scope, `created_at DESC` order, no count annotation.
    #[test]
    fn delete_lookup_statement_pins_scope_and_order() {
        use sea_orm::{DbBackend, QueryTrait};
        let sql = scheduler_delete_lookup_statement(scheduler_id(), "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("INNER JOIN"), "lookup inner join: {sql}");
        assert!(
            sql.contains("\"workspaces\""),
            "lookup workspace join: {sql}"
        );
        assert!(
            sql.contains("\"deleted_at\" IS NULL"),
            "lookup live rows: {sql}"
        );
        assert!(sql.contains("ORDER BY"), "lookup order: {sql}");
        assert!(
            sql.contains("\"created_at\""),
            "lookup orders by created_at: {sql}"
        );
        assert!(
            !sql.contains("_active_binding_count"),
            "lookup stays un-annotated: {sql}"
        );
    }

    /// The R7 slug check keeps the old shape: live rows only, slug +
    /// workspace scope, `LIMIT 1`, and the PATCH self-row exclusion
    /// only on PATCH.
    #[test]
    fn slug_statement_pins_scope_limit_and_exclusion() {
        use sea_orm::{DbBackend, QueryTrait};
        let create_sql = scheduler_slug_statement("nightly", workspace_id(), None)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            create_sql.contains("\"deleted_at\" IS NULL"),
            "slug live rows: {create_sql}"
        );
        assert!(
            create_sql.contains("\"slug\""),
            "slug slug scope: {create_sql}"
        );
        assert!(create_sql.contains("LIMIT 1"), "slug limit: {create_sql}");
        assert!(
            !create_sql.contains("<>") && !create_sql.contains("NOT"),
            "create keeps no exclusion: {create_sql}"
        );

        let patch_sql = scheduler_slug_statement("nightly", workspace_id(), Some(scheduler_id()))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            patch_sql.contains("<>"),
            "patch excludes the self row: {patch_sql}"
        );
        assert!(patch_sql.contains("LIMIT 1"), "patch limit: {patch_sql}");
    }

    /// The R4 PATCH statement rewrites every writable column plus the
    /// audit refresh, scoped by id.
    #[test]
    fn patch_statement_pins_full_column_save() {
        use sea_orm::{DbBackend, QueryTrait};
        let write = SchedulerWrite {
            slug: "nightly".to_owned(),
            name: "Nightly".to_owned(),
            description: "d".to_owned(),
            prompt: "Scan.".to_owned(),
            color: "#10B981".to_owned(),
            is_enabled: true,
        };
        let now: sea_orm::prelude::DateTimeWithTimeZone = Utc::now().into();
        let sql = scheduler_patch_statement(now, workspace_id(), &write, scheduler_id())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("UPDATE \"schedulers\""), "patch update: {sql}");
        for column in [
            "\"updated_at\"",
            "\"updated_by_id\"",
            "\"slug\"",
            "\"name\"",
            "\"description\"",
            "\"prompt\"",
            "\"color\"",
            "\"is_enabled\"",
        ] {
            assert!(sql.contains(column), "patch writes {column}: {sql}");
        }
        assert!(sql.contains("WHERE"), "patch scope: {sql}");
    }

    /// The R5a/R5b cascade keeps the old shape: bindings first
    /// (`deleted_at` only — `updated_at` untouched, ported quirk 4),
    /// then the scheduler with both `now()` samples and the actor.
    #[test]
    fn soft_delete_statements_pin_cascade_shape() {
        use sea_orm::{DbBackend, QueryTrait};
        let now: sea_orm::prelude::DateTimeWithTimeZone = Utc::now().into();
        let now2: sea_orm::prelude::DateTimeWithTimeZone = Utc::now().into();
        let bindings = scheduler_bindings_soft_delete_statement(now, scheduler_id())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            bindings.contains("UPDATE \"scheduler_bindings\""),
            "bindings update: {bindings}"
        );
        assert!(
            bindings.contains("\"deleted_at\""),
            "bindings sets deleted_at: {bindings}"
        );
        assert!(
            !bindings.contains("\"updated_at\""),
            "bindings leaves updated_at alone: {bindings}"
        );
        assert!(
            bindings.contains("\"scheduler_id\""),
            "bindings scope: {bindings}"
        );

        let scheduler = schedulers_soft_delete_statement(now, now2, workspace_id(), scheduler_id())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            scheduler.contains("UPDATE \"schedulers\""),
            "scheduler update: {scheduler}"
        );
        assert!(
            scheduler.contains("\"deleted_at\""),
            "scheduler sets deleted_at: {scheduler}"
        );
        assert!(
            scheduler.contains("\"updated_at\""),
            "scheduler refreshes updated_at: {scheduler}"
        );
        assert!(
            scheduler.contains("\"updated_by_id\""),
            "scheduler sets the actor: {scheduler}"
        );
        assert!(scheduler.contains("WHERE"), "scheduler scope: {scheduler}");
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

    // -- R7 slug uniqueness ---------------------------------------------------

    /// The lone conflict body is wire-exact (captured from `origin/main`)
    /// and equals the mandated [`shape::field_error_body`] output.
    #[test]
    fn slug_conflict_body_is_wire_exact() {
        let rendered =
            serde_json::to_string(&slug_conflict_body(Map::new())).expect("conflict JSON");
        assert_eq!(
            rendered,
            r#"{"slug":["This slug is already in use in this workspace."]}"#
        );
        assert_eq!(
            rendered,
            shape::field_error_body("slug", shape::SLUG_UNIQUE_ERROR)
        );
    }

    /// A conflict merges ahead of other field errors: slug is declared
    /// first, so DRF lists it first (`{"slug": [...], "name": [...]}`).
    #[test]
    fn slug_conflict_merges_ahead_of_other_errors() {
        let prior = errors_for(json!({"slug": "dup", "prompt": "x"}));
        assert_eq!(
            prior.keys().collect::<Vec<_>>(),
            vec!["name"],
            "only name fails; slug reached R7"
        );
        let merged = slug_conflict_body(prior);
        let keys: Vec<&String> = merged.as_object().expect("object").keys().collect();
        assert_eq!(keys, vec!["slug", "name"]);
        assert_eq!(
            serde_json::to_string(&merged).expect("merged JSON"),
            r#"{"slug":["This slug is already in use in this workspace."],"name":["This field is required."]}"#
        );
    }

    /// Every F36-10 `duplicate_slug` body (create + PATCH) equals the
    /// conflict body, byte for byte.
    #[test]
    fn duplicate_slug_bodies_match_f36_10() {
        let fixture = fixture_json(FIXTURE_SCHEDULER_IO);
        let actions = fixture["actions"].as_array().expect("actions array");
        let expected = slug_conflict_body(Map::new());
        let mut checked = 0;
        for action in actions {
            if let Some(duplicate) = action.get("duplicate_slug") {
                assert_eq!(duplicate["body"], expected);
                assert_eq!(duplicate["status"], 400);
                checked += 1;
            }
        }
        assert_eq!(checked, 2, "create + PATCH duplicate bodies");
    }

    /// `validate_slug`'s input set: present and `CharField`-valid slugs
    /// only (stripped, coerced); everything else skips the check.
    #[test]
    fn candidate_slug_matches_validate_slug_inputs() {
        assert_eq!(
            candidate_slug(&json!({"slug": "  dup  "})),
            Some("dup".to_owned())
        );
        assert_eq!(candidate_slug(&json!({"slug": 5})), Some("5".to_owned()));
        assert_eq!(candidate_slug(&json!({"name": "x"})), None);
        assert_eq!(candidate_slug(&json!({"slug": "   "})), None);
        assert_eq!(candidate_slug(&json!({"slug": Value::Null})), None);
        assert_eq!(candidate_slug(&json!({"slug": ["x"]})), None);
        assert_eq!(candidate_slug(&json!("dup")), None);
        assert_eq!(candidate_slug(&json!(null)), None);
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

    #[test]
    fn soft_delete_message_matches_the_mixin_call() {
        // `db/mixins.py:77-78`: `.delay(label, name, pk, using=using)`
        // with `using=None` — three positional args, `using` stays a
        // null kwarg (never a positional `"default"`).
        let pk = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        let message = soft_delete_message("scheduler", &pk);
        assert_eq!(
            message.task,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
        assert_eq!(
            message.args,
            vec![
                Value::String("db".to_owned()),
                Value::String("scheduler".to_owned()),
                Value::String(pk.to_string()),
            ]
        );
        assert_eq!(message.kwargs.len(), 1);
        assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
    }

    #[test]
    fn char_field_strips_python_whitespace() {
        // DRF `CharField` strips with Python `str.strip()`, which also removes
        // U+001C-U+001F (FS/GS/RS/US); Rust `char::is_whitespace` does not
        // (PIDASHCONV-1203). Both the blank check and the stripped value must
        // use the ported predicate so create/PATCH match Django.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = Value::String(format!("{sep}dup{sep}"));
            assert_eq!(
                char_field(&padded, false, Some(64)).unwrap(),
                "dup",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // A lone control character strips to empty: DRF blank error.
        assert_eq!(
            char_field(&Value::String("\u{1f}".to_owned()), false, Some(255)),
            Err(vec![BLANK_MESSAGE.to_owned()])
        );
    }
}
