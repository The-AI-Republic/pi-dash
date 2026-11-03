#![forbid(unsafe_code)]

//! Project work-item type handlers (PIDASHCONV-1679, Django NEWFRONT-984).
//!
//! Ports `app/views/issue/types.py` (`ProjectIssueTypesEndpoint`, GET list /
//! POST create) plus `app/serializers/issue_type.py` (both serializers) onto
//! the foundation crates. The Django side lives on `origin/main` only
//! (rust-dev predates it); every behavior below was probed against live
//! Django (see the issue workpad) and is pinned by
//! `contract-tests/app_issues/test_issue_types.py`.
//!
//! Owned path (registration lives in [`super::routes`]; every other method
//! proxies to Django, which owns the 405s and `OPTIONS` metadata):
//!
//! * `GET`/`POST .../issue-types/`
//!
//! Layering: body negotiation + CPython JSON errors come from the shared
//! `v1_cycles_modules` ports, and the DRF scalar coercions, ordered field
//! errors, and the `@allow_permission` gate from the labels/attachments
//! sibling (`handlers_labels_attachments`, whose helpers this family
//! shares). This module owns the `ProjectEntityPermission` class gates,
//! the create validation, the SQL text, and the 8-key read shape.
//!
//! Gate order (Django's, verified by probe): identifier rewrite (404
//! `detail` miss) → class gate (`ProjectEntityPermission`: GET any live
//! project membership, POST role ADMIN/MEMBER; else the DRF-default 403)
//! → `@allow_permission` decorator (GET ADMIN/MEMBER/GUEST, POST ADMIN,
//! each with the member + workspace-admin bypass; else the allow-style
//! 403) → project-in-workspace lookup (404 `error` miss) → logic. The
//! view 404 fires for soft-deleted projects (their membership rows live
//! on); unknown/foreign projects 403 at the class gate.
//!
//! Django dead code (observed, not reproduced): `validate_name`'s `"Type
//! name cannot be blank."` can never fire — `CharField.run_validation`
//! rejects whitespace-only input with `"This field may not be blank."`
//! before `validate_name` ever runs (probed with `"   "`).
//!
//! Ported bugs (also listed in the PR):
//!
//! * A non-finite `level` (`"nan"`, `"inf"`, `"1e400"`) is stored, then
//!   Django's strict renderer 500s: POST writes both rows and answers
//!   500, and every later GET on that project 500s too. Rust performs the
//!   same writes, then answers its generic 500 (status parity; Django's
//!   own 500 body differs between DEBUG and prod, so bytes are
//!   unmatchable).
//! * HTML `logo_props="null"` parses to JSON null, passes validation (the
//!   raw string is non-empty), and dies on the NOT NULL jsonb column:
//!   400 `{"error":"The payload is not valid"}` with nothing written.
//!   Any `IntegrityError` on either insert maps there, as
//!   `handle_exception` does.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

use super::handlers_labels_attachments::{
    data_has_file, drf_bool_of_value, drf_float_of_value, drf_str_of_value, gate_decorator,
    is_integrity_violation, json_bad_request, json_not_found, object_or_errors, FieldErrors,
    RequestData,
};
use super::Denial;
use crate::middleware::SessionHandle;
use crate::permissions::DefaultPermissionDenied;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_request_data, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

use pidash_compat::response::{json_created, json_ok};

/// Type collection (`app/urls/issue.py:144-148` on main).
pub const ISSUE_TYPES_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/issue-types/";

/// View-body project miss (`types.py`, probed lowercase `error` body).
const TYPE_PROJECT_NOT_FOUND_BODY: &str = r#"{"error":"Project not found"}"#;
/// `handle_exception` `IntegrityError` arm (`views/base.py`): the HTML
/// `"null"` logo (JSON null into NOT NULL jsonb); nothing is written.
const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;

/// Type HTML-input shape: the float/boolean scalars skip blank form input
/// (probed absent-then-default); `name`/`description` keep `""` for the
/// blank arms and `logo_props` keeps `""` for the JSON-parse arm.
const TYPE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &["is_epic", "level"],
};

// ---- gates ------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`): missing
/// snapshots and non-UUID ids are anonymous, rejected 401 first.
fn actor_user_id(extension: Option<axum::Extension<SessionHandle>>) -> Result<Uuid, Denial> {
    extension
        .map(|handle| handle.0)
        .and_then(|handle| {
            let mut session = handle.snapshot();
            session
                .get("_auth_user_id")?
                .as_str()?
                .to_owned()
                .parse::<Uuid>()
                .ok()
        })
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `ProjectEntityPermission` safe-method arm
/// (`app/permissions/project.py`, `ProjectEntityPermission`): any live
/// project membership in this slug; else the DRF-default 403. The
/// `project_identifier` branch needs a view attribute this endpoint lacks.
#[allow(clippy::result_large_err)]
async fn gate_entity_safe(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Response> {
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if member.is_some() {
        Ok(())
    } else {
        Err(DefaultPermissionDenied.into_response())
    }
}

/// `ProjectEntityPermission` POST arm: a live ADMIN/MEMBER project
/// membership; guests fail here with the DRF-default 403 (the decorator
/// bypass below is unreachable for them).
#[allow(clippy::result_large_err)]
async fn gate_entity_post(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Response> {
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
           AND pm.role IN (20, 15) AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if member.is_some() {
        Ok(())
    } else {
        Err(DefaultPermissionDenied.into_response())
    }
}

/// View-body project lookup (`types.py`): the live project row in this
/// workspace; a soft-deleted (or, unreachable past the gates, foreign)
/// project answers the view's 404 `error` body.
#[allow(clippy::result_large_err)]
async fn project_workspace_id(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Uuid, Response> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT p.workspace_id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    row.map(|row| row.0)
        .ok_or_else(|| json_not_found(TYPE_PROJECT_NOT_FOUND_BODY))
}

// ---- request bodies ---------------------------------------------------------

/// Body negotiation over the shared port: empty bodies read `{}`, JSON
/// goes through the CPython-error parser, forms through the spec map. A
/// local copy of the labels sibling's negotiation (not a call) because
/// its 415 renders a capital-`Detail` key while Django answers lowercase
/// (probed; the sibling line is a pre-existing divergence for its owner).
#[allow(clippy::result_large_err)]
fn negotiate_input(headers: &HeaderMap, body: &[u8]) -> Result<RequestData, Response> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => unsupported_media_type(message),
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message).into_response(),
        shared_body::BodyError::ServerError => Denial::ServerError.into_response(),
    };
    match shared_body::negotiate_body(headers, body, &TYPE_BODY_SPEC).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(RequestData {
            value: Value::Object(Map::new()),
            files: BTreeMap::new(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText { text, .. } => parse_request_data(text.as_bytes())
            .map(|parsed| RequestData {
                value: to_serde_publish(&parsed),
                files: BTreeMap::new(),
                is_html: false,
            })
            .map_err(|fail| match fail {
                JsonFail::Message(detail) => {
                    Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")).into_response()
                }
                JsonFail::Recursion => Denial::ServerError.into_response(),
            }),
        shared_body::NegotiatedBody::Form { map, files, .. } => Ok(RequestData {
            value: Value::Object(map),
            files,
            is_html: true,
        }),
    }
}

/// DRF `UnsupportedMediaType` body with the lowercase `detail` key and the
/// full content-type echo (probed).
fn unsupported_media_type(message: String) -> Response {
    let body = format!(
        "{{\"detail\":{}}}",
        serde_json::to_string(&message).expect("415 body")
    );
    Response::builder()
        .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("415 response")
}

// ---- create validation ------------------------------------------------------

/// Validated type write: `None` members are absent (create fills model
/// defaults); `logo_props == Some(None)` is the HTML `"null"` string,
/// parsed to JSON null and bound as SQL NULL so the insert fails exactly
/// like Django's (`IntegrityError` → payload-not-valid).
#[derive(Debug, Default)]
struct TypeWrite {
    name: Option<String>,
    description: Option<String>,
    logo_props: Option<Option<Value>>,
    is_epic: Option<bool>,
    level: Option<f64>,
}

/// Python `str.strip()` emptiness over `White_Space` plus U+001C-U+001F
/// (the list family's shared definition).
fn is_py_blank(text: &str) -> bool {
    text.trim_matches(super::is_py_strip_ws).is_empty()
}

/// Pure half of `IssueTypeCreateSerializer` validation
/// (`serializers/issue_type.py`): field errors in `Meta.fields` order over
/// `{name, description, logo_props, is_epic, level}`; read-only + unknown
/// keys silently dropped (probed). `CharField` trims before the blank and
/// max-length checks (`fields.py`, probed); the NUL validator trails the
/// max-length validator (probed two-message order).
fn validate_type_fields(
    data: &RequestData,
    object: &Map<String, Value>,
) -> Result<TypeWrite, FieldErrors> {
    let mut errors = FieldErrors::default();
    let mut write = TypeWrite::default();

    // `name` (`CharField(255)`, required, no blank): the blank check runs
    // on the raw value, so whitespace-only input reports blank (never the
    // serializer's dead `validate_name` message); max-length and NUL run
    // on the stripped value.
    if data_has_file(&data.files, "name") {
        errors.push("name", "Not a valid string.".to_owned());
    } else if let Some(raw) = object.get("name") {
        match raw {
            Value::Null => errors.push("name", "This field may not be null.".to_owned()),
            Value::String(text) if is_py_blank(text) => {
                errors.push("name", "This field may not be blank.".to_owned());
            }
            _ => match drf_str_of_value(raw) {
                Some(text) => {
                    let trimmed = text.trim_matches(super::is_py_strip_ws);
                    if trimmed.chars().count() > 255 {
                        errors.push(
                            "name",
                            "Ensure this field has no more than 255 characters.".to_owned(),
                        );
                    }
                    if trimmed.contains('\0') {
                        errors.push("name", "Null characters are not allowed.".to_owned());
                    }
                    write.name = Some(trimmed.to_owned());
                }
                None => errors.push("name", "Not a valid string.".to_owned()),
            },
        }
    } else {
        errors.push("name", "This field is required.".to_owned());
    }

    // `description` (`CharField`, blank, no max): blank input (raw or
    // whitespace-only) stores `""`; otherwise trimmed with the NUL check.
    if data_has_file(&data.files, "description") {
        errors.push("description", "Not a valid string.".to_owned());
    } else if let Some(raw) = object.get("description") {
        match raw {
            Value::Null => errors.push("description", "This field may not be null.".to_owned()),
            Value::String(text) if is_py_blank(text) => {
                write.description = Some(String::new());
            }
            _ => match drf_str_of_value(raw) {
                Some(text) => {
                    let trimmed = text.trim_matches(super::is_py_strip_ws);
                    if trimmed.contains('\0') {
                        errors.push("description", "Null characters are not allowed.".to_owned());
                    }
                    write.description = Some(trimmed.to_owned());
                }
                None => errors.push("description", "Not a valid string.".to_owned()),
            },
        }
    }

    // `logo_props` (`JSONField`, default): JSON values pass through
    // (strings, even `""` and valid-JSON text, are kept, never parsed);
    // HTML strings parse as JSON (probed stored-parsed), `"null"` parses
    // to SQL NULL, and anything unparsable — including HTML `""` — fails.
    if data_has_file(&data.files, "logo_props") {
        errors.push("logo_props", "Value must be valid JSON.".to_owned());
    } else if let Some(raw) = object.get("logo_props") {
        match raw {
            Value::Null => errors.push("logo_props", "This field may not be null.".to_owned()),
            Value::String(text) if data.is_html => match serde_json::from_str::<Value>(text) {
                Ok(parsed) => {
                    write.logo_props = Some(if parsed.is_null() { None } else { Some(parsed) });
                }
                Err(_) => errors.push("logo_props", "Value must be valid JSON.".to_owned()),
            },
            _ => write.logo_props = Some(Some(raw.clone())),
        }
    }

    // `is_epic` (`BooleanField`, default): HTML `""` already dropped by
    // the spec, JSON `""` fails the boolean sets.
    if data_has_file(&data.files, "is_epic") {
        errors.push("is_epic", "Must be a valid boolean.".to_owned());
    } else if let Some(raw) = object.get("is_epic") {
        match raw {
            Value::Null => errors.push("is_epic", "This field may not be null.".to_owned()),
            _ => match drf_bool_of_value(raw) {
                Some(flag) => write.is_epic = Some(flag),
                None => errors.push("is_epic", "Must be a valid boolean.".to_owned()),
            },
        }
    }

    // `level` (`FloatField`, default): HTML `""` already dropped by the
    // spec, JSON `""` fails the number coercion; booleans are 1.0/0.0.
    if data_has_file(&data.files, "level") {
        errors.push("level", "A valid number is required.".to_owned());
    } else if let Some(raw) = object.get("level") {
        match raw {
            Value::Null => errors.push("level", "This field may not be null.".to_owned()),
            _ => match drf_float_of_value(raw) {
                Some(number) => write.level = Some(number),
                None => errors.push("level", "A valid number is required.".to_owned()),
            },
        }
    }

    if errors.is_empty() {
        Ok(write)
    } else {
        Err(errors)
    }
}

// ---- read shape -------------------------------------------------------------

/// The 8 `IssueTypeSerializer` columns in wire order.
#[derive(Debug, Clone)]
struct TypeRow {
    id: Uuid,
    name: String,
    description: String,
    logo_props: Value,
    is_epic: bool,
    is_default: bool,
    is_active: bool,
    level: f64,
}

/// Wire view in `Meta.fields` order: a struct (not a map) so the order is
/// structural. `level` goes through serde `f64` (the labels `sort_order`
/// precedent: `0.0` renders `0.0` on both).
#[derive(Serialize)]
struct TypeView {
    id: String,
    name: String,
    description: String,
    logo_props: Value,
    is_epic: bool,
    is_default: bool,
    is_active: bool,
    level: f64,
}

/// Render one row: a non-finite level is the strict-renderer 500 (the
/// caller performs every write first, as Django does).
fn render_type_row(row: &TypeRow) -> Result<String, Denial> {
    if !row.level.is_finite() {
        return Err(Denial::ServerError);
    }
    let view = TypeView {
        id: row.id.to_string(),
        name: row.name.clone(),
        description: row.description.clone(),
        logo_props: row.logo_props.clone(),
        is_epic: row.is_epic,
        is_default: row.is_default,
        is_active: row.is_active,
        level: row.level,
    };
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

fn type_row_of(row: &sqlx::postgres::PgRow) -> Result<TypeRow, Denial> {
    Ok(TypeRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        name: row.try_get("name").map_err(|_| Denial::ServerError)?,
        description: row
            .try_get("description")
            .map_err(|_| Denial::ServerError)?,
        logo_props: row.try_get("logo_props").map_err(|_| Denial::ServerError)?,
        is_epic: row.try_get("is_epic").map_err(|_| Denial::ServerError)?,
        is_default: row.try_get("is_default").map_err(|_| Denial::ServerError)?,
        is_active: row.try_get("is_active").map_err(|_| Denial::ServerError)?,
        level: row.try_get("level").map_err(|_| Denial::ServerError)?,
    })
}

// ---- handlers ---------------------------------------------------------------

/// `GET .../issue-types/` (`types.py:get`): live linked types ordered by
/// link level then name. The `SELECT DISTINCT` carries the link level
/// exactly as Django's queryset does (the ordering column must appear in
/// the select list); the live-link unique constraint keeps it to one row
/// per type.
pub async fn issue_type_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_entity_safe(&pool, &slug, &project_id, &user_id).await {
        return Ok(response);
    }
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    if let Err(response) = project_workspace_id(&pool, &slug, &project_id).await {
        return Ok(response);
    }
    let rows = sqlx::query(
        r#"SELECT DISTINCT t.id, t.name, t.description, t.logo_props, t.is_epic,
               t.is_default, t.is_active, t.level, pit.level AS link_level
           FROM issue_types t
           JOIN project_issue_types pit ON pit.issue_type_id = t.id
               AND pit.project_id = $1 AND pit.deleted_at IS NULL
           WHERE t.deleted_at IS NULL
           ORDER BY link_level, t.name"#,
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&render_type_row(&type_row_of(row)?)?);
    }
    out.push(']');
    Ok(json_ok(out))
}

/// `POST .../issue-types/` (`types.py:post`): class POST gate, ADMIN
/// decorator, full validation, then `IssueType.save()` (model defaults
/// for absent members, `created_by` from the actor, `updated_by = None`)
/// plus the `ProjectIssueType` link at the live-link count — each
/// statement autocommitted, as Django's view does. The 201 render is the
/// read shape; a non-finite level 500s after the writes (ported bug).
pub async fn issue_type_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_entity_post(&pool, &slug, &project_id, &user_id).await {
        return Ok(response);
    }
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], None).await?;
    let workspace_id = match project_workspace_id(&pool, &slug, &project_id).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };

    let data = match negotiate_input(&headers, &body) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let object = match object_or_errors(&data.value) {
        Ok(object) => object,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    let write = match validate_type_fields(&data, object) {
        Ok(write) => write,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };

    let now = Utc::now();
    let id = Uuid::new_v4();
    let name = write.name.clone().expect("validated name");
    let description = write.description.clone().unwrap_or_default();
    let logo_bind: Option<Value> = match write.logo_props.clone() {
        None => Some(Value::Object(Map::new())),
        Some(value) => value,
    };
    let is_epic = write.is_epic.unwrap_or(false);
    let level = write.level.unwrap_or(0.0);
    let inserted = sqlx::query(
        r#"INSERT INTO issue_types (id, created_at, updated_at, created_by_id, updated_by_id,
               deleted_at, workspace_id, name, description, logo_props, is_epic, is_default,
               is_active, level, external_source, external_id)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, FALSE, TRUE, $10, NULL, NULL)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&logo_bind)
    .bind(is_epic)
    .bind(level)
    .execute(&pool)
    .await;
    match inserted {
        Ok(row) => row,
        Err(error) if is_integrity_violation(&error) => {
            return Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY));
        }
        Err(_) => return Err(Denial::ServerError),
    };
    let live: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_issue_types WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let link = sqlx::query(
        r#"INSERT INTO project_issue_types (id, created_at, updated_at, created_by_id,
               updated_by_id, deleted_at, project_id, workspace_id, issue_type_id, level,
               is_default)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, FALSE)"#,
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(id)
    .bind(live.0 as i32)
    .execute(&pool)
    .await;
    match link {
        Ok(_) => {
            // The 201 renders the validated inputs — not a re-read — so a
            // multi-key logo keeps its submission order, as Django's
            // in-memory instance does (probed); a later GET shows PG order.
            let created = TypeRow {
                id,
                name,
                description,
                logo_props: logo_bind.unwrap_or(Value::Object(Map::new())),
                is_epic,
                is_default: false,
                is_active: true,
                level,
            };
            render_type_row(&created).map(json_created)
        }
        Err(error) if is_integrity_violation(&error) => {
            Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY))
        }
        Err(_) => Err(Denial::ServerError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_data(text: &str) -> RequestData {
        RequestData {
            value: serde_json::from_str(text).expect("test json"),
            files: BTreeMap::new(),
            is_html: false,
        }
    }

    fn html_data(pairs: &[(&str, &str)]) -> RequestData {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), Value::String((*value).to_owned()));
        }
        RequestData {
            value: Value::Object(map),
            files: BTreeMap::new(),
            is_html: true,
        }
    }

    fn check(text: &str) -> Result<TypeWrite, String> {
        let data = json_data(text);
        match object_or_errors(&data.value) {
            Ok(object) => validate_type_fields(&data, object).map_err(|errors| errors.body()),
            Err(errors) => Err(errors.body()),
        }
    }

    #[test]
    fn name_arms() {
        assert_eq!(
            check(r#"{}"#).unwrap_err(),
            r#"{"name":["This field is required."]}"#
        );
        assert_eq!(
            check(r#"{"name":null}"#).unwrap_err(),
            r#"{"name":["This field may not be null."]}"#
        );
        // Whitespace-only reports blank, never the dead validate_name text.
        for raw in [r#"{"name":""}"#, r#"{"name":"   "}"#] {
            assert_eq!(
                check(raw).unwrap_err(),
                r#"{"name":["This field may not be blank."]}"#
            );
        }
        for raw in [
            r#"{"name":true}"#,
            r#"{"name":["x"]}"#,
            r#"{"name":{"a":1}}"#,
        ] {
            assert_eq!(
                check(raw).unwrap_err(),
                r#"{"name":["Not a valid string."]}"#
            );
        }
        assert_eq!(
            check(&format!(r#"{{"name":"{}"}}"#, "x".repeat(256))).unwrap_err(),
            r#"{"name":["Ensure this field has no more than 255 characters."]}"#
        );
        // Numerics stringify, surrounding whitespace trims, length counts
        // the stripped value.
        let write = check(r#"{"name":123}"#).expect("int name");
        assert_eq!(write.name.as_deref(), Some("123"));
        let write = check(r#"{"name":12.5}"#).expect("float name");
        assert_eq!(write.name.as_deref(), Some("12.5"));
        let write = check(r#"{"name":"  Sp  "}"#).expect("trimmed name");
        assert_eq!(write.name.as_deref(), Some("Sp"));
        let padded = format!("  {}  ", "x".repeat(254));
        let write = check(&format!(r#"{{"name":"{padded}"}}"#)).expect("stripped length");
        assert_eq!(write.name.as_deref(), Some("x".repeat(254).as_str()));
        // NUL trails max-length in field order.
        assert_eq!(
            check("{\"name\":\"a\\u0000b\"}").unwrap_err(),
            r#"{"name":["Null characters are not allowed."]}"#
        );
        let nul_long = format!("{}\\u0000", "y".repeat(300));
        assert_eq!(
            check(&format!("{{\"name\":\"{nul_long}\"}}")).unwrap_err(),
            r#"{"name":["Ensure this field has no more than 255 characters.","Null characters are not allowed."]}"#
        );
    }

    #[test]
    fn description_arms() {
        let write = check(r#"{"name":"n"}"#).expect("absent description");
        assert_eq!(write.description, None);
        let write = check(r#"{"name":"n","description":""}"#).expect("blank kept");
        assert_eq!(write.description.as_deref(), Some(""));
        let write = check(r#"{"name":"n","description":"  d  "}"#).expect("trimmed");
        assert_eq!(write.description.as_deref(), Some("d"));
        let write = check(r#"{"name":"n","description":5}"#).expect("int coerced");
        assert_eq!(write.description.as_deref(), Some("5"));
        assert_eq!(
            check(r#"{"name":"n","description":null}"#).unwrap_err(),
            r#"{"description":["This field may not be null."]}"#
        );
        assert_eq!(
            check(r#"{"name":"n","description":true}"#).unwrap_err(),
            r#"{"description":["Not a valid string."]}"#
        );
        assert_eq!(
            check("{\"name\":\"n\",\"description\":\"x\\u0000\"}").unwrap_err(),
            r#"{"description":["Null characters are not allowed."]}"#
        );
    }

    #[test]
    fn logo_props_arms() {
        let write = check(r#"{"name":"n"}"#).expect("absent logo");
        assert_eq!(write.logo_props, None);
        // JSON values pass through unparsed, even strings.
        for (raw, expected) in [
            (r#""abc""#, Value::String("abc".to_owned())),
            (r#""""#, Value::String(String::new())),
            ("7", Value::from(7)),
            ("true", Value::Bool(true)),
            (r#"[1,"a"]"#, serde_json::json!([1, "a"])),
            (r#""{\"a\":1}""#, Value::String("{\"a\":1}".to_owned())),
        ] {
            let write = check(&format!(r#"{{"name":"n","logo_props":{raw}}}"#))
                .unwrap_or_else(|_| panic!("logo {raw}"));
            assert_eq!(write.logo_props, Some(Some(expected)));
        }
        assert_eq!(
            check(r#"{"name":"n","logo_props":null}"#).unwrap_err(),
            r#"{"logo_props":["This field may not be null."]}"#
        );
        // HTML strings parse: valid JSON stores parsed, "null" stores SQL
        // NULL, anything else (including "") fails.
        let data = html_data(&[("name", "n"), ("logo_props", r#"{"a":1}"#)]);
        let object = object_or_errors(&data.value).expect("html object");
        let write = validate_type_fields(&data, object).expect("html logo parsed");
        assert_eq!(write.logo_props, Some(Some(serde_json::json!({"a": 1}))));
        let data = html_data(&[("name", "n"), ("logo_props", "null")]);
        let object = object_or_errors(&data.value).expect("html object");
        let write = validate_type_fields(&data, object).expect("html null logo");
        assert_eq!(write.logo_props, Some(None));
        for raw in ["{bad", ""] {
            let data = html_data(&[("name", "n"), ("logo_props", raw)]);
            let object = object_or_errors(&data.value).expect("html object");
            assert_eq!(
                validate_type_fields(&data, object).unwrap_err().body(),
                r#"{"logo_props":["Value must be valid JSON."]}"#
            );
        }
    }

    #[test]
    fn boolean_and_float_arms() {
        let write = check(r#"{"name":"n"}"#).expect("defaults absent");
        assert_eq!((write.is_epic, write.level), (None, None));
        for (raw, expected) in [
            ("true", true),
            ("false", false),
            (r#""yes""#, true),
            ("1", true),
        ] {
            let write = check(&format!(r#"{{"name":"n","is_epic":{raw}}}"#))
                .unwrap_or_else(|_| panic!("epic {raw}"));
            assert_eq!(write.is_epic, Some(expected));
        }
        let write = check(r#"{"name":"n","is_epic":0.0}"#).expect("epic 0.0");
        assert_eq!(write.is_epic, Some(false));
        for raw in [r#""""#, "null", r#""bogus""#] {
            let err = check(&format!(r#"{{"name":"n","is_epic":{raw}}}"#)).unwrap_err();
            let message = if raw == "null" {
                "This field may not be null."
            } else {
                "Must be a valid boolean."
            };
            assert_eq!(err, format!(r#"{{"is_epic":["{message}"]}}"#));
        }
        let write = check(r#"{"name":"n","level":"2.5"}"#).expect("level string");
        assert_eq!(write.level, Some(2.5));
        let write = check(r#"{"name":"n","level":true}"#).expect("level bool");
        assert_eq!(write.level, Some(1.0));
        let write = check(r#"{"name":"n","level":"1_0"}"#).expect("level underscores");
        assert_eq!(write.level, Some(10.0));
        for raw in [r#""""#, "null", "[1]"] {
            let err = check(&format!(r#"{{"name":"n","level":{raw}}}"#)).unwrap_err();
            let message = if raw == "null" {
                "This field may not be null."
            } else {
                "A valid number is required."
            };
            assert_eq!(err, format!(r#"{{"level":["{message}"]}}"#));
        }
        // HTML "" is pre-dropped by the spec: absent here reads defaults.
        let data = html_data(&[("name", "n")]);
        let object = object_or_errors(&data.value).expect("html object");
        let write = validate_type_fields(&data, object).expect("html defaults");
        assert_eq!((write.is_epic, write.level), (None, None));
    }

    #[test]
    fn top_level_and_accumulation() {
        let data = json_data("null");
        assert_eq!(
            object_or_errors(&data.value).unwrap_err().body(),
            r#"{"non_field_errors":["No data provided"]}"#
        );
        let data = json_data("[1,2]");
        assert_eq!(
            object_or_errors(&data.value).unwrap_err().body(),
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#
        );
        // Errors accumulate in Meta.fields order; unknown + read-only keys
        // are dropped.
        assert_eq!(
            check(r#"{"zzz":1,"is_default":true,"level":"","is_epic":"bogus"}"#).unwrap_err(),
            r#"{"name":["This field is required."],"is_epic":["Must be a valid boolean."],"level":["A valid number is required."]}"#
        );
    }

    #[test]
    fn render_row_bytes() {
        let row = TypeRow {
            id: Uuid::nil(),
            name: "Bug".to_owned(),
            description: "A defect".to_owned(),
            logo_props: serde_json::json!({"icon": "bug"}),
            is_epic: false,
            is_default: false,
            is_active: true,
            level: 3.5,
        };
        assert_eq!(
            render_type_row(&row).expect("render"),
            r#"{"id":"00000000-0000-0000-0000-000000000000","name":"Bug","description":"A defect","logo_props":{"icon":"bug"},"is_epic":false,"is_default":false,"is_active":true,"level":3.5}"#
        );
        let zero = TypeRow {
            level: 0.0,
            ..row.clone()
        };
        assert!(render_type_row(&zero)
            .expect("render")
            .ends_with(r#""level":0.0}"#));
        let nan = TypeRow {
            level: f64::NAN,
            ..row
        };
        assert!(render_type_row(&nan).is_err());
    }

    #[test]
    fn py_blank_matches_strip_set() {
        assert!(is_py_blank("  \t\r\n\u{1c}\u{1f}  "));
        assert!(!is_py_blank("  x  "));
        assert!(!is_py_blank("\0"));
    }
}
