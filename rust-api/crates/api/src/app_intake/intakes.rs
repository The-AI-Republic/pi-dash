//! Intake CRUD handlers (D-32, stage 5).
//!
//! Ports the five handler units of `app/views/intake/base.py:56-91`
//! (`IntakeViewSet.list` / `perform_create` via the `create` flow / the
//! DRF-default `retrieve` / `partial_update` / `destroy`) with routes
//! from `app/urls/intake.py:16-25,36-45`:
//!
//! * `GET intakes/` + `inboxes/` (`list`)
//! * `POST intakes/` + `inboxes/` (`create` → `perform_create`)
//! * `GET intakes/<pk>/` + `inboxes/<pk>/` (`retrieve`, inherited default)
//! * `PATCH intakes/<pk>/` + `inboxes/<pk>/` (`partial_update`,
//!   inherited default)
//! * `DELETE intakes/<pk>/` + `inboxes/<pk>/` (`destroy`)
//!
//! Only the methods above are owned: `PUT`/`PATCH`/`DELETE` on the
//! collection and `POST`/`PUT` on the detail proxy to Django (its
//! 405-after-auth and metadata responses live there), as do `OPTIONS`
//! and `HEAD`-adjacent handling per [`crate::edge::owned`].
//!
//! Layering: permission gates in
//! `pidash_services::app_intake::permissions`, serializer key order in
//! `::shape` ([`INTAKE_KEY_ORDER`]), the annotation SQL shape in
//! `pidash_db::app_intake::queries`. This module owns the HTTP shell
//! (routes, session auth, tenant + membership resolution), the SQL text
//! for the handler-owned lookups/writes, the DRF field-validation
//! mirrors, and the row rendering.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-perform-create (`base.py:77-79`): `@allow_permission` decorates
//!   `perform_create(serializer)`, but DRF invokes it with the serializer
//!   in the `request` slot, so the wrapper dies on `request.user`
//!   (`AttributeError` → `handle_exception` 500) for every role, admins
//!   included. Validation (400s) runs first, so invalid payloads still
//!   answer 400; every valid payload answers 500 and writes nothing.
//!   [`guards::intake_perform_create_outcome`] pins the 500.
//! * BUG-destroy-missing (`base.py:83`): `DELETE intakes/<unknown-pk>/`
//!   reads `.first()` (`None`) then dereferences `.is_default`
//!   unconditionally → `AttributeError` → 500, never 404.
//!   [`guards::intake_destroy_gate`] pins the 500 via
//!   [`guards::IntakeLookup::Missing`].
//! * BUG-intake-detail-undecorated (`base.py:56-91`): the DRF-default
//!   `retrieve` / `partial_update` carry no decorator, so any
//!   authenticated user passes with no project-membership check.
//!   [`guards::intake_detail_undecorated`] pins the pass-through.
//! * QUIRK-list-null (`base.py:73-75`): with no intake row the view
//!   serves `IntakeSerializer(None).data` — a null body with 200, not a
//!   404. Ported as written.
//! * QUIRK-unique-required (`intake.py:17-24` + `intake.py:34`): DRF's
//!   `get_uniqueness_extra_kwargs` (`serializers.py:1471-1505`) forces
//!   `required=True` on every member of the `unique_together` /
//!   `UniqueConstraint` sets present on the serializer — so `name` and
//!   `deleted_at` are required on create even though the model fields
//!   carry `blank=True` — while the `UniqueTogetherValidator` itself is
//!   skipped (`serializers.py:1596`: the read-only `project` maps to no
//!   writable source).
//! * QUIRK-no-unique-check (`intake.py:34`): with the validator skipped,
//!   no uniqueness check runs on intake writes. Duplicate names pass
//!   validation: on create they die in BUG-perform-create (500 like
//!   every valid payload); on update they reach the database, where the
//!   partial unique constraint fires and `handle_exception` answers the
//!   `IntegrityError` branch (`{"error": "The payload is not valid"}`).
//!   Verified live against Django (probes `dup_same_project`,
//!   `patch_dup_name`).

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QuerySelect,
    RelationTrait,
};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{file_assets, intakes as intake_entity, projects, users, workspaces};

use pidash_services::app_intake::permissions as guards;

use super::{
    actor, enqueue_soft_delete, guard_denial, is_canonical_uuid, load_membership, parse_body,
    pool_of, raw_json_response, Denial,
};
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use pidash_compat::datetime::render_uuid_opt as opt_uuid;
use pidash_compat::validate::{push_error, validate_char, validate_datetime};

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// Tenant project for the intake CRUD routes.
///
/// Django checks less than [`super::resolve_tenant`] on these paths:
/// `_rewrite_project_kwarg` (`app/views/base.py:49-82`) resolves
/// identifier-form ids through `Project.resolve` (miss →
/// `Http404("Project not found")`, `db/models/project.py:214-218`)
/// and passes UUIDs through untouched — no existence check. The
/// membership gate (gated routes) or the row lookup (undecorated
/// routes) decides the rest, so a missing UUID answers 403 on gated
/// routes and `No Intake matches…` on undecorated ones, never 404.
pub async fn intake_tenant(pool: &PgPool, slug: &str, project_raw: &str) -> Result<Uuid, Denial> {
    if let Ok(id) = project_raw.parse::<Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(project_raw);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::Identifier.eq(upper))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::ProjectNotFound)
}

/// Parse a UUID path segment: Django's `<uuid:>` converter leaves the
/// route unmatched, so every non-canonical spelling — uppercase, simple
/// (32 hex), braced, `urn:uuid:` and garbage — answers the `handler404`
/// body before auth (`custom_404_view`, prod). `Uuid::parse_str` is
/// permissive, so gate on the canonical spelling first (PIDASHCONV-1132
/// / 1193).
#[allow(clippy::result_large_err)]
pub fn parse_pk(raw: &str) -> Result<Uuid, Response> {
    match raw.parse::<Uuid>() {
        Ok(id) if is_canonical_uuid(raw) => Ok(id),
        _ => Err(Denial::PageNotFound.into_response()),
    }
}

/// Serializer field errors: DRF's `{field: [messages]}` mapping plus the
/// `non_field_errors` list. `serde_json::Map` iterates in key order, so
/// multi-field bodies are deterministic.
pub type FieldErrors = serde_json::Map<String, serde_json::Value>;

/// DRF `BooleanField.to_internal_value` (`fields.py:665-707`): the
/// `TRUE_VALUES` / `FALSE_VALUES` sets, matched case-insensitively for
/// strings; JSON numbers compare numerically, so `1.0`/`0.0` count.
fn validate_boolean(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<bool> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    match value {
        serde_json::Value::Bool(flag) => Some(*flag),
        serde_json::Value::Number(number) => {
            if is_one(number) {
                Some(true)
            } else if is_zero(number) {
                Some(false)
            } else {
                push_error(errors, field, "Must be a valid boolean.".to_owned());
                None
            }
        }
        serde_json::Value::String(text) => match text.trim().to_lowercase().as_str() {
            "true" | "t" | "y" | "yes" | "on" | "1" => Some(true),
            "false" | "f" | "n" | "no" | "off" | "0" => Some(false),
            _ => {
                push_error(errors, field, "Must be a valid boolean.".to_owned());
                None
            }
        },
        _ => {
            push_error(errors, field, "Must be a valid boolean.".to_owned());
            None
        }
    }
}

/// Numeric `1` in any JSON number shape (`1`, `1.0`).
fn is_one(number: &serde_json::Number) -> bool {
    number.as_i64() == Some(1) || number.as_u64() == Some(1) || number.as_f64() == Some(1.0)
}

/// Numeric `0` in any JSON number shape (`0`, `0.0`).
fn is_zero(number: &serde_json::Number) -> bool {
    number.as_i64() == Some(0) || number.as_u64() == Some(0) || number.as_f64() == Some(0.0)
}

/// DRF `JSONField.to_internal_value` (`view_props`, `logo_props`,
/// `fields.py:1774-1784`): only the HTML-form `is_json_string` path
/// parses; parsed-JSON input just re-serializes (always fine) and passes
/// through untouched — even plain strings. `None` is rejected upstream
/// with the null message (no `null=True` on the model fields).
fn validate_json_field(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
) -> Option<serde_json::Value> {
    if value.is_null() {
        push_error(errors, field, "This field may not be null.".to_owned());
        return None;
    }
    Some(value.clone())
}

/// DRF `PrimaryKeyRelatedField.to_internal_value` over the `User`
/// queryset (`created_by`, `updated_by`, `relations.py:252-263`,
/// `pk_field=None`): bools fail with the incorrect-type message; the
/// empty string reads as null (verified against the live serializer);
/// every other input renders through Django's UUID `to_python`
/// (`“<str(value)>” is not a valid UUID.` on failure, with Python
/// `str()` rendering); a well-formed id must name a live row. Returns
/// `Ok(None)` for a nullish input or a field error (recorded in
/// `errors`), `Err` for a database failure (the 500 envelope).
async fn validate_user_pk(
    pool: &PgPool,
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
) -> Result<Option<Uuid>, Denial> {
    if value.is_null() {
        return Ok(None);
    }
    if value.is_boolean() {
        push_error(
            errors,
            field,
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        );
        return Ok(None);
    }
    if let serde_json::Value::String(text) = value {
        // The empty string reads as null (live-serializer probe).
        if text.is_empty() {
            return Ok(None);
        }
        if let Ok(id) = text.parse::<Uuid>() {
            return check_user_row(pool, errors, field, &id.to_string(), id).await;
        }
        push_error(
            errors,
            field,
            format!("\u{201c}{text}\u{201d} is not a valid UUID."),
        );
        return Ok(None);
    }
    // Integers convert (`uuid.UUID(int=…)`); floats, arrays and objects
    // fail with the Python-`str()` rendering in the message.
    if let Some(int) = value.as_i64() {
        if int >= 0 {
            let id = Uuid::from_u128(int as u128);
            return check_user_row(pool, errors, field, &int.to_string(), id).await;
        }
    } else if let Some(uint) = value.as_u64() {
        let id = Uuid::from_u128(uint as u128);
        return check_user_row(pool, errors, field, &uint.to_string(), id).await;
    }
    let rendered = python_str(value);
    push_error(
        errors,
        field,
        format!("\u{201c}{rendered}\u{201d} is not a valid UUID."),
    );
    Ok(None)
}

/// The `does_not_exist` lookup (`relations.py:261`): a well-formed id
/// must name a live `users` row.
async fn check_user_row(
    pool: &PgPool,
    errors: &mut FieldErrors,
    field: &str,
    rendered: &str,
    id: Uuid,
) -> Result<Option<Uuid>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<Uuid> = users::Entity::find_by_id(id)
        .select_only()
        .column(users::Column::Id)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        push_error(
            errors,
            field,
            format!("Invalid pk \"{rendered}\" - object does not exist."),
        );
        return Ok(None);
    }
    Ok(Some(id))
}

/// Python `str()` rendering for JSON values, for the UUID failure
/// message: strings as-is, integers decimal, floats with the `.0`
/// suffix when integral, booleans capitalized, `None`/`null` as
/// `None`, arrays and objects with single quotes (an approximation for
/// nested escapes, which no contract input exercises).
fn python_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(number) => python_number(number),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => {
            let inner = items.iter().map(python_repr).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        serde_json::Value::Object(map) => {
            let inner = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_single(key), python_repr(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

/// Python `repr()` for nested values inside the UUID failure message:
/// strings single-quoted, everything else like [`python_str`].
fn python_repr(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => python_single(text),
        _ => python_str(value),
    }
}

fn python_number(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    match number.as_f64() {
        Some(float) if float.fract() == 0.0 && float.abs() < 1e21 => {
            format!("{}.0", float.trunc() as i64)
        }
        Some(float) => float.to_string(),
        None => number.to_string(),
    }
}

fn python_single(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `intakes` row with every column the handlers read or render.
#[derive(Debug, Clone)]
pub struct IntakeRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub view_props: serde_json::Value,
    pub logo_props: serde_json::Value,
}

/// `intakes` row decoded by name, with the pending count annotation.
#[derive(Debug, Clone, FromQueryResult)]
struct IntakeWithCount {
    id: Uuid,
    created_at: chrono::DateTime<chrono::FixedOffset>,
    updated_at: chrono::DateTime<chrono::FixedOffset>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    project_id: Uuid,
    workspace_id: Uuid,
    name: String,
    description: String,
    is_default: bool,
    view_props: serde_json::Value,
    logo_props: serde_json::Value,
    pending_issue_count: i64,
}

impl From<IntakeWithCount> for IntakeRow {
    fn from(row: IntakeWithCount) -> Self {
        Self {
            id: row.id,
            created_at: row.created_at.with_timezone(&Utc),
            updated_at: row.updated_at.with_timezone(&Utc),
            created_by_id: row.created_by_id,
            updated_by_id: row.updated_by_id,
            deleted_at: row.deleted_at.map(|dt| dt.with_timezone(&Utc)),
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            name: row.name,
            description: row.description,
            is_default: row.is_default,
            view_props: row.view_props,
            logo_props: row.logo_props,
        }
    }
}

// ---------------------------------------------------------------------------
// Fetches (same WHERE semantics as the queries layer)
// ---------------------------------------------------------------------------

/// `IntakeViewSet.get_queryset` (`base.py:60-70`) executed: the tenant
/// filter with the `pending_issue_count` annotation (`:68`,
/// `Count("issue_intake", filter=status=-2)` over the soft-delete-scoped
/// reverse manager — the `deleted_at IS NULL` sits in the `ON` clause,
/// exactly like Django's annotation join), `Meta.ordering` (`name` ASC)
/// and, for list, `.first()` (`:74` → `LIMIT 1`).
///
/// `pk` scopes to one row (`retrieve` / `partial_update` / `destroy`
/// tenant lookup); `None` takes the first row in name order (`list`).
pub async fn fetch_intake(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: Option<Uuid>,
    include_deleted: bool,
) -> Result<Option<(IntakeRow, i64)>, Denial> {
    use sea_orm::sea_query::{Alias, Expr, Func, Order};
    let db = pidash_db::pool::sea_orm_connection(pool);
    // Django `Count("issue_intake", filter=status=-2)` over the soft-delete
    // scoped reverse manager: LEFT JOIN with `deleted_at IS NULL` in the ON
    // clause, `COUNT(ii.id) FILTER (WHERE ii.status = -2)`. One sea-query
    // statement, decoded by name into `IntakeWithCount`.
    let mut stmt = sea_orm::sea_query::Query::select();
    stmt.expr(Expr::col((Alias::new("i"), Alias::new("id"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("created_at"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("updated_at"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("created_by_id"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("updated_by_id"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("deleted_at"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("project_id"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("workspace_id"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("name"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("description"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("is_default"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("view_props"))))
        .expr(Expr::col((Alias::new("i"), Alias::new("logo_props"))))
        .expr_as(
            {
                let case = sea_orm::sea_query::CaseStatement::new().case(
                    Expr::col((Alias::new("ii"), Alias::new("status"))).eq(-2),
                    Expr::col((Alias::new("ii"), Alias::new("id"))),
                );
                Func::count(sea_orm::sea_query::SimpleExpr::Case(Box::new(case)))
            },
            Alias::new("pending_issue_count"),
        )
        .from_as(Alias::new("intakes"), Alias::new("i"))
        .join(
            JoinType::InnerJoin,
            Alias::new("workspaces"),
            Expr::col((Alias::new("w"), Alias::new("id")))
                .equals((Alias::new("i"), Alias::new("workspace_id"))),
        )
        .join(
            JoinType::LeftJoin,
            Alias::new("intake_issues"),
            Expr::col((Alias::new("i"), Alias::new("id")))
                .equals((Alias::new("ii"), Alias::new("intake_id")))
                .and(Expr::col((Alias::new("ii"), Alias::new("deleted_at"))).is_null()),
        )
        .cond_where(
            sea_orm::sea_query::Condition::all()
                .add(Expr::col((Alias::new("w"), Alias::new("slug"))).eq(slug))
                .add(Expr::col((Alias::new("i"), Alias::new("project_id"))).eq(*project_id)),
        );
    if !include_deleted {
        stmt.and_where(Expr::col((Alias::new("i"), Alias::new("deleted_at"))).is_null());
    }
    if let Some(pk_value) = pk {
        stmt.and_where(Expr::col((Alias::new("i"), Alias::new("id"))).eq(pk_value));
    }
    stmt.group_by_col((Alias::new("i"), Alias::new("id")))
        .order_by((Alias::new("i"), Alias::new("name")), Order::Asc)
        .limit(1);
    let statement = sea_orm::DbBackend::Postgres.build(&stmt);
    let result = db
        .query_one(statement)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(result) = result else {
        return Ok(None);
    };
    let row = FromQueryResult::from_query_result(&result, "").map_err(|_| Denial::ServerError)?;
    let row: IntakeWithCount = row;
    let pending = row.pending_issue_count;
    Ok(Some((IntakeRow::from(row), pending)))
}

/// One `projects` row for the `ProjectLiteSerializer` nest (plain join,
/// like `select_related("project")`).
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

#[derive(Debug, Clone, FromQueryResult)]
struct ProjectProbe {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_asset_id: Option<Uuid>,
    logo_props: serde_json::Value,
    description: String,
    is_default: bool,
}

async fn fetch_project(pool: &PgPool, project_id: &Uuid) -> Result<Option<ProjectRow>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<ProjectProbe> = projects::Entity::find_by_id(*project_id)
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::Identifier,
            projects::Column::Name,
            projects::Column::CoverImage,
            projects::Column::CoverImageAssetId,
            projects::Column::LogoProps,
            projects::Column::Description,
            projects::Column::IsDefault,
        ])
        .into_model::<ProjectProbe>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|r| ProjectRow {
        id: r.id,
        identifier: r.identifier,
        name: r.name,
        cover_image: r.cover_image,
        cover_image_asset_id: r.cover_image_asset_id,
        logo_props: r.logo_props,
        description: r.description,
        is_default: r.is_default,
    }))
}

/// `cover_image_url` (`db/models/project.py:176-185`): the cover asset's
/// Decision from one `file_assets` cover lookup. The pre-migration
/// `Option<(String,)>` decode made a present row with a NULL
/// `entity_type` a decode error (500); preserved exactly here. Do not
/// align to Django (which returns the asset URL, `None` for NULL or
/// unmatched types) — that divergence is tracked separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverDecision {
    /// No asset row, or a non-static entity type: serve the raw
    /// `cover_image` text (or null).
    Raw,
    /// A static cover type: serve `/api/assets/v2/static/<id>/`.
    Static,
    /// A present row with a NULL `entity_type`: preserve the old 500.
    ServerError,
}

fn cover_decision(row: Option<Option<String>>) -> CoverDecision {
    match row {
        None => CoverDecision::Raw,
        Some(None) => CoverDecision::ServerError,
        Some(Some(entity_type)) => match entity_type.as_str() {
            "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
                CoverDecision::Static
            }
            _ => CoverDecision::Raw,
        },
    }
}

/// URL, else the raw `cover_image` text, else null.
async fn cover_image_url(pool: &PgPool, project: &ProjectRow) -> Result<Option<String>, Denial> {
    if let Some(asset_id) = project.cover_image_asset_id {
        let db = pidash_db::pool::sea_orm_connection(pool);
        let row: Option<Option<String>> = file_assets::Entity::find_by_id(asset_id)
            .select_only()
            .column(file_assets::Column::EntityType)
            .into_tuple()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        match cover_decision(row) {
            CoverDecision::ServerError => return Err(Denial::ServerError),
            CoverDecision::Static => {
                return Ok(Some(format!("/api/assets/v2/static/{asset_id}/")));
            }
            CoverDecision::Raw => return Ok(project.cover_image.clone()),
        }
    }
    Ok(project.cover_image.clone())
}

fn opt_string(text: &Option<String>) -> serde_json::Value {
    text.clone()
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null)
}

fn opt_dt(moment: &Option<DateTime<Utc>>, timezone: chrono_tz::Tz) -> serde_json::Value {
    moment
        .map(|dt| serde_json::Value::String(render_datetime_in(&dt, &timezone)))
        .unwrap_or(serde_json::Value::Null)
}

/// `ProjectLiteSerializer` (`app/serializers/project.py:120-132`), the
/// `project_detail` nest: 8 keys in `Meta.fields` order. `cover_image`
/// renders the raw text (null when unset); `cover_image_url` the asset
/// URL (null when no asset resolves).
fn render_project_detail(project: &ProjectRow, cover_url: Option<String>) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(8);
    map.insert(
        "id".to_string(),
        serde_json::Value::String(project.id.to_string()),
    );
    map.insert(
        "identifier".to_string(),
        serde_json::Value::String(project.identifier.clone()),
    );
    map.insert(
        "name".to_string(),
        serde_json::Value::String(project.name.clone()),
    );
    map.insert("cover_image".to_string(), opt_string(&project.cover_image));
    map.insert(
        "cover_image_url".to_string(),
        cover_url
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
    );
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

/// `IntakeSerializer` (`app/serializers/intake.py:17-24`): keys in
/// [`pidash_services::app_intake::shape::INTAKE_KEY_ORDER`]. `id` is the
/// `BaseSerializer` PK field (string form); `created_by` / `updated_by`
/// / `project` / `workspace` render as id-or-null strings;
/// `pending_issue_count` is the queryset annotation; datetimes render in
/// the request user's zone ([`render_datetime_in`]).
fn render_intake(
    intake: &IntakeRow,
    project_detail: serde_json::Value,
    pending_issue_count: i64,
    timezone: chrono_tz::Tz,
) -> serde_json::Value {
    let mut map =
        serde_json::Map::with_capacity(pidash_services::app_intake::shape::INTAKE_KEY_ORDER.len());
    map.insert(
        "id".to_string(),
        serde_json::Value::String(intake.id.to_string()),
    );
    map.insert("project_detail".to_string(), project_detail);
    map.insert(
        "pending_issue_count".to_string(),
        serde_json::Value::Number(pending_issue_count.into()),
    );
    map.insert(
        "created_at".to_string(),
        serde_json::Value::String(render_datetime_in(&intake.created_at, &timezone)),
    );
    map.insert(
        "updated_at".to_string(),
        serde_json::Value::String(render_datetime_in(&intake.updated_at, &timezone)),
    );
    map.insert(
        "deleted_at".to_string(),
        opt_dt(&intake.deleted_at, timezone),
    );
    map.insert(
        "name".to_string(),
        serde_json::Value::String(intake.name.clone()),
    );
    map.insert(
        "description".to_string(),
        serde_json::Value::String(intake.description.clone()),
    );
    map.insert(
        "is_default".to_string(),
        serde_json::Value::Bool(intake.is_default),
    );
    map.insert("view_props".to_string(), intake.view_props.clone());
    map.insert("logo_props".to_string(), intake.logo_props.clone());
    map.insert("created_by".to_string(), opt_uuid(&intake.created_by_id));
    map.insert("updated_by".to_string(), opt_uuid(&intake.updated_by_id));
    map.insert(
        "project".to_string(),
        serde_json::Value::String(intake.project_id.to_string()),
    );
    map.insert(
        "workspace".to_string(),
        serde_json::Value::String(intake.workspace_id.to_string()),
    );
    serde_json::Value::Object(map)
}

/// Render one intake row end to end: project nest plus the serializer
/// body. The tenant resolution guarantees the project row, so a miss
/// here is the 500 envelope (unreachable through the routes).
pub async fn render_intake_row(
    pool: &PgPool,
    intake: &IntakeRow,
    pending_issue_count: i64,
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, Denial> {
    let project = match fetch_project(pool, &intake.project_id).await? {
        Some(project) => project,
        None => return Err(Denial::ServerError),
    };
    let cover_url = cover_image_url(pool, &project).await?;
    let detail = render_project_detail(&project, cover_url);
    Ok(render_intake(intake, detail, pending_issue_count, timezone))
}

// ---------------------------------------------------------------------------
// Input validation (`IntakeSerializer`, create + partial)
// ---------------------------------------------------------------------------

/// `name` column bound (`db/models/intake.py:13`).
pub const NAME_MAX_LENGTH: usize = 255;

/// Required-field message (`name` + `deleted_at` on create,
/// QUIRK-unique-required).
pub const REQUIRED_MESSAGE: &str = "This field is required.";

/// Validated intake writes. `None` fields were absent (partial) or failed
/// validation; nullable columns use the inner `Option` (`Some(None)` is
/// an explicit null).
#[derive(Debug, Default)]
pub struct ValidatedIntake {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_default: Option<bool>,
    pub view_props: Option<serde_json::Value>,
    pub logo_props: Option<serde_json::Value>,
    pub deleted_at: Option<Option<DateTime<Utc>>>,
    pub created_by: Option<Option<Uuid>>,
    pub updated_by: Option<Option<Uuid>>,
}

/// Mirror `IntakeSerializer` input handling (`intake.py:17-24`):
/// read-only fields (`id`, `project_detail`, `pending_issue_count`,
/// `project`, `workspace`, auto datetimes) and unknown keys are ignored;
/// every other key validates as its DRF field. On create (`partial =
/// false`) `name` + `deleted_at` are required (QUIRK-unique-required);
/// on partial every field is optional.
pub async fn validate_intake(
    pool: &PgPool,
    top: &serde_json::Map<String, serde_json::Value>,
    partial: bool,
    errors: &mut FieldErrors,
) -> Result<Option<ValidatedIntake>, Denial> {
    let mut validated = ValidatedIntake::default();
    if let Some(value) = top.get("name") {
        validated.name = validate_char(errors, "name", value, NAME_MAX_LENGTH, false, false);
    } else if !partial {
        push_error(errors, "name", REQUIRED_MESSAGE.to_owned());
    }
    if let Some(value) = top.get("description") {
        validated.description =
            validate_char(errors, "description", value, usize::MAX, true, false);
    }
    if let Some(value) = top.get("is_default") {
        validated.is_default = validate_boolean(errors, "is_default", value, false);
    }
    if let Some(value) = top.get("view_props") {
        validated.view_props = validate_json_field(errors, "view_props", value);
    }
    if let Some(value) = top.get("logo_props") {
        validated.logo_props = validate_json_field(errors, "logo_props", value);
    }
    if let Some(value) = top.get("deleted_at") {
        validated.deleted_at = match validate_datetime(errors, "deleted_at", value, true) {
            Some(dt) => Some(Some(dt)),
            None if value.is_null() => Some(None),
            None => None,
        };
    } else if !partial {
        push_error(errors, "deleted_at", REQUIRED_MESSAGE.to_owned());
    }
    if let Some(value) = top.get("created_by") {
        validated.created_by = match validate_user_pk(pool, errors, "created_by", value).await? {
            Some(id) => Some(Some(id)),
            None if value.is_null() => Some(None),
            None => None,
        };
    }
    if let Some(value) = top.get("updated_by") {
        validated.updated_by = match validate_user_pk(pool, errors, "updated_by", value).await? {
            Some(id) => Some(Some(id)),
            None if value.is_null() => Some(None),
            None => None,
        };
    }
    if errors.is_empty() {
        Ok(Some(validated))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `list` (`base.py:72-75`): ADMIN + MEMBER gate, then the first queryset
/// row through `IntakeSerializer`, 200 — or a null 200 when the tenant
/// has no intake (QUIRK-list-null).
pub async fn list(
    State(state): State<AppState>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match intake_tenant(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    // Decorator (`:72`): ADMIN + MEMBER.
    let gate = guards::intake_list_gate(&membership.as_guard());
    if gate.is_err() {
        return guard_denial(gate);
    }
    let first = match fetch_intake(pool, &slug, &project_id, None, false).await {
        Ok(first) => first,
        Err(denial) => return denial.into_response(),
    };
    let Some((intake, pending)) = first else {
        // `IntakeSerializer(None).data` — null body, still 200.
        return raw_json_response("null".to_owned());
    };
    let body = match render_intake_row(pool, &intake, pending, actor.timezone).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    raw_json_response(body.to_string())
}

/// `create` (the `BaseViewSet` create flow into `perform_create`,
/// `base.py:77-79`): session auth, then the serializer validation
/// (400s), then BUG-perform-create — the decorated `perform_create`
/// dies on `request.user` before any write, so every valid payload
/// answers 500 and nothing is inserted. No membership check and no
/// tenant lookup run: the decorator crashes before evaluating a role
/// (which is why guests see the same 500 on valid payloads) and the
/// uniqueness validators are skipped entirely
/// (QUIRK-no-unique-check), so even duplicate names answer the 500.
pub async fn create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
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
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let serde_json::Value::Object(top) = data else {
        return Denial::ServerError.into_response();
    };
    let mut field_errors = FieldErrors::new();
    let validated = match validate_intake(pool, &top, false, &mut field_errors).await {
        Ok(validated) => validated,
        Err(denial) => return denial.into_response(),
    };
    if !field_errors.is_empty() {
        return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
    }
    if validated.is_none() {
        return Denial::ServerError.into_response();
    }
    // BUG-perform-create: `serializer.save(project_id=...)` never runs —
    // the decorator crashes first. The actor read above keeps the auth
    // shape (anonymous still 401s before validation).
    let _ = actor;
    Denial::ServerError.into_response()
}

/// `retrieve` (the DRF-default `ModelViewSet` retrieve — no override in
/// `IntakeViewSet`): tenant-scoped get plus the full serializer, 200. A
/// miss answers `get_object`'s `Http404` (`{"detail": "No Intake
/// matches the given query."}`). Undecorated: any authenticated user
/// passes (BUG-intake-detail-undecorated).
pub async fn retrieve(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match intake_tenant(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    let found = match fetch_intake(pool, &slug, &project_id, Some(pk), false).await {
        Ok(found) => found,
        Err(denial) => return denial.into_response(),
    };
    let Some((intake, pending)) = found else {
        return Denial::IntakeNotFound.into_response();
    };
    let body = match render_intake_row(pool, &intake, pending, actor.timezone).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    raw_json_response(body.to_string())
}

/// `partial_update` (the DRF-default `ModelViewSet` partial update — no
/// override in `IntakeViewSet`): tenant-scoped get (404 like retrieve),
/// partial serializer validation, `BaseModel.save` (which stamps
/// `updated_by` from the request user via crum and `updated_at` via
/// `auto_now`), then the full serializer, 200. Undecorated: any
/// authenticated user passes (BUG-intake-detail-undecorated).
pub async fn partial_update(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match intake_tenant(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    // `get_object` runs before the body parses: a miss 404s even with a
    // malformed payload.
    let stored = match fetch_intake(pool, &slug, &project_id, Some(pk), false).await {
        Ok(stored) => stored,
        Err(denial) => return denial.into_response(),
    };
    let Some((stored, _)) = stored else {
        return Denial::IntakeNotFound.into_response();
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let serde_json::Value::Object(top) = data else {
        return Denial::ServerError.into_response();
    };
    let mut field_errors = FieldErrors::new();
    let validated = match validate_intake(pool, &top, true, &mut field_errors).await {
        Ok(validated) => validated,
        Err(denial) => return denial.into_response(),
    };
    if !field_errors.is_empty() {
        return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
    }
    let Some(validated) = validated else {
        return Denial::ServerError.into_response();
    };
    // `serializer.save()` + `BaseModel.save`: provided columns update;
    // `updated_by` always becomes the request user (crum) and
    // `updated_at` stamps `auto_now`, even with an empty patch.
    let now = Utc::now();
    let name = validated.name.unwrap_or(stored.name);
    let description = validated.description.unwrap_or(stored.description);
    let is_default = validated.is_default.unwrap_or(stored.is_default);
    let view_props = validated.view_props.unwrap_or(stored.view_props);
    let logo_props = validated.logo_props.unwrap_or(stored.logo_props);
    let deleted_at = validated.deleted_at.unwrap_or(stored.deleted_at);
    let created_by_id = validated.created_by.unwrap_or(stored.created_by_id);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let write = intake_entity::Entity::update_many()
        .col_expr(
            intake_entity::Column::Name,
            sea_orm::sea_query::Expr::value(name),
        )
        .col_expr(
            intake_entity::Column::Description,
            sea_orm::sea_query::Expr::value(description),
        )
        .col_expr(
            intake_entity::Column::IsDefault,
            sea_orm::sea_query::Expr::value(is_default),
        )
        .col_expr(
            intake_entity::Column::ViewProps,
            sea_orm::sea_query::Expr::value(view_props),
        )
        .col_expr(
            intake_entity::Column::LogoProps,
            sea_orm::sea_query::Expr::value(logo_props),
        )
        .col_expr(
            intake_entity::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(
                deleted_at.map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())),
            ),
        )
        .col_expr(
            intake_entity::Column::CreatedById,
            sea_orm::sea_query::Expr::value(created_by_id),
        )
        .col_expr(
            intake_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(
                now.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
            ),
        )
        .col_expr(
            intake_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(actor.id),
        )
        .filter(intake_entity::Column::Id.eq(pk))
        .exec(&db)
        .await;
    if let Err(error) = write {
        // The uniqueness validators are skipped (QUIRK-no-unique-check),
        // so a duplicate name reaches the database: the partial unique
        // constraint fires and `handle_exception` answers the
        // `IntegrityError` branch (`{"error": "The payload is not
        // valid"}`).
        if pidash_db::orm_tx::is_integrity_violation(&error) {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
        return Denial::ServerError.into_response();
    }
    // Re-read the just-written row for the response (`serializer.data`
    // renders the saved instance): the live-only scope would miss a row
    // this PATCH soft-deleted and 500 where Django 200s (PIDASHCONV-773
    // probe find).
    let updated = match fetch_intake(pool, &slug, &project_id, Some(pk), true).await {
        Ok(updated) => updated,
        Err(denial) => return denial.into_response(),
    };
    let Some((updated, pending)) = updated else {
        return Denial::ServerError.into_response();
    };
    let body = match render_intake_row(pool, &updated, pending, actor.timezone).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    raw_json_response(body.to_string())
}

/// `destroy` (`base.py:81-91`): ADMIN + MEMBER gate, then the tenant
/// lookup — a miss dereferences `None.is_default` → 500
/// (BUG-destroy-missing) — then the default-intake 400, then the soft
/// delete (204) with the related-objects sweep enqueued best-effort.
pub async fn destroy(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match intake_tenant(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    // `Intake.objects.filter(workspace__slug, project_id, pk).first()`
    // — soft-delete-scoped, so deleted rows read as missing.
    let found = match fetch_intake(pool, &slug, &project_id, Some(pk), false).await {
        Ok(found) => found,
        Err(denial) => return denial.into_response(),
    };
    // Decorator (`:81`) runs before the lookup dereference: outsiders
    // 403 even for missing rows. The gate reads the role first, then
    // the lookup (missing → 500, default → 400).
    let lookup = match &found {
        Some((intake, _)) => guards::IntakeLookup::Found {
            is_default: intake.is_default,
        },
        None => guards::IntakeLookup::Missing,
    };
    let gate = guards::intake_destroy_gate(&membership.as_guard(), lookup);
    if gate.is_err() {
        return guard_denial(gate);
    }
    let Some((intake, _)) = found else {
        return Denial::ServerError.into_response();
    };
    // `SoftDeleteModel.delete` (`db/mixins.py:71-77`): stamp + save
    // (`updated_by` from crum, `updated_at` from `auto_now`).
    let now = Utc::now();
    if let Err(denial) = soft_delete_intake(pool, &intake.id, &actor.id, now).await {
        return denial.into_response();
    }
    enqueue_soft_delete(pool, "db", "intake", &pk).await;
    Response::builder()
        .status(axum::http::StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204")
}

/// `SoftDeleteModel.delete` (`db/mixins.py:71-77`): stamp + save.
async fn soft_delete_intake(
    pool: &PgPool,
    intake_id: &Uuid,
    actor_id: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let tz = chrono::FixedOffset::east_opt(0).unwrap();
    intake_entity::Entity::update_many()
        .col_expr(
            intake_entity::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(now.with_timezone(&tz)),
        )
        .col_expr(
            intake_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now.with_timezone(&tz)),
        )
        .col_expr(
            intake_entity::Column::UpdatedById,
            sea_orm::sea_query::Expr::value(*actor_id),
        )
        .filter(intake_entity::Column::Id.eq(*intake_id))
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn errors() -> FieldErrors {
        FieldErrors::new()
    }

    fn bool_of(value: &serde_json::Value) -> (Option<bool>, FieldErrors) {
        let mut errs = errors();
        let out = validate_boolean(&mut errs, "is_default", value, false);
        (out, errs)
    }

    #[test]
    fn validate_datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01, read as UTC (probed live both backends).
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let mut errs = errors();
            let parsed =
                validate_datetime(&mut errs, "deleted_at", &json!(text), true).expect(text);
            assert_eq!(parsed.to_rfc3339(), "1900-01-01T00:00:00+00:00", "{text:?}");
            assert!(errs.is_empty(), "{text:?}");
        }
        // Near-misses stay invalid (exact match on the raw text, both
        // sides probed — padding fails even though this port trims).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            let mut errs = errors();
            assert!(
                validate_datetime(&mut errs, "deleted_at", &json!(text), true).is_none(),
                "{text:?}"
            );
        }
    }

    /// `cover_image_url` lookup outcome (NEWPIDASH-113): the
    /// pre-migration `Option<(String,)>` decode made a present row
    /// with a NULL `entity_type` a 500; preserved exactly.
    #[test]
    fn cover_decision_preserves_old_null_decode_error() {
        // No row: raw fall-through, as before.
        assert_eq!(cover_decision(None), CoverDecision::Raw);
        // NULL entity_type: the old 500, not a raw-text fallback.
        assert_eq!(cover_decision(Some(None)), CoverDecision::ServerError);
        // Static cover types serve the static path.
        for kind in [
            "WORKSPACE_LOGO",
            "USER_AVATAR",
            "USER_COVER",
            "PROJECT_COVER",
        ] {
            assert_eq!(
                cover_decision(Some(Some(kind.to_owned()))),
                CoverDecision::Static,
                "{kind}"
            );
        }
        // Anything else (incl. empty) serves the raw text, as before.
        for kind in ["ISSUE_ATTACHMENT", "unknown", ""] {
            assert_eq!(
                cover_decision(Some(Some(kind.to_owned()))),
                CoverDecision::Raw,
                "{kind}"
            );
        }
    }

    /// DRF `TRUE_VALUES` / `FALSE_VALUES` (`fields.py:665-686`),
    /// verified live (probes `bool_*`).
    #[test]
    fn boolean_spellings_match_drf_sets() {
        for truthy in ["true", "t", "y", "yes", "on", "1", "YES", "On"] {
            let (out, errs) = bool_of(&json!(truthy));
            assert_eq!(out, Some(true), "{truthy}");
            assert!(errs.is_empty());
        }
        for falsy in ["false", "f", "n", "no", "off", "0", "NO", "Off"] {
            let (out, errs) = bool_of(&json!(falsy));
            assert_eq!(out, Some(false), "{falsy}");
            assert!(errs.is_empty());
        }
        assert_eq!(bool_of(&json!(true)).0, Some(true));
        assert_eq!(bool_of(&json!(false)).0, Some(false));
        assert_eq!(bool_of(&json!(1)).0, Some(true));
        assert_eq!(bool_of(&json!(0)).0, Some(false));
        assert_eq!(bool_of(&json!(1.0)).0, Some(true));
        assert_eq!(bool_of(&json!(0.0)).0, Some(false));
        for bad in [json!("maybe"), json!("yes please"), json!(2), json!([])] {
            let (out, errs) = bool_of(&bad);
            assert_eq!(out, None, "{bad}");
            assert_eq!(
                errs.get("is_default"),
                Some(&json!(["Must be a valid boolean."]))
            );
        }
        let (out, errs) = bool_of(&json!(null));
        assert_eq!(out, None);
        assert_eq!(
            errs.get("is_default"),
            Some(&json!(["This field may not be null."]))
        );
    }

    /// DRF `JSONField` passes parsed-JSON input through untouched
    /// (`fields.py:1774-1784`), verified live (`view_props_*` probes);
    /// null is rejected (no `null=True` on the model fields).
    #[test]
    fn json_field_passes_everything_through() {
        for value in [
            json!("abc"),
            json!("{\"a\": 1}"),
            json!([1, 2]),
            json!({}),
            json!(7),
        ] {
            let mut errs = errors();
            let out = validate_json_field(&mut errs, "view_props", &value);
            assert_eq!(out, Some(value.clone()));
            assert!(errs.is_empty());
        }
        let mut errs = errors();
        assert_eq!(
            validate_json_field(&mut errs, "view_props", &json!(null)),
            None
        );
        assert_eq!(
            errs.get("view_props"),
            Some(&json!(["This field may not be null."]))
        );
    }

    /// Python `str()` rendering for the UUID failure message, verified
    /// live (`created_by_*` / `fk_*` probes).
    #[test]
    fn python_str_renders_like_django() {
        assert_eq!(python_str(&json!("not-a-uuid")), "not-a-uuid");
        assert_eq!(python_str(&json!(5)), "5");
        assert_eq!(python_str(&json!(5.0)), "5.0");
        assert_eq!(python_str(&json!(["x"])), "['x']");
        assert_eq!(python_str(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_str(&json!(null)), "None");
    }

    /// `IntakeSerializer` key order follows `INTAKE_KEY_ORDER`
    /// (fixture `serializers/intake.golden.json`).
    #[test]
    fn intake_key_order_matches_fixture() {
        assert_eq!(
            pidash_services::app_intake::shape::INTAKE_KEY_ORDER,
            &[
                "id",
                "project_detail",
                "pending_issue_count",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "is_default",
                "view_props",
                "logo_props",
                "created_by",
                "updated_by",
                "project",
                "workspace",
            ]
        );
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
