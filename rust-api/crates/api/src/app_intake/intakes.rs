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
//! and `HEAD`-adjacent handling per [`super::owned`].
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
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_services::app_intake::permissions as guards;

use super::{
    actor, enqueue_soft_delete, guard_denial, load_membership, parse_body, pool_of,
    raw_json_response, Denial,
};
use crate::serializer::render_datetime_in;
use crate::state::AppState;

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
    let upper = project_raw.trim().to_uppercase();
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// Parse a UUID path segment: Django's `<uuid:>` converter leaves the
/// route unmatched, so unparseable ids answer the `handler404` body
/// (`custom_404_view`, prod).
#[allow(clippy::result_large_err)]
pub fn parse_pk(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::PageNotFound.into_response())
}

/// Serializer field errors: DRF's `{field: [messages]}` mapping plus the
/// `non_field_errors` list. `serde_json::Map` iterates in key order, so
/// multi-field bodies are deterministic.
pub type FieldErrors = serde_json::Map<String, serde_json::Value>;

fn push_error(errors: &mut FieldErrors, field: &str, message: String) {
    errors
        .entry(field.to_owned())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .expect("error list")
        .push(serde_json::Value::String(message));
}

/// DRF `CharField.to_internal_value`: bools, lists and dicts fail;
/// numbers stringify; strings trim. `None` is rejected upstream with
/// the null message.
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

fn validate_char(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    max_length: usize,
    allow_blank: bool,
    allow_null: bool,
) -> Option<String> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    match char_internal(value) {
        Err(()) => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
        Ok(text) => {
            if text.is_empty() && !allow_blank {
                push_error(errors, field, "This field may not be blank.".to_owned());
                return None;
            }
            // `[:255]` counts code points; over-long is a 400, never a
            // panic.
            if text.chars().count() > max_length {
                push_error(
                    errors,
                    field,
                    format!("Ensure this field has no more than {max_length} characters."),
                );
                return None;
            }
            Some(text)
        }
    }
}

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

/// DRF `DateTimeField.to_internal_value`: ISO-8601 with `Z`/offsets
/// (plus a bare date, read as midnight UTC, like DRF's date fallback).
fn validate_datetime(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<DateTime<Utc>> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    let text = match value {
        serde_json::Value::String(text) => text.trim().to_string(),
        _ => {
            push_error(errors, field, datetime_format_message());
            return None;
        }
    };
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0).expect("midnight").and_utc());
    }
    match chrono::DateTime::parse_from_rfc3339(&text) {
        Ok(dt) => Some(dt.with_timezone(&Utc)),
        Err(_) => {
            push_error(errors, field, datetime_format_message());
            None
        }
    }
}

fn datetime_format_message() -> String {
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        .to_owned()
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
    let row: Option<(Uuid,)> = sqlx::query_as(r#"SELECT id FROM users WHERE id = $1"#)
        .bind(id)
        .fetch_optional(pool)
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

const INTAKE_COLS: &str = concat!(
    "i.id, i.created_at, i.updated_at, i.created_by_id, i.updated_by_id, i.deleted_at, ",
    "i.project_id, i.workspace_id, i.name, i.description, i.is_default, i.view_props, i.logo_props"
);

impl IntakeRow {
    pub fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            project_id: row.try_get("project_id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            is_default: row.try_get("is_default")?,
            view_props: row.try_get("view_props")?,
            logo_props: row.try_get("logo_props")?,
        })
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
) -> Result<Option<(IntakeRow, i64)>, Denial> {
    let mut sql = format!(
        r#"SELECT {cols}, COUNT(ii.id) FILTER (WHERE ii.status = -2) AS pending_issue_count
           FROM intakes i INNER JOIN workspaces w ON (w.id = i.workspace_id)
           LEFT OUTER JOIN intake_issues ii ON (i.id = ii.intake_id AND ii.deleted_at IS NULL)
           WHERE (w.slug = $1 AND i.project_id = $2 AND i.deleted_at IS NULL"#,
        cols = INTAKE_COLS,
    );
    if pk.is_some() {
        sql.push_str(" AND i.id = $3");
    }
    sql.push_str(") GROUP BY i.id ORDER BY i.name ASC LIMIT 1");
    let mut query = sqlx::query(&sql).bind(slug).bind(project_id);
    if let Some(pk) = pk {
        query = query.bind(pk);
    }
    let row = query
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let intake = IntakeRow::get(&row).map_err(|_| Denial::ServerError)?;
            let pending: i64 = row
                .try_get("pending_issue_count")
                .map_err(|_| Denial::ServerError)?;
            Ok(Some((intake, pending)))
        }
    }
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

async fn fetch_project(pool: &PgPool, project_id: &Uuid) -> Result<Option<ProjectRow>, Denial> {
    let row = sqlx::query(
        r#"SELECT id, identifier, name, cover_image, cover_image_asset_id, logo_props, description, is_default FROM "projects" WHERE "projects"."id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| {
        Ok(ProjectRow {
            id: row.try_get("id").map_err(|_| Denial::ServerError)?,
            identifier: row.try_get("identifier").map_err(|_| Denial::ServerError)?,
            name: row.try_get("name").map_err(|_| Denial::ServerError)?,
            cover_image: row
                .try_get("cover_image")
                .map_err(|_| Denial::ServerError)?,
            cover_image_asset_id: row
                .try_get("cover_image_asset_id")
                .map_err(|_| Denial::ServerError)?,
            logo_props: row.try_get("logo_props").map_err(|_| Denial::ServerError)?,
            description: row
                .try_get("description")
                .map_err(|_| Denial::ServerError)?,
            is_default: row.try_get("is_default").map_err(|_| Denial::ServerError)?,
        })
    })
    .transpose()
}

/// `cover_image_url` (`db/models/project.py:176-185`): the cover asset's
/// URL, else the raw `cover_image` text, else null.
async fn cover_image_url(pool: &PgPool, project: &ProjectRow) -> Result<Option<String>, Denial> {
    if let Some(asset_id) = project.cover_image_asset_id {
        let row: Option<(String,)> = sqlx::query_as(
            r#"SELECT entity_type FROM "file_assets" WHERE "file_assets"."id" = $1"#,
        )
        .bind(asset_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if let Some((entity_type,)) = row {
            match entity_type.as_str() {
                "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
                    return Ok(Some(format!("/api/assets/v2/static/{asset_id}/")));
                }
                _ => return Ok(project.cover_image.clone()),
            }
        }
    }
    Ok(project.cover_image.clone())
}

fn opt_uuid(id: &Option<Uuid>) -> serde_json::Value {
    id.map(|id| serde_json::Value::String(id.to_string()))
        .unwrap_or(serde_json::Value::Null)
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
    let first = match fetch_intake(pool, &slug, &project_id, None).await {
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
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let found = match fetch_intake(pool, &slug, &project_id, Some(pk)).await {
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
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    // `get_object` runs before the body parses: a miss 404s even with a
    // malformed payload.
    let stored = match fetch_intake(pool, &slug, &project_id, Some(pk)).await {
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
    let write = sqlx::query(
        r#"UPDATE intakes SET name = $2, description = $3, is_default = $4, view_props = $5, logo_props = $6,
                  deleted_at = $7, created_by_id = $8, updated_at = $9, updated_by_id = $10
           WHERE id = $1"#,
    )
    .bind(pk)
    .bind(name)
    .bind(description)
    .bind(is_default)
    .bind(view_props)
    .bind(logo_props)
    .bind(deleted_at)
    .bind(created_by_id)
    .bind(now)
    .bind(actor.id)
    .execute(pool)
    .await;
    if let Err(error) = write {
        // The uniqueness validators are skipped (QUIRK-no-unique-check),
        // so a duplicate name reaches the database: the partial unique
        // constraint fires and `handle_exception` answers the
        // `IntegrityError` branch (`{"error": "The payload is not
        // valid"}`).
        if let sqlx::Error::Database(db) = &error {
            if db.code().as_deref() == Some("23505") || db.constraint().is_some() {
                return Denial::BadError("The payload is not valid".to_owned()).into_response();
            }
        }
        return Denial::ServerError.into_response();
    }
    let updated = match fetch_intake(pool, &slug, &project_id, Some(pk)).await {
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
    let pk = match parse_pk(&pk_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    // `Intake.objects.filter(workspace__slug, project_id, pk).first()`
    // — soft-delete-scoped, so deleted rows read as missing.
    let found = match fetch_intake(pool, &slug, &project_id, Some(pk)).await {
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
    sqlx::query(
        r#"UPDATE intakes SET deleted_at = $2, updated_at = $2, updated_by_id = $3 WHERE id = $1"#,
    )
    .bind(intake_id)
    .bind(now)
    .bind(actor_id)
    .execute(pool)
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
