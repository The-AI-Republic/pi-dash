#![forbid(unsafe_code)]

//! Project issue-label + attachment handlers (D-26 handlers E).
//!
//! Ports `app/views/issue/label.py:23-117` (`LabelViewSet`, list / create /
//! retrieve / update / partial_update / destroy, plus
//! `BulkCreateIssueLabelsEndpoint`) and `app/views/issue/attachment.py:31-229`
//! (`IssueAttachmentEndpoint` v1 multipart CRUD, `IssueAttachmentV2Endpoint`
//! assets/v2 presigned flows) onto the foundation crates. Fixtures:
//! `FX-ISS-18.labels_attachments.json` (family behavior) +
//! `FX-ISS-21.signals_tasks.json` (enqueue goldens); contract suites
//! `test_labels.py` + `test_attachments.py`.
//!
//! Owned paths (registration lives in [`super::routes`]; every other method
//! on these paths proxies to Django, which also owns the `TypeError` 500
//! arms — `GET`/`POST` on the v1 detail tail, `DELETE` on the v1 collection
//! tail, `POST` on the v2 detail tail, `PATCH`/`DELETE` on the v2 collection
//! tail — plus the DRF 405s and `OPTIONS` metadata):
//!
//! * `GET`/`POST .../issue-labels/` and `GET`/`PUT`/`PATCH`/`DELETE`
//!   `.../issue-labels/{pk}/`
//! * `POST .../bulk-create-labels/`
//! * `GET`/`POST .../issues/{issue_id}/issue-attachments/` and `DELETE`
//!   `.../issue-attachments/{pk}/`
//! * `GET`/`POST .../assets/v2/.../issues/{issue_id}/attachments/` and
//!   `GET`/`PATCH`/`DELETE .../attachments/{pk}/`
//!
//! Layering: label read shapes + `validate_name` come from
//! `pidash_services::app_issues::serializers_label` (PIDASHCONV-710); label /
//! asset column consts from `pidash_db::app_issues::{models_core, models_links}`;
//! the `asset_url` property from `pidash_db::app_assets::columns`; body
//! negotiation + CPython JSON errors from the shared `v1_cycles_modules`
//! ports (the `app_workspace` precedent); cache invalidation through
//! `pidash_db::redis::RedisHandle`; task publishes as best-effort
//! `rust_job_queue` rows (the space-intake precedent). The full 25-key app
//! `IssueAttachmentSerializer` read shape (`:867-882`) is inlined here: the
//! merged space `issue_graph` port covers the 24-key *space* class only (no
//! declared `asset_url`), and this family is its sole consumer.
//!
//! DRF bytes below were pinned on Django 4.2.30 / DRF 3.15.2 probes (see
//! the issue workpad): `asset_url` second after `id`; `CharField`
//! int/float coercion with bool rejection; fancy-quote invalid-UUID bodies;
//! HTML blank-input rules; `{"detail": ...}` exception key; `404 No <Model>
//! matches the given query`; `415` full content-type echo; `HEAD` served via
//! `GET`; empty `204` with no content type; `302` as `text/html`.
//!
//! Ported bugs (also listed in the PR):
//!
//! * v1 `POST` with a valid file always 500s: `S3Storage.__init__` never
//!   calls `super().__init__()`, so `file_overwrite` is missing and every
//!   file save raises `AttributeError` before any row is written.
//! * v1 `DELETE` of an existing row always 500s the same way (`location`
//!   missing in `storage.delete`), before the row delete: the row survives.
//! * Bulk colors use `random.randint(0, 0xFFFFFF + 1)` with an inclusive
//!   upper bound, so `#1000000` (seven hex digits) is possible.
//! * Label create ignores an input `sort_order` whenever the project already
//!   has labels (`save()` unconditionally overwrites with `max + 10000`).
//! * `PUT` update runs `validate_name` with `project_id=None` (DRF's default
//!   `get_serializer_context` carries no `project_id`), so the duplicate
//!   check scans null-project labels instead of this project's.
//! * `partial_update`'s exact-match pre-check and `validate_name`'s iexact
//!   check disagree: `Foo` vs `foo` passes the pre-check, then fails
//!   validation with a differently-shaped body.
//! * `PUT` update carries no `@invalidate_cache` and no `@allow_permission`
//!   (only the class permission gates it); bulk create never invalidates.
//! * `DELETE` on the v1 collection tail / `GET`+`POST` on the v1 detail tail
//!   (and the v2 `POST`-on-detail / `PATCH`+`DELETE`-on-collection twins)
//!   are `TypeError` 500s from unexpected/missing view kwargs (proxied).
//! * Filenames can never trip the 800-char `FileField` limit: Django's
//!   `UploadedFile` truncates every upload name to 255 chars first.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use super::Denial;
use crate::middleware::SessionHandle;
use crate::permissions::DefaultPermissionDenied;
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_data, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

use pidash_db::redis::RedisHandle;
use pidash_services::app_issues::serializers_label::{
    app_label_to_representation, AppLabelRow, LABEL_NAME_CONFLICT_BODY,
    LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL, LABEL_NAME_CONFLICT_PROBE_SQL,
};

// ---- owned paths ------------------------------------------------------------

/// Label collection (`app/urls/issue.py:74-78`).
pub const LABELS_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/issue-labels/";
/// Label detail (`app/urls/issue.py:79-89`).
pub const LABEL_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/";
/// Bulk create (`app/urls/issue.py:91-95`).
pub const BULK_LABELS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-create-labels/";
/// v1 collection (`app/urls/issue.py:149-153`).
pub const V1_ATTACHMENTS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/";
/// v1 detail (`app/urls/issue.py:154-158`).
pub const V1_ATTACHMENT_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/";
/// v2 collection (`app/urls/issue.py:160-164`).
pub const V2_ATTACHMENTS_PATH: &str =
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/";
/// v2 detail (`app/urls/issue.py:165-169`).
pub const V2_ATTACHMENT_PATH: &str =
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/";

/// `PUT` validate arm (`serializers/issue.py:564-575` with `project_id=None`):
/// same iexact probe as [`LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL`]
/// over the null-project scope (DRF's default serializer context carries no
/// `project_id`, so `filter(project_id=None)` renders `IS NULL`).
const LABEL_NAME_CONFLICT_NULL_PROJECT_PROBE_SQL: &str = "SELECT 1 FROM labels WHERE project_id IS NULL AND UPPER(name::text) = UPPER($1) AND deleted_at IS NULL AND NOT (id = $2) LIMIT 1";

/// `partial_update` exact-match pre-check (`label.py:61-65`): case-sensitive
/// `name=` within the project, excluding self, under the default manager.
const LABEL_NAME_PRECHECK_SQL: &str = "SELECT 1 FROM labels WHERE project_id = $1 AND name = $2 AND NOT (id = $3) AND deleted_at IS NULL LIMIT 1";

/// `get_object` miss body (`generics.py` via `get_object_or_404`, probed).
const LABEL_NOT_FOUND_BODY: &str = r#"{"detail":"No Label matches the given query."}"#;
/// v1 delete miss body (`attachment.py:66-70`).
const ATTACHMENT_NOT_FOUND_BODY: &str = r#"{"error":"Issue attachment not found."}"#;
/// v2 invalid-type body (`attachment.py:104-108`).
const INVALID_FILE_TYPE_BODY: &str = r#"{"error":"Invalid file type.","status":false}"#;
/// v2 pending-asset body (`attachment.py:176-180`).
const ASSET_NOT_UPLOADED_BODY: &str = r#"{"error":"The asset is not uploaded.","status":false}"#;
/// Label create `IntegrityError` arm (`label.py:51-55`): any integrity error
/// on the single-row insert, including the validate race and (wrongly, but
/// as coded) an impossible-here FK failure.
const LABEL_DUPLICATE_BODY: &str =
    r#"{"error":"Label with the same name already exists in the project"}"#;
/// `BaseViewSet`/`BaseAPIView.handle_exception` `IntegrityError` arm
/// (`views/base.py:120-124,220-224`): PUT/PATCH saves, bulk create, v2 POST.
const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;

/// `ATTACHMENT_MIME_TYPES` (`settings/common.py:652-...)`: the v2 `type`
/// allowlist, order kept for readability only (`in` is order-free).
const ATTACHMENT_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/webp",
    "image/tiff",
    "image/bmp",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/markdown",
    "application/rtf",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.visio",
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-pixmap",
    "application/vnd.oasis.opendocument.database",
    "audio/mpeg",
    "audio/wav",
    "audio/ogg",
    "audio/midi",
    "audio/x-midi",
    "audio/aac",
    "audio/flac",
    "audio/x-m4a",
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-ms-wmv",
    "application/zip",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-zip",
    "application/x-zip-compressed",
    "application/x-7z-compressed",
    "application/x-compressed",
    "application/x-compressed-tar",
    "application/x-compressed-tar-gz",
    "application/x-compressed-tar-bz2",
    "application/x-compressed-tar-zip",
    "application/x-compressed-tar-7z",
    "application/x-compressed-tar-rar",
    "application/x-compressed-tar-zip",
    "model/gltf-binary",
    "model/gltf+json",
    "application/octet-stream",
    "font/ttf",
    "font/otf",
    "font/woff",
    "font/woff2",
    "text/css",
    "text/javascript",
    "application/json",
    "text/xml",
    "text/csv",
    "application/xml",
    "application/x-sql",
    "application/x-gzip",
    // Duplicated in the source (`common.py` lists it twice); kept: `in` is
    // order- and dup-free, and the const mirrors the source line for line.
    "text/markdown",
];

// ---- app attachment read shape ----------------------------------------------

/// App `IssueAttachmentSerializer` wire keys (`issue.py:867-882`), in DRF
/// wire order: `id`, the declared `asset_url` (`Meta.fields` places declared
/// fields right after the pk — probed), then the `__all__` columns in the
/// space port's probed order (concrete columns, then forward relations).
pub const APP_ATTACHMENT_FIELDS: [&str; 25] = [
    "id",
    "asset_url",
    "created_at",
    "updated_at",
    "deleted_at",
    "attributes",
    "asset",
    "entity_type",
    "entity_identifier",
    "is_deleted",
    "is_archived",
    "external_id",
    "external_source",
    "size",
    "is_uploaded",
    "storage_metadata",
    "created_by",
    "updated_by",
    "user",
    "workspace",
    "draft_issue",
    "project",
    "issue",
    "comment",
    "page",
];

/// A `file_assets` row plus its caller-resolved `asset_url` (a model
/// `@property`, `asset.py:79-100`, needing the workspace slug) and
/// user-timezone datetimes (rendered strings — formatting owns to the DB
/// edge, like the space twin). FK columns are the relation names with UUID
/// strings or `null`.
pub struct AppAttachmentRow {
    pub id: Uuid,
    pub asset_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub attributes: Value,
    pub asset: Option<String>,
    pub entity_type: Option<String>,
    pub entity_identifier: Option<String>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Option<Value>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub user: Option<Uuid>,
    pub workspace: Option<Uuid>,
    pub draft_issue: Option<Uuid>,
    pub project: Option<Uuid>,
    pub issue: Option<Uuid>,
    pub comment: Option<Uuid>,
    pub page: Option<Uuid>,
}

/// App `IssueAttachmentSerializer.to_representation` output, in
/// [`APP_ATTACHMENT_FIELDS`] order. Field-for-field twin of the space
/// `issue_graph` view plus the declared `asset_url` second.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppAttachmentView {
    pub id: String,
    pub asset_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub attributes: Value,
    pub asset: Option<String>,
    pub entity_type: Option<String>,
    pub entity_identifier: Option<String>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Option<Value>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub user: Option<String>,
    pub workspace: Option<String>,
    pub draft_issue: Option<String>,
    pub project: Option<String>,
    pub issue: Option<String>,
    pub comment: Option<String>,
    pub page: Option<String>,
}

/// Port of the app `IssueAttachmentSerializer` read shape
/// (`issue.py:867-882`). UUIDs render as strings (the JSON encoder does
/// that in Python); `asset` is the stored path verbatim with `""` as `null`
/// (`fields.py:1539`, `if not value`); datetimes arrive rendered.
pub fn app_attachment_to_representation(row: &AppAttachmentRow) -> AppAttachmentView {
    let id_string = |id: &Option<Uuid>| id.map(|id| id.to_string());
    AppAttachmentView {
        id: row.id.to_string(),
        asset_url: row.asset_url.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
        deleted_at: row.deleted_at.clone(),
        attributes: row.attributes.clone(),
        asset: row.asset.clone(),
        entity_type: row.entity_type.clone(),
        entity_identifier: row.entity_identifier.clone(),
        is_deleted: row.is_deleted,
        is_archived: row.is_archived,
        external_id: row.external_id.clone(),
        external_source: row.external_source.clone(),
        size: row.size,
        is_uploaded: row.is_uploaded,
        storage_metadata: row.storage_metadata.clone(),
        created_by: id_string(&row.created_by),
        updated_by: id_string(&row.updated_by),
        user: id_string(&row.user),
        workspace: id_string(&row.workspace),
        draft_issue: id_string(&row.draft_issue),
        project: id_string(&row.project),
        issue: id_string(&row.issue),
        comment: id_string(&row.comment),
        page: id_string(&row.page),
    }
}

// ---- ordered serializer errors ----------------------------------------------

/// DRF `ErrorDict` accumulation in field order: serializer errors render one
/// key per failing field, in `Meta.fields` order (probed), each a list of
/// messages. `non_field_errors` collects the top-level shape failures.
#[derive(Debug, Default)]
struct FieldErrors {
    entries: Vec<(String, Vec<String>)>,
}

impl FieldErrors {
    fn push(&mut self, field: &str, message: String) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.0 == field) {
            entry.1.push(message);
        } else {
            self.entries.push((field.to_owned(), vec![message]));
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Exact `{"field": ["msg", ...], ...}` bytes in accumulation order.
    fn body(&self) -> String {
        let mut out = String::from("{");
        for (index, (field, messages)) in self.entries.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&serde_json::to_string(field).expect("field name"));
            out.push_str(":[");
            for (number, message) in messages.iter().enumerate() {
                if number > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(message).expect("message"));
            }
            out.push(']');
        }
        out.push('}');
        out
    }
}

// ---- Python scalar coercions -------------------------------------------------

/// Python `repr(float)` spelling for an `f64`: shortest roundtrip digits
/// (via serde/ryu), then CPython's layout rule — scientific `d[.ddd]e±XX`
/// (two-digit minimum exponent, no bare `.0`) when the normalized decimal
/// exponent is `< -4` or `>= 16`, else fixed with a forced `.0`
/// (`float_repr_style short`, `PyOS_double_to_string 'r'`). Non-finite
/// renders lowercase. Feeds `CharField` int/float coercion, `str()` for
/// f-strings, and the fancy-quote UUID bodies.
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let text = serde_json::to_string(&value).expect("finite float");
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (mantissa, exponent): (&str, i32) = match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse().expect("ryu exponent")),
        None => (text, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int, frac)) => (int, frac),
        None => (mantissa, ""),
    };
    let digits: String = format!("{int_part}{frac_part}");
    let point = int_part.len() as i32;
    let Some(first) = digits.find(|c| c != '0') else {
        return if negative { "-0.0" } else { "0.0" }.to_owned();
    };
    // Normalized exponent: d.ddd × 10^power.
    let power = point - first as i32 - 1 + exponent;
    let significant = &digits[first..];
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..16).contains(&power) {
        out.push_str(&significant[..1]);
        let rest = significant[1..].trim_end_matches('0');
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        if power < 0 {
            out.push('-');
            let digits = (-power).to_string();
            if digits.len() < 2 {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            out.push('+');
            let digits = power.to_string();
            if digits.len() < 2 {
                out.push('0');
            }
            out.push_str(&digits);
        }
        return out;
    }
    if power >= 0 {
        let int_len = power as usize + 1;
        if significant.len() >= int_len {
            out.push_str(&significant[..int_len]);
            let rest = significant[int_len..].trim_end_matches('0');
            out.push('.');
            if rest.is_empty() {
                out.push('0');
            } else {
                out.push_str(rest);
            }
        } else {
            out.push_str(significant);
            out.push_str(&"0".repeat(int_len - significant.len()));
            out.push_str(".0");
        }
        return out;
    }
    out.push_str("0.");
    out.push_str(&"0".repeat((-power - 1) as usize));
    out.push_str(significant.trim_end_matches('0'));
    out
}

/// Python `str()` of a JSON number: ints render decimally (arbitrary
/// precision kept — `-0` becomes `0`), floats via [`py_float_repr`].
fn py_str_of_number(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(int) = number.as_u64() {
        return int.to_string();
    }
    if let Some(float) = number.as_f64() {
        if !number.is_f64() {
            // Arbitrary-precision int past u64: normalize the literal (strip
            // a redundant sign/zeros; `-0…0` is `0`).
            let text = number.to_string();
            let (negative, digits) = match text.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, text.as_str()),
            };
            let digits = digits.trim_start_matches('0');
            if digits.is_empty() {
                return "0".to_owned();
            }
            if negative {
                return format!("-{digits}");
            }
            return digits.to_owned();
        }
        return py_float_repr(float);
    }
    number.to_string()
}

/// Python `str()` of a JSON scalar for f-strings and exact-match lookups:
/// strings verbatim, numbers via [`py_str_of_number`], booleans capitalized.
/// Containers use [`py_repr_value`]; callers handle `null` themselves.
fn py_str_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(py_str_of_number(number)),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

/// Python `repr()` of a JSON value (single quotes, `True`/`False`/`None`,
/// `", "` / `": "` separators) for f-string interpolation and the
/// fancy-quote UUID bodies. String escaping covers the realistic shapes
/// (`\\`, `'`, `\n`, `\r`, `\t`); exotic controls render verbatim.
fn py_repr_value(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => py_str_of_number(number),
        Value::String(text) => format!("'{}'", py_repr_string(text)),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, member)| {
                    format!(
                        "{}: {}",
                        py_repr_value(&Value::String(key.clone())),
                        py_repr_value(member)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn py_repr_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// `uuid.UUID(value)` acceptance for a JSON scalar, mirroring
/// `UUIDField.to_python` (`fields/__init__.py`): strings go through hex
/// parsing (dashes/braces/URN accepted, like `Uuid::parse_str`), ints must
/// fit `u128` (`uuid.UUID(int=…)`; negatives and past-`2**128` fail),
/// everything else fails. Returns the parsed id on success.
fn py_uuid_of_value(value: &Value) -> Option<Uuid> {
    match value {
        Value::String(text) => Uuid::parse_str(text).ok(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return None;
                }
                return Some(Uuid::from_u128(int as u128));
            }
            if let Some(int) = number.as_u64() {
                return Some(Uuid::from_u128(u128::from(int)));
            }
            // Arbitrary-precision literal: all digits and fits u128.
            number.to_string().parse::<u128>().ok().map(Uuid::from_u128)
        }
        _ => None,
    }
}

/// Fancy-quote invalid-UUID body
/// (`UUIDField.error_messages["invalid"]`, `“%(value)s” is not a valid
/// UUID.`): `%(value)s` is `str(value)` — identity for strings, decimal /
/// [`py_float_repr`] for numbers, `True`/`False` for booleans, [`py_repr_value`]
/// for containers (probed).
fn invalid_uuid_message(value: &Value) -> String {
    let rendered = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => py_str_of_number(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr_value(value),
    };
    format!("\u{201c}{rendered}\u{201d} is not a valid UUID.")
}

/// Python `float(text)` for DRF `FloatField` strings: surrounding whitespace
/// stripped, `inf`/`infinity`/`nan` in any case with optional sign, digits
/// with single underscores between digits, then the decimal/exponent grammar.
/// Overflow yields infinity (no error), like CPython.
fn parse_py_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let (sign, rest) = match lower.strip_prefix(['+', '-']) {
        Some(rest) => (lower.starts_with('-'), rest),
        None => (false, lower.as_str()),
    };
    if rest == "inf" || rest == "infinity" {
        return Some(if sign {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if rest == "nan" {
        return Some(f64::NAN);
    }
    // Underscores must sit singly between digits (`float("1__0")` fails).
    let bytes = trimmed.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'_' {
            continue;
        }
        let prev = index.checked_sub(1).and_then(|i| bytes.get(i));
        let next = bytes.get(index + 1);
        let digit = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_digit());
        if !(digit(prev) && digit(next)) {
            return None;
        }
    }
    let clean: String = trimmed.chars().filter(|c| *c != '_').collect();
    clean.parse::<f64>().ok()
}

/// DRF `FloatField.to_internal_value` (`fields.py`): booleans become
/// `1.0`/`0.0` (probed), ints/floats convert, strings go through
/// [`parse_py_float`], containers fail.
fn drf_float_of_value(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) => Some(0.0),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => parse_py_float(text),
        _ => None,
    }
}

/// DRF `BooleanField.to_internal_value` (`fields.py:700-708`): the
/// `TRUE_VALUES` / `FALSE_VALUES` sets over case-folded strings, `1`/`True`
/// and `0`/`0.0`/`False` (`1.0` counts via `==`); unhashables and the rest
/// fail. `None` never reaches here (`validate_empty_values` owns it).
fn drf_bool_of_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_u64() == Some(1) {
                return Some(true);
            }
            if number.as_i64() == Some(0) || number.as_u64() == Some(0) {
                return Some(false);
            }
            match number.as_f64() {
                Some(1.0) => Some(true),
                Some(0.0) => Some(false),
                _ => None,
            }
        }
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Some(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// DRF `CharField.to_internal_value` for the label/attachment text fields
/// (`fields.py`): booleans and containers fail, ints/floats stringify via
/// [`py_str_of_number`], strings pass through for the blank/max checks.
fn drf_str_of_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(py_str_of_number(number)),
        _ => None,
    }
}

// ---- request bodies ----------------------------------------------------------

/// Label HTML-input shape: no list fields; form `sort_order=` behaves as
/// absent (probed `not required and not allow_blank` blank-input rule).
/// `parent=` keeps `""` for the allow-null mapping; `name`/`color` keep `""`
/// for the blank/literal arms.
const LABEL_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &["sort_order"],
};

/// Attachment HTML-input shape: the float/boolean scalars skip blank form
/// input; `attributes`/`storage_metadata` keep `""` (their `get_value`
/// override parses it as JSON and fails); text/FK fields keep `""`.
const ATTACHMENT_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &["size", "is_deleted", "is_archived", "is_uploaded"],
};

/// Bulk/v2 shapes share the label blank rules (no float/bool fields of
/// their own read through the map; `size` is parsed raw, where `""` fails
/// `int()` either way).
const RAW_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// A negotiated body: the JSON/form value plus uploads per key. HTML values
/// are lone strings (spec blank-skips applied); JSON values went through the
/// CPython-error parser.
struct RequestData {
    value: Value,
    files: shared_body::FilesMap,
    is_html: bool,
}

/// The v1 attachment endpoint's `parser_classes =
/// (MultiPartParser, FormParser)`: JSON is a 415 (probed, full content-type
/// echo). Every other family here takes DRF's default parsers.
#[allow(clippy::result_large_err)]
fn negotiate_input(
    headers: &HeaderMap,
    body: &[u8],
    spec: &shared_body::BodySpec,
    json_allowed: bool,
) -> Result<RequestData, Response> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => unsupported_media_type(message),
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message).into_response(),
        shared_body::BodyError::ServerError => Denial::ServerError.into_response(),
    };
    match shared_body::negotiate_body(headers, body, spec).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(RequestData {
            value: Value::Object(Map::new()),
            files: BTreeMap::new(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText(text) => {
            if !json_allowed {
                let content_type = headers
                    .get(header::CONTENT_TYPE)
                    .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                    .unwrap_or_default();
                return Err(unsupported_media_type(format!(
                    "Unsupported media type \"{content_type}\" in request."
                )));
            }
            parse_request_data(text.as_bytes())
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
                })
        }
        shared_body::NegotiatedBody::Form { map, files } => Ok(RequestData {
            value: Value::Object(map),
            files,
            is_html: true,
        }),
    }
}

fn unsupported_media_type(message: String) -> Response {
    let body = format!(
        "{{\"Detail\":{}}}",
        serde_json::to_string(&message).expect("415 body")
    );
    Response::builder()
        .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("415 response")
}

/// `request.data.get(key)` for merged form data: a key carrying any upload
/// reads the LAST file (`QueryDict` last-wins over texts-then-files);
/// otherwise the map member. `str(UploadedFile)` is its filename.
fn data_has_file(files: &shared_body::FilesMap, key: &str) -> bool {
    files.get(key).is_some_and(|parts| !parts.is_empty())
}

fn data_file_name<'a>(files: &'a shared_body::FilesMap, key: &str) -> Option<&'a str> {
    files
        .get(key)
        .and_then(|parts| parts.last())
        .map(|part| part.filename.as_str())
}

// ---- label validation ---------------------------------------------------------

/// Validated label write: `None` members are absent (create fills model
/// defaults; partial leaves the column alone); `parent == Some(None)` sets
/// SQL `NULL` (probed: `""` and JSON `null` both validate to `None`).
#[derive(Debug, Default)]
struct LabelWrite {
    parent: Option<Option<Uuid>>,
    name: Option<String>,
    color: Option<String>,
    sort_order: Option<f64>,
}

/// Pure half of `LabelSerializer` validation (`issue.py:549-575`): field
/// errors in `Meta.fields` order over `{parent, name, color, sort_order}`;
/// read-only + unknown keys silently dropped (probed). `partial` skips the
/// required checks. DB-backed checks (parent existence, `validate_name`)
/// run in the caller so the probe SQL stays with the query text.
fn validate_label_fields(
    data: &RequestData,
    object: &Map<String, Value>,
    partial: bool,
) -> Result<LabelWrite, FieldErrors> {
    let mut errors = FieldErrors::default();
    let mut write = LabelWrite::default();

    // `parent` (self-FK, null/blank): `""`/`null` validate to None (probed).
    if data_has_file(&data.files, "parent") {
        let filename = data_file_name(&data.files, "parent").unwrap_or_default();
        errors.push(
            "parent",
            format!("\u{201c}{filename}\u{201d} is not a valid UUID."),
        );
    } else if let Some(raw) = object.get("parent") {
        match raw {
            Value::Null => write.parent = Some(None),
            Value::String(text) if text.is_empty() => write.parent = Some(None),
            Value::Bool(_) => errors.push(
                "parent",
                "Incorrect type. Expected pk value, received bool.".to_owned(),
            ),
            Value::String(_) | Value::Number(_) => match py_uuid_of_value(raw) {
                Some(id) => write.parent = Some(Some(id)),
                None => errors.push("parent", invalid_uuid_message(raw)),
            },
            Value::Array(_) | Value::Object(_) => {
                errors.push("parent", invalid_uuid_message(raw));
            }
        }
    }

    // `name` (`CharField(255)`, required, no blank).
    if data_has_file(&data.files, "name") {
        errors.push("name", "Not a valid string.".to_owned());
    } else if let Some(raw) = object.get("name") {
        match raw {
            Value::Null => errors.push("name", "This field may not be null.".to_owned()),
            Value::String(text) if text.is_empty() => {
                errors.push("name", "This field may not be blank.".to_owned());
            }
            _ => match drf_str_of_value(raw) {
                Some(text) if text.chars().count() > 255 => errors.push(
                    "name",
                    "Ensure this field has no more than 255 characters.".to_owned(),
                ),
                Some(text) => write.name = Some(text),
                None => errors.push("name", "Not a valid string.".to_owned()),
            },
        }
    } else if !partial {
        errors.push("name", "This field is required.".to_owned());
    }

    // `color` (`CharField(255)`, blank: optional everywhere, `""` kept).
    if data_has_file(&data.files, "color") {
        errors.push("color", "Not a valid string.".to_owned());
    } else if let Some(raw) = object.get("color") {
        match raw {
            Value::Null => errors.push("color", "This field may not be null.".to_owned()),
            _ => match drf_str_of_value(raw) {
                Some(text) if text.chars().count() > 255 => errors.push(
                    "color",
                    "Ensure this field has no more than 255 characters.".to_owned(),
                ),
                Some(text) => write.color = Some(text),
                None => errors.push("color", "Not a valid string.".to_owned()),
            },
        }
    }

    // `sort_order` (`FloatField`, default: optional; HTML `""` already
    // dropped by the spec, JSON `""` fails the number coercion).
    if data_has_file(&data.files, "sort_order") {
        errors.push("sort_order", "A valid number is required.".to_owned());
    } else if let Some(raw) = object.get("sort_order") {
        match raw {
            Value::Null => errors.push("sort_order", "This field may not be null.".to_owned()),
            _ => match drf_float_of_value(raw) {
                Some(number) => write.sort_order = Some(number),
                None => errors.push("sort_order", "A valid number is required.".to_owned()),
            },
        }
    }

    if errors.is_empty() {
        Ok(write)
    } else {
        Err(errors)
    }
}

/// Top-level `request.data` shape check (`serializers.py:340`,
/// `to_internal_value`): `null` is `No data provided`, anything else
/// non-dict names its Python type (probed).
fn object_or_errors(value: &Value) -> Result<&Map<String, Value>, FieldErrors> {
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => {
            let mut errors = FieldErrors::default();
            errors.push("non_field_errors", "No data provided".to_owned());
            Err(errors)
        }
        other => {
            let datatype = match other {
                Value::Bool(_) => "bool",
                Value::Number(number) if number.is_f64() => "float",
                Value::Number(_) => "int",
                Value::String(_) => "str",
                Value::Array(_) => "list",
                Value::Object(_) | Value::Null => unreachable!("matched above"),
            };
            let mut errors = FieldErrors::default();
            errors.push(
                "non_field_errors",
                format!("Invalid data. Expected a dictionary, but got {datatype}."),
            );
            Err(errors)
        }
    }
}

// ---- attachment validation (v1 write subset) ----------------------------------

/// FK targets for the v1 write validation: the queryset each
/// `PrimaryKeyRelatedField` checks (`Target._default_manager`). Only `users`
/// is unscoped (plain `UserManager`); the rest inherit
/// `SoftDeletionManager` from `BaseModel`.
const ATTACHMENT_FK_TABLES: &[(&str, &str, bool)] = &[
    ("user", "users", false),
    ("draft_issue", "draft_issues", true),
    ("comment", "issue_comments", true),
    ("page", "pages", true),
];

/// Pure half of `IssueAttachmentSerializer` write validation
/// (`issue.py:867-882`) for the v1 `POST`: field errors in wire order over
/// the writable subset. Read-only and unknown keys are silently dropped.
/// Only the error/success verdict escapes: a valid body always hits the
/// broken-storage 500, so no write struct is built. FK existence checks run
/// in the caller.
fn validate_attachment_fields(data: &RequestData, object: &Map<String, Value>) -> FieldErrors {
    let mut errors = FieldErrors::default();

    // `attributes` (`JSONField`, default, no null): JSON values pass
    // through (strings are valid primitives); HTML values parse as JSON.
    if data_has_file(&data.files, "attributes") {
        errors.push("attributes", "Value must be valid JSON.".to_owned());
    } else if let Some(raw) = object.get("attributes") {
        match raw {
            Value::Null => errors.push("attributes", "This field may not be null.".to_owned()),
            Value::String(text) if data.is_html && serde_json::from_str::<Value>(text).is_err() => {
                errors.push("attributes", "Value must be valid JSON.".to_owned());
            }
            _ => {}
        }
    }

    // `asset` (`FileField`, required): the last upload wins; names never
    // trip `max_length` (Django truncates uploads to 255 first).
    if let Some(parts) = data.files.get("asset") {
        let empty = parts.last().is_some_and(|part| part.bytes.is_empty());
        if empty {
            errors.push("asset", "The submitted file is empty.".to_owned());
        }
    } else if let Some(raw) = object.get("asset") {
        match raw {
            Value::Null => errors.push("asset", "This field may not be null.".to_owned()),
            _ => errors.push(
                "asset",
                "The submitted data was not a file. Check the encoding type on the form."
                    .to_owned(),
            ),
        }
    } else {
        errors.push("asset", "No file was submitted.".to_owned());
    }

    // Text fields (`CharField(255)`, null + blank): optional, `""` kept.
    for field in [
        "entity_type",
        "entity_identifier",
        "external_id",
        "external_source",
    ] {
        if data_has_file(&data.files, field) {
            errors.push(field, "Not a valid string.".to_owned());
        } else if let Some(raw) = object.get(field) {
            match raw {
                Value::Null => {}
                _ => match drf_str_of_value(raw) {
                    Some(text) if text.chars().count() > 255 => errors.push(
                        field,
                        "Ensure this field has no more than 255 characters.".to_owned(),
                    ),
                    Some(_) => {}
                    None => errors.push(field, "Not a valid string.".to_owned()),
                },
            }
        }
    }

    // Boolean fields (defaults, no null; HTML `""` already spec-dropped).
    for field in ["is_deleted", "is_archived", "is_uploaded"] {
        if data_has_file(&data.files, field) {
            errors.push(field, "Must be a valid boolean.".to_owned());
        } else if let Some(raw) = object.get(field) {
            match raw {
                Value::Null => errors.push(field, "This field may not be null.".to_owned()),
                _ => {
                    if drf_bool_of_value(raw).is_none() {
                        errors.push(field, "Must be a valid boolean.".to_owned());
                    }
                }
            }
        }
    }

    // `size` (`FloatField`, default, no null; HTML `""` spec-dropped).
    if data_has_file(&data.files, "size") {
        errors.push("size", "A valid number is required.".to_owned());
    } else if let Some(raw) = object.get("size") {
        match raw {
            Value::Null => errors.push("size", "This field may not be null.".to_owned()),
            _ => {
                if drf_float_of_value(raw).is_none() {
                    errors.push("size", "A valid number is required.".to_owned());
                }
            }
        }
    }

    // `storage_metadata` (`JSONField`, default, null): like `attributes`
    // but nullable.
    if data_has_file(&data.files, "storage_metadata") {
        errors.push("storage_metadata", "Value must be valid JSON.".to_owned());
    } else if let Some(raw) = object.get("storage_metadata") {
        match raw {
            Value::Null => {}
            Value::String(text) if data.is_html && serde_json::from_str::<Value>(text).is_err() => {
                errors.push("storage_metadata", "Value must be valid JSON.".to_owned());
            }
            _ => {}
        }
    }

    // Nullable FKs: `""`/`null` validate to `None`; UUID parsing here,
    // existence in the caller.
    for (field, _, _) in ATTACHMENT_FK_TABLES {
        if data_has_file(&data.files, field) {
            let filename = data_file_name(&data.files, field).unwrap_or_default();
            errors.push(
                field,
                format!("\u{201c}{filename}\u{201d} is not a valid UUID."),
            );
        } else if let Some(raw) = object.get(*field) {
            match raw {
                Value::Null => {}
                Value::String(text) if text.is_empty() => {}
                Value::Bool(_) => errors.push(
                    field,
                    "Incorrect type. Expected pk value, received bool.".to_owned(),
                ),
                Value::String(_) | Value::Number(_) => {
                    if py_uuid_of_value(raw).is_none() {
                        errors.push(field, invalid_uuid_message(raw));
                    }
                }
                Value::Array(_) | Value::Object(_) => {
                    errors.push(field, invalid_uuid_message(raw));
                }
            }
        }
    }

    errors
}

// ---- gates -------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`), mirroring
/// `super`'s actor resolution: missing snapshots and non-UUID ids are
/// anonymous, rejected 401 before anything else (the rewrite stays closed).
fn actor_user_id(extension: Option<axum::Extension<SessionHandle>>) -> Result<Uuid, Denial> {
    let user_id = extension
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
        .ok_or(Denial::Unauthorized)?;
    Ok(user_id)
}

/// `ProjectBasePermission` safe-method arm (`permissions/project.py:18-22`):
/// any active workspace membership; else the DRF-default 403.
#[allow(clippy::result_large_err)]
async fn gate_class_safe(pool: &sqlx::PgPool, slug: &str, user_id: &Uuid) -> Result<(), Response> {
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if member.is_some() {
        Ok(())
    } else {
        Err(DefaultPermissionDenied.into_response())
    }
}

/// `ProjectBasePermission` POST arm (`:25-31`): an active workspace
/// ADMIN/MEMBER membership; guests fail here with the DRF-default 403.
#[allow(clippy::result_large_err)]
async fn gate_class_post(pool: &sqlx::PgPool, slug: &str, user_id: &Uuid) -> Result<(), Response> {
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role IN (20, 15)
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if member.is_some() {
        Ok(())
    } else {
        Err(DefaultPermissionDenied.into_response())
    }
}

/// `ProjectBasePermission` unsafe-method arm (`:33-53`): a project ADMIN
/// membership, or any project membership plus a workspace ADMIN membership;
/// else the DRF-default 403.
#[allow(clippy::result_large_err)]
async fn gate_class_unsafe(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Response> {
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
           AND pm.role = 20 AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if admin.is_some() {
        return Ok(());
    }
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
    let ws_admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if member.is_some() && ws_admin.is_some() {
        Ok(())
    } else {
        Err(DefaultPermissionDenied.into_response())
    }
}

/// `@allow_permission(roles)` at `PROJECT` level
/// (`permissions/base.py:19-88`): a role match, else the member +
/// workspace-admin bypass; failures answer the allow-style 403. With
/// `creator_pk`, the creator arm runs first: workspace membership is
/// required, then a `created_by = user` row bypasses the role gate
/// entirely (no workspace/project scoping on that check, as coded).
async fn gate_decorator(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    roles: &[i16],
    creator_pk: Option<Uuid>,
) -> Result<(), Denial> {
    if let Some(pk) = creator_pk {
        let member: Option<(i32,)> = sqlx::query_as(
            r#"SELECT 1 FROM workspace_members wm
               JOIN workspaces w ON w.id = wm.workspace_id
               WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
        )
        .bind(user_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if member.is_none() {
            return Err(Denial::Forbidden);
        }
        let created: Option<(i32,)> = sqlx::query_as(
            r#"SELECT 1 FROM file_assets WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
        )
        .bind(pk)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if created.is_some() {
            return Ok(());
        }
    }
    let role: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND w.slug = $2 AND pm.project_id = $3
           AND pm.role = ANY($4) AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(project_id)
    .bind(roles)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if role.is_some() {
        return Ok(());
    }
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND w.slug = $2 AND pm.project_id = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if member.is_some() && admin.is_some() {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// The actor's `user_timezone` for attachment datetime rendering
/// (`TimezoneMixin.initial` activates it per request). A missing row or an
/// unparsable zone is a 500, like the list family's tenant context.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse::<Tz>().map_err(|_| Denial::ServerError)
}

/// `Workspace.objects.get(slug=slug)`: 404 `{"error": ...}` on a miss
/// (default-manager scoped).
async fn workspace_row(pool: &sqlx::PgPool, slug: &str) -> Result<(Uuid, String), Denial> {
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, slug FROM workspaces WHERE slug = $1 AND deleted_at IS NULL")
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

/// `Project.objects.get(pk=...)`: 404 `{"error": ...}` on a miss
/// (default-manager scoped).
async fn project_row(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<(Uuid, Uuid), Denial> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

// ---- cache invalidation -------------------------------------------------------

/// `@invalidate_cache(path="/api/workspaces/:slug/labels/", url_params=True,
/// user=False, ...)` (`utils/cache.py:72-88`, `label.py:42,57,84`): runs
/// before the permission decorator and the view body. `multiple=True`
/// (create) is `KEYS :1:*{key}*` + `DEL`; otherwise a single-key `DEL` of the
/// django-redis key (`:1:` prefix, version 1 — verified against the pinned
/// `django-redis==5.4.0` plus Django's default key func; no overrides in
/// settings). Any Redis failure — including a missing handle, mirroring
/// Django's empty-`LOCATION` client — is the 500 the suite pins.
fn labels_cache_key(slug: &str) -> String {
    format!("/api/workspaces/{slug}/labels/")
}

/// The `KEYS` pattern per invalidate arm: `:1:*{key}*` for create
/// (`multiple=True`), the exact django-redis key (`:1:` prefix, version 1)
/// for the single-key arms.
fn labels_cache_pattern(slug: &str, multiple: bool) -> String {
    let key = labels_cache_key(slug);
    // `django_redis.keys()` routes the search through `make_pattern`
    // (`:1:` + the raw glob; the user pattern itself is unescaped).
    if multiple {
        format!(":1:*{key}*")
    } else {
        format!(":1:{key}")
    }
}

async fn invalidate_workspace_labels(
    redis: Option<&RedisHandle>,
    slug: &str,
    multiple: bool,
) -> Result<(), Denial> {
    let Some(handle) = redis else {
        return Err(Denial::ServerError);
    };
    handle
        .invalidate_matching(&labels_cache_pattern(slug, multiple))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---- task publishes -----------------------------------------------------------

/// Celery wire name for `issue_activity`
/// (`bgtasks/issue_activities_task.py:1504`).
const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// `base_host(request, is_app=True)` (`utils/host.py:17-66`): `APP_BASE_URL`
/// wins when set, else `WEB_URL`; unset is `ImproperlyConfigured` → 500.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .app_base_url
        .clone()
        .or_else(|| state.settings().urls.web_url.clone())
        .ok_or(Denial::ServerError)
}

/// One `issue_activity.delay(...)` enqueue (`attachment.py:47-57,73-83,
/// 155-165,209-219`): the nine kwargs exactly as the view passes them
/// (`subscriber` keeps its default and stays out of the payload).
#[allow(clippy::too_many_arguments)]
fn issue_activity_message(
    activity_type: &str,
    current_instance: Option<String>,
    issue_id: &Uuid,
    actor_id: &Uuid,
    project_id: &Uuid,
    epoch: i64,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert("requested_data".to_owned(), Value::Null);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map_or(Value::Null, Value::String),
    );
    kwargs.insert("epoch".to_owned(), Value::from(epoch));
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs)
}

/// `get_asset_object_metadata.delay(str(asset.id))` (`attachment.py:227`):
/// one positional arg, no kwargs, fired only when `storage_metadata` is
/// falsy (`None` or `{}`).
fn asset_metadata_message(asset_id: &Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::space::GET_ASSET_OBJECT_METADATA_TASK,
        vec![Value::String(asset_id.to_string())],
        Map::new(),
    )
}

/// `soft_delete_related_objects.delay("db", "label", pk, using=None)`
/// (`db/mixins.py:77`, via `Label.delete()`): the `None` travels as a null
/// fourth positional (the api-token precedent; the worker parses
/// positional-or-kwarg and reads null as unset).
fn label_soft_delete_message(pk: &Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("label".to_owned()),
            Value::String(pk.to_string()),
            Value::Null,
        ],
        Map::new(),
    )
}

/// Best-effort deferred publish (the space-intake precedent): without the
/// queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

// ---- SigV4 presigning (offline; mirrors `S3Storage` + botocore) ----------------

/// Request scheme for MinIO-mode signing: `X-Forwarded-Proto` when the
/// proxy sets it, else `http` (Django's `request.scheme` default).
fn scheme_of(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| v == "http" || v == "https")
        .unwrap_or_else(|| "http".to_owned())
}

fn host_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// RFC 3986 percent-encoding for SigV4 (unreserved marks stay bare,
/// everything else `%XX` uppercase — botocore's `quote(..., safe='-_.~')`).
fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// `urllib.parse.quote` with default `safe='/'` for the disposition
/// filename (`storage.py:119`): letters, digits, `_.-~/` stay bare.
fn quote_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// SigV4 signing key: `kDate/kRegion/kService/kSigning`
/// (`storage.py` always signs `s3`).
fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn credential_scope(date: &str, region: &str) -> String {
    format!("{date}/{region}/s3/aws4_request")
}

/// Endpoint + host/path split for signing, mirroring
/// `S3Storage.__init__` with a request (`is_server=False`):
/// MinIO mode signs `{scheme}://{Host}` path-style; an explicit
/// endpoint URL signs path-style against it; otherwise the
/// virtual-hosted AWS default.
fn endpoint_parts(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
) -> (String, String) {
    if storage.use_minio {
        (format!("{scheme}://{host}"), host.to_owned())
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/');
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint)
            .split('/')
            .next()
            .unwrap_or(endpoint);
        (endpoint.to_owned(), signed_host.to_owned())
    } else {
        let region = storage.region.as_str();
        let base = if region.is_empty() {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        (
            format!("https://{}.{base}", storage.bucket_name),
            format!("{}.{base}", storage.bucket_name),
        )
    }
}

/// Path encoding for the canonical URI: slashes survive, every
/// segment is RFC 3986-encoded (botocore `quote(path, safe='/~')`
/// with the same unreserved set as [`uri_encode`]).
fn uri_encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// The download `filename` argument: `_get_content_disposition`
/// (`storage.py:115-123`) with `disposition="attachment"`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Filename {
    /// A stored string name: quoted into the disposition.
    Name(String),
    /// Missing/null name: a fresh uuid4 hex per call.
    FreshHex,
    /// Empty name: falsy → the bare disposition, no filename part.
    Bare,
}

fn disposition_value(filename: &Filename) -> String {
    match filename {
        Filename::Name(name) => {
            format!("attachment; filename*=UTF-8''{}", quote_filename(name))
        }
        Filename::FreshHex => {
            format!("attachment; filename*=UTF-8''{}", Uuid::new_v4().simple())
        }
        Filename::Bare => "attachment".to_owned(),
    }
}

/// `generate_presigned_url(object_name, disposition="attachment",
/// filename=...)` (`attachment.py:183-187`, `storage.py:126-153`):
/// presigned GET with the attachment disposition above.
fn presigned_get_url(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    filename: Filename,
    now: &DateTime<Utc>,
) -> String {
    let region = storage.region.as_str();
    let (endpoint, signed_host) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let credential = format!("{}/{}", storage.access_key_id, scope);
    let disposition = disposition_value(&filename);
    let mut params = [
        ("response-content-disposition".to_owned(), disposition),
        ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential".to_owned(), credential),
        ("X-Amz-Date".to_owned(), amz_date),
        (
            "X-Amz-Expires".to_owned(),
            storage.signed_url_expiration_secs.to_string(),
        ),
        ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
    ];
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = params
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let path_style = storage.use_minio
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty());
    let canonical_path = if path_style {
        format!("/{}/{}", storage.bucket_name, uri_encode_path(object_name))
    } else {
        format!("/{}", uri_encode_path(object_name))
    };
    let canonical = format!(
        "GET\n{canonical_path}\n{canonical_query}\nhost:{signed_host}\n\nhost\nUNSIGNED-PAYLOAD"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        params
            .iter()
            .find(|(k, _)| k == "X-Amz-Date")
            .expect("date param")
            .1,
        scope,
        sha256_hex(canonical.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        string_to_sign.as_bytes(),
    ));
    format!("{endpoint}{canonical_path}?{canonical_query}&X-Amz-Signature={signature}")
}

/// `generate_presigned_post(object_name, file_type, file_size)`
/// (`attachment.py:135`, `storage.py:79-113`): `{"url","fields"}` with
/// botocore's field order and the `storage.py` condition order.
fn presigned_post(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    file_type: &str,
    file_size: &str,
    now: &DateTime<Utc>,
) -> Value {
    let region = storage.region.as_str();
    let (endpoint, _) = endpoint_parts(storage, scheme, host);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let expiration = (*now + chrono::Duration::seconds(storage.signed_url_expiration_secs))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    // Condition order mirrors `storage.py`: bucket, content-length
    // range, Content-Type, key — then the three signer conditions.
    // Serialized with CPython `json.dumps` default separators
    // (`, `, `: `, `ensure_ascii`) so the policy bytes — and hence the
    // signature S3 verifies — match botocore's.
    let credential = format!("{}/{}", storage.access_key_id, scope);
    // The client-level `generate_presigned_post` appends its own
    // `{"bucket"}` / `{"key"}` conditions after the caller's and before
    // the signer's (`botocore/signers.py`), so the policy carries nine.
    let conditions = format!(
        "[{{\"bucket\": {}}}, [\"content-length-range\", 1, {}], {{\"Content-Type\": {}}}, {{\"key\": {}}}, {{\"bucket\": {}}}, {{\"key\": {}}}, {{\"x-amz-algorithm\": \"AWS4-HMAC-SHA256\"}}, {{\"x-amz-credential\": {}}}, {{\"x-amz-date\": {}}}]",
        py_json_string(&storage.bucket_name),
        file_size,
        py_json_string(file_type),
        py_json_string(object_name),
        py_json_string(&storage.bucket_name),
        py_json_string(object_name),
        py_json_string(&credential),
        py_json_string(&amz_date),
    );
    let policy_json = format!(
        "{{\"expiration\": {}, \"conditions\": {conditions}}}",
        py_json_string(&expiration),
    );
    let policy_b64 = base64_encode(policy_json.as_bytes());
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        policy_b64.as_bytes(),
    ));
    // `url`: path-style against a custom/MinIO endpoint, otherwise the
    // virtual-hosted bucket root with its trailing slash
    // (`https://uploads.s3.amazonaws.com/`).
    let url = if storage.use_minio
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty())
    {
        format!("{endpoint}/{}", storage.bucket_name)
    } else {
        format!("{endpoint}/")
    };
    let mut fields = Map::with_capacity(7);
    fields.insert(
        "Content-Type".to_owned(),
        Value::String(file_type.to_owned()),
    );
    fields.insert("key".to_owned(), Value::String(object_name.to_owned()));
    fields.insert(
        "x-amz-algorithm".to_owned(),
        Value::String("AWS4-HMAC-SHA256".to_owned()),
    );
    fields.insert("x-amz-credential".to_owned(), Value::String(credential));
    fields.insert("x-amz-date".to_owned(), Value::String(amz_date));
    fields.insert("policy".to_owned(), Value::String(policy_b64));
    fields.insert("x-amz-signature".to_owned(), Value::String(signature));
    serde_json::json!({"url": url, "fields": fields})
}

/// CPython `json.dumps` string encoding (`ensure_ascii`): `"` and
/// `\` escaped, C0 controls short/`\u00XX`, everything else non-ASCII
/// as `\uXXXX` (surrogate pairs past the BMP).
fn py_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn base64_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}

// ---- rows --------------------------------------------------------------------

/// The seven label read columns (`LabelSerializer.Meta.fields`).
#[derive(Debug, Clone)]
struct LabelRow {
    parent_id: Option<Uuid>,
    name: String,
    color: String,
    id: Uuid,
    project_id: Option<Uuid>,
    workspace_id: Uuid,
    sort_order: f64,
}

/// The 23 `file_assets` columns the read shape needs.
#[derive(Debug, Clone)]
struct AssetRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    attributes: Value,
    asset: String,
    user_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    draft_issue_id: Option<Uuid>,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    comment_id: Option<Uuid>,
    page_id: Option<Uuid>,
    entity_type: Option<String>,
    entity_identifier: Option<String>,
    is_deleted: bool,
    is_archived: bool,
    external_id: Option<String>,
    external_source: Option<String>,
    size: f64,
    is_uploaded: bool,
    storage_metadata: Option<Value>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
}

const LABEL_READ_COLUMNS: &str =
    "l.parent_id, l.name, l.color, l.id, l.project_id, l.workspace_id, l.sort_order";

const ASSET_READ_COLUMNS: &str = "fa.id, fa.created_at, fa.updated_at, fa.deleted_at, fa.attributes, fa.asset, fa.user_id, fa.workspace_id, fa.draft_issue_id, fa.project_id, fa.issue_id, fa.comment_id, fa.page_id, fa.entity_type, fa.entity_identifier, fa.is_deleted, fa.is_archived, fa.external_id, fa.external_source, fa.size, fa.is_uploaded, fa.storage_metadata, fa.created_by_id, fa.updated_by_id";

fn label_row_of(row: &sqlx::postgres::PgRow) -> Result<LabelRow, Denial> {
    Ok(LabelRow {
        parent_id: row.try_get("parent_id").map_err(|_| Denial::ServerError)?,
        name: row.try_get("name").map_err(|_| Denial::ServerError)?,
        color: row.try_get("color").map_err(|_| Denial::ServerError)?,
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
        sort_order: row.try_get("sort_order").map_err(|_| Denial::ServerError)?,
    })
}

fn asset_row_of(row: &sqlx::postgres::PgRow) -> Result<AssetRow, Denial> {
    Ok(AssetRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
        attributes: row.try_get("attributes").map_err(|_| Denial::ServerError)?,
        asset: row.try_get("asset").map_err(|_| Denial::ServerError)?,
        user_id: row.try_get("user_id").map_err(|_| Denial::ServerError)?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
        draft_issue_id: row
            .try_get("draft_issue_id")
            .map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        issue_id: row.try_get("issue_id").map_err(|_| Denial::ServerError)?,
        comment_id: row.try_get("comment_id").map_err(|_| Denial::ServerError)?,
        page_id: row.try_get("page_id").map_err(|_| Denial::ServerError)?,
        entity_type: row
            .try_get("entity_type")
            .map_err(|_| Denial::ServerError)?,
        entity_identifier: row
            .try_get("entity_identifier")
            .map_err(|_| Denial::ServerError)?,
        is_deleted: row.try_get("is_deleted").map_err(|_| Denial::ServerError)?,
        is_archived: row
            .try_get("is_archived")
            .map_err(|_| Denial::ServerError)?,
        external_id: row
            .try_get("external_id")
            .map_err(|_| Denial::ServerError)?,
        external_source: row
            .try_get("external_source")
            .map_err(|_| Denial::ServerError)?,
        size: row.try_get("size").map_err(|_| Denial::ServerError)?,
        is_uploaded: row
            .try_get("is_uploaded")
            .map_err(|_| Denial::ServerError)?,
        storage_metadata: row
            .try_get("storage_metadata")
            .map_err(|_| Denial::ServerError)?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by_id: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// Render one label row through the merged app serializer port.
fn render_label(row: &LabelRow) -> Result<String, Denial> {
    let parent = row.parent_id.map(|id| id.to_string());
    let id = row.id.to_string();
    let project_id = row.project_id.map(|id| id.to_string());
    let workspace_id = row.workspace_id.to_string();
    let row_data = AppLabelRow {
        parent: parent.as_deref(),
        name: row.name.as_str(),
        color: row.color.as_str(),
        id: id.as_str(),
        project_id: project_id.as_deref(),
        workspace_id: workspace_id.as_str(),
        sort_order: row.sort_order,
    };
    let view = app_label_to_representation(&row_data);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// Render one asset row: datetimes in the actor's zone, `asset_url` via the
/// merged property port (`None` renders `"None"` for missing ids, as the
/// f-string does), empty `asset` as `null`.
fn render_asset(row: &AssetRow, slug: &str, timezone: &Tz) -> Result<String, Denial> {
    let none = "None".to_owned();
    let id = row.id.to_string();
    let project_id = row.project_id.map(|id| id.to_string());
    let issue_id = row.issue_id.map(|id| id.to_string());
    let asset_url = pidash_db::app_assets::columns::asset_url(
        row.entity_type.as_deref(),
        &id,
        slug,
        project_id.as_deref().unwrap_or(&none),
        issue_id.as_deref().unwrap_or(&none),
    );
    let view = app_attachment_to_representation(&AppAttachmentRow {
        id: row.id,
        asset_url,
        created_at: render_datetime_in(&row.created_at, timezone),
        updated_at: render_datetime_in(&row.updated_at, timezone),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| render_datetime_in(dt, timezone)),
        attributes: row.attributes.clone(),
        asset: if row.asset.is_empty() {
            None
        } else {
            Some(row.asset.clone())
        },
        entity_type: row.entity_type.clone(),
        entity_identifier: row.entity_identifier.clone(),
        is_deleted: row.is_deleted,
        is_archived: row.is_archived,
        external_id: row.external_id.clone(),
        external_source: row.external_source.clone(),
        size: row.size,
        is_uploaded: row.is_uploaded,
        storage_metadata: row.storage_metadata.clone(),
        created_by: row.created_by_id,
        updated_by: row.updated_by_id,
        user: row.user_id,
        workspace: row.workspace_id,
        draft_issue: row.draft_issue_id,
        project: row.project_id,
        issue: row.issue_id,
        comment: row.comment_id,
        page: row.page_id,
    });
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// Whether a `sqlx` failure is an integrity-constraint violation (SQLSTATE
/// class 23: unique / not-null / FK — what Django surfaces as
/// `IntegrityError`).
fn is_integrity_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

// ---- responses -----------------------------------------------------------------

fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

fn json_created(body: String) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("created response")
}

fn json_bad_request(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("bad-request response")
}

fn json_not_found(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("not-found response")
}

fn no_content() -> Response {
    // Probed: DRF's empty 204 carries no content type.
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("no-content response")
}

fn redirect(location: String) -> Response {
    // `HttpResponseRedirect`: 302 + `Location`, `text/html`, empty body.
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Split a proxied-or-read request after the UUID guards passed. The 8MB cap
/// covers the 5MB attachment ceiling plus multipart framing; overflow is a
/// 500 (the serve-wide 413 lives in Django middleware and is unported).
async fn split_parts(
    req: axum::extract::Request,
) -> Result<(axum::http::request::Parts, Vec<u8>), Denial> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 8 * 1024 * 1024)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((parts, bytes.to_vec()))
}

// ---- label reads -----------------------------------------------------------------

/// `GET .../issue-labels/` (`label.py:28-40`): DRF default list over the
/// viewset queryset — workspace + project + any project membership of the
/// actor (no `is_active` filter, as coded), distinct, `sort_order`.
/// Reads take no decorator: safe-method guests list too.
pub async fn label_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_safe(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    let rows = sqlx::query(&format!(
        r#"SELECT DISTINCT {LABEL_READ_COLUMNS} FROM labels l
               JOIN workspaces w ON w.id = l.workspace_id
               WHERE l.project_id = $1 AND w.slug = $2 AND l.deleted_at IS NULL
               AND EXISTS (SELECT 1 FROM project_members pm
                           WHERE pm.project_id = l.project_id AND pm.member_id = $3)
               ORDER BY l.sort_order"#
    ))
    .bind(project_id)
    .bind(&slug)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&render_label(&label_row_of(row)?)?);
    }
    out.push(']');
    Ok(json_ok(out))
}

/// Fetch one label through the viewset queryset + pk (`get_object`).
async fn label_object(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<LabelRow>, Denial> {
    let row = sqlx::query(&format!(
        r#"SELECT {LABEL_READ_COLUMNS} FROM labels l
               JOIN workspaces w ON w.id = l.workspace_id
               WHERE l.id = $1 AND l.project_id = $2 AND w.slug = $3 AND l.deleted_at IS NULL
               AND EXISTS (SELECT 1 FROM project_members pm
                           WHERE pm.project_id = l.project_id AND pm.member_id = $4)"#
    ))
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| label_row_of(&row)).transpose()
}

/// `GET .../issue-labels/{pk}/`: DRF default retrieve (no custom code).
pub async fn label_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // Non-UUID tails never match Django's `<uuid:pk>` converter: proxy.
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_safe(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    match label_object(&pool, &slug, &project_id, &user_id, &pk).await? {
        Some(row) => Ok(json_ok(render_label(&row)?)),
        None => Ok(json_not_found(LABEL_NOT_FOUND_BODY)),
    }
}

// ---- label writes ------------------------------------------------------------------

/// `validate_name` create arm (`issue.py:564-575`): iexact same-name label
/// in this project (non-deleted) → `{"name": ["LABEL_NAME_ALREADY_EXISTS"]}`.
async fn validate_name_create(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    name: &str,
) -> Result<Result<(), String>, Denial> {
    let hit: Option<(i32,)> = sqlx::query_as(LABEL_NAME_CONFLICT_PROBE_SQL)
        .bind(project_id)
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(if hit.is_some() {
        Err(LABEL_NAME_CONFLICT_BODY.to_owned())
    } else {
        Ok(())
    })
}

/// `validate_name` update arm, excluding the instance under edit.
async fn validate_name_update(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    name: &str,
    exclude: &Uuid,
) -> Result<Result<(), String>, Denial> {
    let hit: Option<(i32,)> = sqlx::query_as(LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL)
        .bind(project_id)
        .bind(name)
        .bind(exclude)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(if hit.is_some() {
        Err(LABEL_NAME_CONFLICT_BODY.to_owned())
    } else {
        Ok(())
    })
}

/// `validate_name` `PUT` arm: same probe over the null-project scope (the
/// serializer context carries no `project_id` on the default update path).
async fn validate_name_put(
    pool: &sqlx::PgPool,
    name: &str,
    exclude: &Uuid,
) -> Result<Result<(), String>, Denial> {
    let hit: Option<(i32,)> = sqlx::query_as(LABEL_NAME_CONFLICT_NULL_PROJECT_PROBE_SQL)
        .bind(name)
        .bind(exclude)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(if hit.is_some() {
        Err(LABEL_NAME_CONFLICT_BODY.to_owned())
    } else {
        Ok(())
    })
}

/// Parent existence for a validated parent id: any non-deleted label
/// (`Label.objects`, unscoped by project).
async fn parent_exists(pool: &sqlx::PgPool, parent: &Uuid) -> Result<bool, Denial> {
    let hit: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM labels WHERE id = $1 AND deleted_at IS NULL")
            .bind(parent)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

/// `POST .../issue-labels/` (`label.py:42-55`): class POST gate, cache
/// invalidation, ADMIN decorator, full validation + `validate_name`, then
/// `Label.save()`: `sort_order = max + 10000` whenever the project already
/// has labels (an input `sort_order` is ignored then), `workspace` filled
/// from the project (`WorkspaceBaseModel.save`), `created_by` from the
/// actor and `updated_by = None` (`BaseModel.save`). Any `IntegrityError`
/// (validate race, or the unreachable-here FK arm) answers the custom 400.
pub async fn label_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_post(&pool, &slug, &user_id).await {
        return Ok(response);
    }
    invalidate_workspace_labels(state.redis(), &slug, true).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], None).await?;

    let data = match negotiate_input(&headers, &body, &LABEL_BODY_SPEC, true) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let object = match object_or_errors(&data.value) {
        Ok(object) => object,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    let write = match validate_label_fields(&data, object, false) {
        Ok(write) => write,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    if let Some(Some(parent)) = write.parent {
        if !parent_exists(&pool, &parent).await? {
            let mut errors = FieldErrors::default();
            errors.push(
                "parent",
                format!("Invalid pk \"{parent}\" - object does not exist."),
            );
            return Ok(json_bad_request(&errors.body()));
        }
    }
    let name = write.name.clone().expect("validated name");
    let conflict = validate_name_create(&pool, &project_id, &name).await?;
    if let Err(body) = conflict {
        return Ok(json_bad_request(&body));
    }

    let (_, workspace_id) = project_row(&pool, &project_id).await?;
    let largest: (Option<f64>,) = sqlx::query_as(
        "SELECT MAX(sort_order) FROM labels WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sort_order = pidash_db::app_issues::models_core::label::new_sort_order(largest.0)
        .unwrap_or_else(|| write.sort_order.unwrap_or(65535.0));
    let now = Utc::now();
    let id = Uuid::new_v4();
    let color = write.color.clone().unwrap_or_default();
    let inserted = sqlx::query(
        r#"INSERT INTO labels (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, parent_id, name, description, color, sort_order,
            external_source, external_id)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, '', $9, $10, NULL, NULL)
           RETURNING parent_id, name, color, id, project_id, workspace_id, sort_order"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(write.parent.flatten())
    .bind(&name)
    .bind(&color)
    .bind(sort_order)
    .fetch_one(&pool)
    .await;
    match inserted {
        Ok(row) => Ok(json_created(render_label(&label_row_of(&row)?)?)),
        Err(error) if is_integrity_violation(&error) => Ok(json_bad_request(LABEL_DUPLICATE_BODY)),
        Err(_) => Err(Denial::ServerError),
    }
}

/// `PUT .../issue-labels/{pk}/`: DRF's *default* update — no
/// `@invalidate_cache`, no `@allow_permission` (only the class unsafe arm
/// gates it), full validation, and `validate_name` in the null-project
/// scope. `IntegrityError` surfaces the generic payload arm.
pub async fn label_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_unsafe(&pool, &slug, &project_id, &user_id).await {
        return Ok(response);
    }
    let Some(current) = label_object(&pool, &slug, &project_id, &user_id, &pk).await? else {
        return Ok(json_not_found(LABEL_NOT_FOUND_BODY));
    };
    let (parts, bytes) = split_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes, &LABEL_BODY_SPEC, true) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let object = match object_or_errors(&data.value) {
        Ok(object) => object,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    let write = match validate_label_fields(&data, object, false) {
        Ok(write) => write,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    if let Some(Some(parent)) = write.parent {
        if !parent_exists(&pool, &parent).await? {
            let mut errors = FieldErrors::default();
            errors.push(
                "parent",
                format!("Invalid pk \"{parent}\" - object does not exist."),
            );
            return Ok(json_bad_request(&errors.body()));
        }
    }
    let name = write.name.clone().expect("validated name");
    let conflict = validate_name_put(&pool, &name, &pk).await?;
    if let Err(body) = conflict {
        return Ok(json_bad_request(&body));
    }
    // `save()` re-fills `workspace` from the project (a no-op fetch that
    // 404s only when the project row vanished under a surviving label).
    project_row(&pool, &project_id).await?;
    let now = Utc::now();
    // Absent optional fields keep their columns (DRF excludes them from
    // `validated_data`); only explicit `null` clears `parent`.
    let updated = sqlx::query(
        r#"UPDATE labels SET parent_id = $1, name = $2, color = $3, sort_order = $4,
           updated_by_id = $5, updated_at = $6 WHERE id = $7
           RETURNING parent_id, name, color, id, project_id, workspace_id, sort_order"#,
    )
    .bind(write.parent.unwrap_or(current.parent_id))
    .bind(&name)
    .bind(write.color.unwrap_or_else(|| current.color.clone()))
    .bind(write.sort_order.unwrap_or(current.sort_order))
    .bind(user_id)
    .bind(now)
    .bind(pk)
    .fetch_one(&pool)
    .await;
    match updated {
        Ok(row) => Ok(json_ok(render_label(&label_row_of(&row)?)?)),
        Err(error) if is_integrity_violation(&error) => {
            Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY))
        }
        Err(_) => Err(Denial::ServerError),
    }
}

/// `PATCH .../issue-labels/{pk}/` (`label.py:57-82`): class unsafe gate,
/// invalidation, ADMIN decorator, then the exact-match pre-check (BEFORE
/// `get_object`: a duplicate name on a missing row is a 400, not a 404),
/// partial validation + `validate_name`, and the save. `PUT`-style payload
/// arm on `IntegrityError`.
pub async fn label_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_unsafe(&pool, &slug, &project_id, &user_id).await {
        return Ok(response);
    }
    invalidate_workspace_labels(state.redis(), &slug, false).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], None).await?;

    let (parts, bytes) = split_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes, &LABEL_BODY_SPEC, true) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let object = match object_or_errors(&data.value) {
        Ok(object) => object,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    // Exact-match pre-check over the raw value (`str()` of scalars, like
    // `get_prep_value`; `null` never matches; an upload reads its filename).
    let precheck_name: Option<String> = if data_has_file(&data.files, "name") {
        data_file_name(&data.files, "name").map(str::to_owned)
    } else {
        object.get("name").and_then(py_str_scalar)
    };
    if let Some(candidate) = precheck_name {
        let hit: Option<(i32,)> = sqlx::query_as(LABEL_NAME_PRECHECK_SQL)
            .bind(project_id)
            .bind(&candidate)
            .bind(pk)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if hit.is_some() {
            return Ok(json_bad_request(LABEL_DUPLICATE_BODY));
        }
    }

    let Some(current) = label_object(&pool, &slug, &project_id, &user_id, &pk).await? else {
        return Ok(json_not_found(LABEL_NOT_FOUND_BODY));
    };
    let write = match validate_label_fields(&data, object, true) {
        Ok(write) => write,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    if let Some(Some(parent)) = write.parent {
        if !parent_exists(&pool, &parent).await? {
            let mut errors = FieldErrors::default();
            errors.push(
                "parent",
                format!("Invalid pk \"{parent}\" - object does not exist."),
            );
            return Ok(json_bad_request(&errors.body()));
        }
    }
    let name = write.name.clone().unwrap_or_else(|| current.name.clone());
    if write.name.is_some() {
        let conflict = validate_name_update(&pool, &project_id, &name, &pk).await?;
        if let Err(body) = conflict {
            return Ok(json_bad_request(&body));
        }
    }
    project_row(&pool, &project_id).await?;
    let now = Utc::now();
    let updated = sqlx::query(
        r#"UPDATE labels SET parent_id = $1, name = $2, color = $3, sort_order = $4,
           updated_by_id = $5, updated_at = $6 WHERE id = $7
           RETURNING parent_id, name, color, id, project_id, workspace_id, sort_order"#,
    )
    .bind(write.parent.unwrap_or(current.parent_id))
    .bind(&name)
    .bind(write.color.unwrap_or_else(|| current.color.clone()))
    .bind(write.sort_order.unwrap_or(current.sort_order))
    .bind(user_id)
    .bind(now)
    .bind(pk)
    .fetch_one(&pool)
    .await;
    match updated {
        Ok(row) => Ok(json_ok(render_label(&label_row_of(&row)?)?)),
        Err(error) if is_integrity_violation(&error) => {
            Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY))
        }
        Err(_) => Err(Denial::ServerError),
    }
}

/// `DELETE .../issue-labels/{pk}/` (`label.py:84-88` + DRF default destroy):
/// soft-delete (`deleted_at`, `updated_by`) plus the
/// `soft_delete_related_objects` enqueue, then 204.
pub async fn label_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    if let Err(response) = gate_class_unsafe(&pool, &slug, &project_id, &user_id).await {
        return Ok(response);
    }
    invalidate_workspace_labels(state.redis(), &slug, false).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], None).await?;
    if label_object(&pool, &slug, &project_id, &user_id, &pk)
        .await?
        .is_none()
    {
        return Ok(json_not_found(LABEL_NOT_FOUND_BODY));
    }
    // `delete()` → `save()` re-fills `workspace` from the project first.
    project_row(&pool, &project_id).await?;
    let now = Utc::now();
    sqlx::query(
        "UPDATE labels SET deleted_at = $1, updated_by_id = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(now)
    .bind(user_id)
    .bind(now)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_message(&pool, label_soft_delete_message(&pk)).await;
    Ok(no_content())
}

// ---- bulk create -------------------------------------------------------------------

/// `POST .../bulk-create-labels/` (`label.py:91-117`): ADMIN-only, no
/// invalidation, no validation at all. `label_data` must be an array of
/// objects (anything else is `AttributeError`/`TypeError` → 500); names and
/// descriptions `str()` (with `None` tripping the `NOT NULL` columns on
/// flush → the payload 400); colors random with the inclusive-upper-bound
/// quirk; ids pre-assigned per row (UUID defaults apply at construction, so
/// conflict-skipped rows still echo their ids); `bulk_create` batches of 50
/// with `ignore_conflicts` (a `NULL` name fails its whole batch).
pub async fn bulk_create_labels(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], None).await?;

    let data = match negotiate_input(&headers, &body, &RAW_BODY_SPEC, true) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // `request.data.get("label_data", [])`: non-dicts have no `.get`.
    let Value::Object(map) = &data.value else {
        return Err(Denial::ServerError);
    };
    let items = match map.get("label_data") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        // `for label in <non-iterable>` is `TypeError`; iterating a dict
        // or string feeds non-dicts to `.get` below — all 500s. Only a
        // JSON array iterates objects; anything else fails here or at the
        // item lookup, so reject non-arrays up front.
        Some(_) => return Err(Denial::ServerError),
    };
    // `label.get("name", "Migrated")`: non-dict items 500.
    let mut rows: Vec<(Uuid, Option<String>, Option<String>, String)> =
        Vec::with_capacity(items.len());
    for item in &items {
        let Value::Object(item) = item else {
            return Err(Denial::ServerError);
        };
        let name = match item.get("name") {
            None => Some("Migrated".to_owned()),
            Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Number(number)) => Some(py_str_of_number(number)),
            Some(Value::Bool(true)) => Some("True".to_owned()),
            Some(Value::Bool(false)) => Some("False".to_owned()),
            Some(Value::Array(_) | Value::Object(_)) => {
                Some(py_repr_value(item.get("name").expect("matched")))
            }
        };
        let description = match item.get("description") {
            None => Some("Migrated Issue".to_owned()),
            Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Number(number)) => Some(py_str_of_number(number)),
            Some(Value::Bool(true)) => Some("True".to_owned()),
            Some(Value::Bool(false)) => Some("False".to_owned()),
            Some(Value::Array(_) | Value::Object(_)) => {
                Some(py_repr_value(item.get("description").expect("matched")))
            }
        };
        let color = format!("#{:06X}", rand::rng().random_range(0..=0x1000000));
        rows.push((Uuid::new_v4(), name, description, color));
    }

    let (_, workspace_id) = project_row(&pool, &project_id).await?;
    let now = Utc::now();
    // `bulk_create(..., batch_size=50, ignore_conflicts=True)`: one
    // multi-row `INSERT ... ON CONFLICT DO NOTHING` per batch (Django
    // emits the arbiter-less form, which needs no partial-index match).
    for batch in rows.chunks(50) {
        let mut sql = String::from(
            "INSERT INTO labels (id, created_at, updated_at, created_by_id, updated_by_id, workspace_id, project_id, parent_id, name, description, color, sort_order, external_source, external_id) VALUES ",
        );
        for (index, _) in batch.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            // The six shared binds occupy $1..$6; each row adds four.
            let base = index * 4;
            sql.push_str(&format!(
                "(${}, $1, $2, $3, $4, $5, $6, NULL, ${}, ${}, ${}, 65535, NULL, NULL)",
                base + 7,
                base + 8,
                base + 9,
                base + 10,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        let mut insert = sqlx::query(&sql)
            .bind(now)
            .bind(now)
            .bind(user_id)
            .bind(user_id)
            .bind(workspace_id)
            .bind(project_id);
        for (id, name, description, color) in batch {
            insert = insert
                .bind(id)
                .bind(name.as_deref())
                .bind(description.as_deref())
                .bind(color);
        }
        // A `NULL` name/description trips `NOT NULL` for its batch (the
        // whole batch fails, like the single Django statement); unique
        // conflicts are swallowed per row by `DO NOTHING`.
        if let Err(error) = insert.execute(&pool).await {
            if is_integrity_violation(&error) {
                return Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY));
            }
            return Err(Denial::ServerError);
        }
    }

    // `LabelSerializer(labels, many=True)` over the constructed rows:
    // every input echoes with its pre-assigned id, `sort_order` the field
    // default (`bulk_create` skips `save()`).
    let mut out = String::from("{\"labels\":[");
    for (index, (id, name, description, color)) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = description;
        let id = id.to_string();
        let project_id = project_id.to_string();
        let workspace_id = workspace_id.to_string();
        let row_data = AppLabelRow {
            parent: None,
            name: name.as_deref().unwrap_or(""),
            color: color.as_str(),
            id: id.as_str(),
            project_id: Some(project_id.as_str()),
            workspace_id: workspace_id.as_str(),
            sort_order: 65535.0,
        };
        let view = app_label_to_representation(&row_data);
        out.push_str(&serde_json::to_string(&view).map_err(|_| Denial::ServerError)?);
    }
    out.push_str("]}");
    Ok(json_created(out))
}

// ---- v1 attachments ------------------------------------------------------------------

/// v1 list query rows for an issue: no entity-type/uploaded filter (as
/// coded), `-created_at` from `Meta.ordering`.
async fn v1_list_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Vec<AssetRow>, Denial> {
    let rows = sqlx::query(&format!(
        r#"SELECT {ASSET_READ_COLUMNS} FROM file_assets fa
           JOIN workspaces w ON w.id = fa.workspace_id
           WHERE fa.issue_id = $1 AND w.slug = $2 AND fa.project_id = $3 AND fa.deleted_at IS NULL
           ORDER BY fa.created_at DESC"#
    ))
    .bind(issue_id)
    .bind(slug)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    rows.iter().map(asset_row_of).collect()
}

/// `GET .../issue-attachments/`: ADMIN/MEMBER/GUEST list.
pub async fn v1_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(issue_id) = issue_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let rows = v1_list_rows(&pool, &slug, &project_id, &issue_id).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&render_asset(row, &slug, &timezone)?);
    }
    out.push(']');
    Ok(json_ok(out))
}

/// `POST .../issue-attachments/` (`attachment.py:32-60`): workspace lookup,
/// full serializer validation — then the ported 500: any valid file dies
/// in the broken `S3Storage` before any row is written.
pub async fn v1_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(_) = issue_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    // The workspace lookup mirrors `:39` (a miss is the required-object
    // 404). It runs after the decorator, which 403s on a bad slug first,
    // so the miss arm is unreachable while the slug gates pass.
    workspace_row(&pool, &slug).await?;

    let (parts, bytes) = split_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes, &ATTACHMENT_BODY_SPEC, false) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    let object = match object_or_errors(&data.value) {
        Ok(object) => object,
        Err(errors) => return Ok(json_bad_request(&errors.body())),
    };
    let mut errors = validate_attachment_fields(&data, object);
    // FK existence over the default managers (only when the pure pass is
    // otherwise clean — DRF collects all field errors first either way,
    // and existence failures append per field in the same order).
    if errors.is_empty() {
        for (field, table, scoped) in ATTACHMENT_FK_TABLES {
            let raw = match object.get(*field) {
                None => continue,
                Some(Value::Null) => continue,
                Some(Value::String(text)) if text.is_empty() => continue,
                Some(raw) => raw,
            };
            // Unparseable values already errored in the pure pass.
            let Some(id) = py_uuid_of_value(raw) else {
                continue;
            };
            let sql = if *scoped {
                format!("SELECT 1 FROM {table} WHERE id = $1 AND deleted_at IS NULL")
            } else {
                format!("SELECT 1 FROM {table} WHERE id = $1")
            };
            let hit: Option<(i32,)> = sqlx::query_as(&sql)
                .bind(id)
                .fetch_optional(&pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            if hit.is_none() {
                let rendered = match raw {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => py_str_of_number(number),
                    _ => unreachable!("pure pass filters these"),
                };
                errors.push(
                    field,
                    format!("Invalid pk \"{rendered}\" - object does not exist."),
                );
            }
        }
    }
    if !errors.is_empty() {
        return Ok(json_bad_request(&errors.body()));
    }
    // Valid file in hand: `serializer.save()` → `FileField.pre_save` →
    // `S3Storage._save` → `AttributeError` on the missing
    // `file_overwrite` → generic 500. No row, no activity.
    Err(Denial::ServerError)
}

/// `DELETE .../issue-attachments/{pk}/` (`attachment.py:62-90`): ADMIN or
/// creator; a miss is the custom 404; an existing row hits the same broken
/// `storage.delete` (`location` missing) → generic 500 with the row, the
/// soft-delete, and the activity all untouched.
pub async fn v1_delete(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let (Ok(issue_id), Ok(pk)) = (issue_raw.parse::<Uuid>(), pk_raw.parse::<Uuid>()) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], Some(pk)).await?;
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT fa.id FROM file_assets fa
           JOIN workspaces w ON w.id = fa.workspace_id
           WHERE fa.id = $1 AND w.slug = $2 AND fa.project_id = $3 AND fa.issue_id = $4
           AND fa.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(pk)
    .bind(&slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        return Ok(json_not_found(ATTACHMENT_NOT_FOUND_BODY));
    }
    Err(Denial::ServerError)
}

// ---- v2 attachments -------------------------------------------------------------------

/// Python `int(value)` for the v2 `size` (`attachment.py:102`), as a
/// normalized decimal (sign + digits, no leading zeros): bools are `0`/`1`,
/// floats truncate toward zero, strings strip and take an optional sign +
/// digits (single underscores between digits; empty and float spellings
/// fail, as do digit runs past 4300), containers/`null`/non-finite fail.
/// Arbitrary precision is kept because `min(size, limit)`, the attributes
/// JSON, and the presigned conditions all spell the full decimal.
fn py_int_for_size(value: &Value) -> Result<String, ()> {
    match value {
        Value::Bool(true) => Ok("1".to_owned()),
        Value::Bool(false) => Ok("0".to_owned()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int.to_string());
            }
            if let Some(int) = number.as_u64() {
                return Ok(int.to_string());
            }
            if number.is_f64() {
                let float = number.as_f64().ok_or(())?;
                if !float.is_finite() {
                    return Err(());
                }
                let truncated = float.trunc();
                // Past i128 the decimal is still exact: expose enough
                // digits via fixed formatting, then normalize.
                if truncated.abs() >= 1e38 {
                    return Ok(normalize_decimal(&format!("{truncated:.0}")));
                }
                return Ok((truncated as i128).to_string());
            }
            // Arbitrary-precision literal: optional `-`, plain digits.
            int_text_value(&number.to_string())
        }
        Value::String(text) => int_text_value(text),
        _ => Err(()),
    }
}

fn normalize_decimal(text: &str) -> String {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".to_owned();
    }
    if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    }
}

fn int_text_value(text: &str) -> Result<String, ()> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() {
        return Err(());
    }
    // Single underscores between digits, like `float()`.
    let bytes = digits.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'_' {
            if !byte.is_ascii_digit() {
                return Err(());
            }
            continue;
        }
        let prev = index.checked_sub(1).and_then(|i| bytes.get(i));
        let next = bytes.get(index + 1);
        if !(prev.is_some_and(|b| b.is_ascii_digit()) && next.is_some_and(|b| b.is_ascii_digit())) {
            return Err(());
        }
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    // `sys.get_int_max_str_digits()` (4300): past it `int()` raises.
    if clean.len() > 4300 {
        return Err(());
    }
    let negative = trimmed.starts_with('-');
    Ok(normalize_decimal(&format!(
        "{}{clean}",
        if negative { "-" } else { "" }
    )))
}

/// `min(size, limit)` over a normalized decimal, without parsing (either
/// side may exceed `i128`): sign-aware magnitude comparison.
fn min_with_limit(size: &str, limit: i64) -> String {
    let cap = limit.to_string();
    let (size_neg, size_digits) = match size.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, size),
    };
    let (cap_neg, cap_digits) = match cap.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, cap.as_str()),
    };
    let size_is_smaller = match (size_neg, cap_neg) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => {
            size_digits.len() < cap_digits.len()
                || (size_digits.len() == cap_digits.len() && size_digits <= cap_digits)
        }
        (true, true) => {
            size_digits.len() > cap_digits.len()
                || (size_digits.len() == cap_digits.len() && size_digits >= cap_digits)
        }
    };
    if size_is_smaller {
        size.to_owned()
    } else {
        cap
    }
}

/// `GET .../assets/v2/.../attachments/`: uploaded issue attachments only.
pub async fn v2_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(issue_id) = issue_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let rows = sqlx::query(&format!(
        r#"SELECT {ASSET_READ_COLUMNS} FROM file_assets fa
           JOIN workspaces w ON w.id = fa.workspace_id
           WHERE fa.issue_id = $1 AND fa.entity_type = 'ISSUE_ATTACHMENT' AND w.slug = $2
           AND fa.project_id = $3 AND fa.is_uploaded AND fa.deleted_at IS NULL
           ORDER BY fa.created_at DESC"#
    ))
    .bind(issue_id)
    .bind(&slug)
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&render_asset(&asset_row_of(row)?, &slug, &timezone)?);
    }
    out.push(']');
    Ok(json_ok(out))
}

/// One v2-scoped asset row (no issue scoping on the `.get` calls, as coded).
async fn v2_object(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<AssetRow>, Denial> {
    let row = sqlx::query(&format!(
        r#"SELECT {ASSET_READ_COLUMNS} FROM file_assets fa
           JOIN workspaces w ON w.id = fa.workspace_id
           WHERE fa.id = $1 AND w.slug = $2 AND fa.project_id = $3 AND fa.deleted_at IS NULL"#
    ))
    .bind(pk)
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| asset_row_of(&row)).transpose()
}

/// `POST .../assets/v2/.../attachments/` (`attachment.py:92-146`): the
/// `int(size)` 500 arm, then the allowlist 400, then the workspace 404,
/// then the row + presigned POST. A missing project/issue surfaces as the
/// payload 400 (FK `IntegrityError`); `name` is unvalidated (a missing one
/// renders `None` into the key and the attributes, as the f-string does).
pub async fn v2_create(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(issue_id) = issue_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;

    let (parts, bytes) = split_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes, &RAW_BODY_SPEC, true) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // `.get(...)` on non-dicts is `AttributeError` → 500.
    let Value::Object(map) = &data.value else {
        return Err(Denial::ServerError);
    };
    // `request.data` merges texts-then-files: an upload wins the key and
    // reads as its filename — except `size`, where `int(file)` is
    // `TypeError` → 500.
    let name = data_file_name(&data.files, "name")
        .map(|name| Value::String(name.to_owned()))
        .or_else(|| map.get("name").cloned());
    let file_type = data_file_name(&data.files, "type")
        .map(|name| Value::String(name.to_owned()))
        .or_else(|| map.get("type").cloned());
    if data_has_file(&data.files, "size") {
        return Err(Denial::ServerError);
    }
    let limit = state.settings().file_size_limit;
    let size = match map.get("size") {
        None => limit.to_string(),
        Some(raw) => py_int_for_size(raw).map_err(|()| Denial::ServerError)?,
    };
    let file_type = match file_type.as_ref().and_then(|t| t.as_str()) {
        Some(text) if !text.is_empty() && ATTACHMENT_MIME_TYPES.contains(&text) => text.to_owned(),
        _ => return Ok(json_bad_request(INVALID_FILE_TYPE_BODY)),
    };
    let (workspace_id, workspace_slug) = workspace_row(&pool, &slug).await?;
    let size_limit = min_with_limit(&size, limit);
    // `size = FloatField`: `min()` caps at the configured limit, so an
    // infinite `size_limit` needs `FILE_SIZE_LIMIT` itself past `f64::MAX`
    // (Python would emit bare `Infinity` there); 500 before writing.
    let size_float: f64 = size_limit.parse().unwrap_or(f64::NAN);
    if !size_float.is_finite() {
        return Err(Denial::ServerError);
    }
    let asset_key = format!(
        "{workspace_id}/{}-{}",
        Uuid::new_v4().simple(),
        match name.as_ref() {
            None | Some(Value::Null) => "None".to_owned(),
            Some(raw) => py_str_scalar(raw).unwrap_or_else(|| py_repr_value(raw)),
        }
    );
    let mut attributes = Map::with_capacity(3);
    attributes.insert("name".to_owned(), name.clone().unwrap_or(Value::Null));
    attributes.insert("type".to_owned(), Value::String(file_type.clone()));
    // The attributes int keeps arbitrary precision (like the Python `int`).
    let size_number: serde_json::Number =
        serde_json::from_str(&size_limit).expect("normalized decimal");
    attributes.insert("size".to_owned(), Value::Number(size_number));
    let now = Utc::now();
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        r#"INSERT INTO file_assets (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            attributes, asset, user_id, workspace_id, draft_issue_id, project_id, issue_id,
            comment_id, page_id, entity_type, entity_identifier, is_deleted, is_archived,
            external_id, external_source, size, is_uploaded, storage_metadata)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, NULL, $7, NULL, $8, $9,
            NULL, NULL, 'ISSUE_ATTACHMENT', NULL, FALSE, FALSE, NULL, NULL, $10, FALSE, '{}')
           RETURNING id"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(Value::Object(attributes))
    .bind(&asset_key)
    .bind(workspace_id)
    .bind(project_id)
    .bind(issue_id)
    .bind(size_float)
    .fetch_one(&pool)
    .await;
    if let Err(error) = inserted {
        if is_integrity_violation(&error) {
            return Ok(json_bad_request(PAYLOAD_NOT_VALID_BODY));
        }
        return Err(Denial::ServerError);
    }
    let row = v2_object(&pool, &slug, &project_id, &id)
        .await?
        .ok_or(Denial::ServerError)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let attachment = render_asset(&row, &workspace_slug, &timezone)?;
    let host = host_of(&parts.headers).ok_or(Denial::ServerError)?;
    let upload_data = presigned_post(
        &state.settings().storage,
        &scheme_of(&parts.headers),
        &host,
        &asset_key,
        &file_type,
        &size_limit,
        &now,
    );
    let asset_url = pidash_db::app_assets::columns::asset_url(
        Some("ISSUE_ATTACHMENT"),
        &id.to_string(),
        &workspace_slug,
        &project_id.to_string(),
        &issue_id.to_string(),
    );
    // Key order mirrors the view dict: `upload_data`, `asset_id`,
    // `attachment`, `asset_url`.
    let mut out = String::from("{\"upload_data\":");
    out.push_str(&serde_json::to_string(&upload_data).map_err(|_| Denial::ServerError)?);
    out.push_str(",\"asset_id\":");
    out.push_str(&serde_json::to_string(&id.to_string()).expect("uuid"));
    out.push_str(",\"attachment\":");
    out.push_str(&attachment);
    out.push_str(",\"asset_url\":");
    out.push_str(&serde_json::to_string(&asset_url).map_err(|_| Denial::ServerError)?);
    out.push('}');
    Ok(json_ok(out))
}

/// `GET .../assets/v2/.../attachments/{pk}/` (`attachment.py:169-188`): a
/// miss 404s; a pending asset 400s; an uploaded one redirects (302) to the
/// presigned GET. The filename is `attributes["name"]` (missing → fresh
/// uuid4 hex, empty → bare disposition); non-dict attributes are the 500
/// the `.get` raises.
pub async fn v2_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let (Ok(_), Ok(pk)) = (issue_raw.parse::<Uuid>(), pk_raw.parse::<Uuid>()) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    let Some(row) = v2_object(&pool, &slug, &project_id, &pk).await? else {
        return Ok(Denial::NotFound.into_response());
    };
    if !row.is_uploaded {
        return Ok(json_bad_request(ASSET_NOT_UPLOADED_BODY));
    }
    let Value::Object(attributes) = &row.attributes else {
        return Err(Denial::ServerError);
    };
    let filename = match attributes.get("name") {
        None | Some(Value::Null) => Filename::FreshHex,
        Some(Value::String(name)) if name.is_empty() => Filename::Bare,
        Some(Value::String(name)) => Filename::Name(name.clone()),
        // `quote(non_str)` is `TypeError` → 500 (reachable: v2 `POST`
        // stores `name` raw, so a numeric name 500s its own download).
        Some(_) => return Err(Denial::ServerError),
    };
    let (parts, _) = split_parts(req).await?;
    let host = host_of(&parts.headers).ok_or(Denial::ServerError)?;
    let url = presigned_get_url(
        &state.settings().storage,
        &scheme_of(&parts.headers),
        &host,
        &row.asset,
        filename,
        &Utc::now(),
    );
    Ok(redirect(url))
}

/// `PATCH .../assets/v2/.../attachments/{pk}/` (`attachment.py:190-229`):
/// the row is serialized BEFORE the flip (`current_instance` carries
/// `is_uploaded: false`); a pending asset flips to uploaded with
/// `created_by` reset to the actor and publishes the created activity; an
/// empty `storage_metadata` (`None` or `{}`) enqueues the metadata task;
/// `save()` runs unconditionally (`updated_by`, `updated_at`); 204. The
/// body is never read.
pub async fn v2_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let (Ok(issue_id), Ok(pk)) = (issue_raw.parse::<Uuid>(), pk_raw.parse::<Uuid>()) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20, 15, 5], None).await?;
    let Some(row) = v2_object(&pool, &slug, &project_id, &pk).await? else {
        return Ok(Denial::NotFound.into_response());
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    let origin = request_origin(&state)?;
    let now = Utc::now();
    if !row.is_uploaded {
        // Pre-flip serialization, exactly as the view passes it to
        // `json.dumps` (compact separators here — the payload parses
        // identically; the cycles precedent).
        let current_instance = render_asset(&row, &slug, &timezone)?;
        enqueue_message(
            &pool,
            issue_activity_message(
                "attachment.activity.created",
                Some(current_instance),
                &issue_id,
                &user_id,
                &project_id,
                now.timestamp(),
                &origin,
            ),
        )
        .await;
        sqlx::query(
            "UPDATE file_assets SET is_uploaded = TRUE, created_by_id = $1, updated_by_id = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(user_id)
        .bind(user_id)
        .bind(now)
        .bind(pk)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    } else {
        sqlx::query("UPDATE file_assets SET updated_by_id = $1, updated_at = $2 WHERE id = $3")
            .bind(user_id)
            .bind(now)
            .bind(pk)
            .execute(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    let metadata_empty = row
        .storage_metadata
        .as_ref()
        .is_none_or(|meta| meta.as_object().is_some_and(|map| map.is_empty()));
    if metadata_empty {
        enqueue_message(&pool, asset_metadata_message(&pk)).await;
    }
    Ok(no_content())
}

/// `DELETE .../assets/v2/.../attachments/{pk}/` (`attachment.py:232-252`):
/// ADMIN or creator; flag-style soft delete (plain `save()`, no enqueue
/// from the model) plus the deleted activity; 204.
pub async fn v2_delete(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let (Ok(issue_id), Ok(pk)) = (issue_raw.parse::<Uuid>(), pk_raw.parse::<Uuid>()) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = super::resolve_project_id(&pool, &slug, &project_raw).await?;
    gate_decorator(&pool, &slug, &project_id, &user_id, &[20], Some(pk)).await?;
    if v2_object(&pool, &slug, &project_id, &pk).await?.is_none() {
        return Ok(Denial::NotFound.into_response());
    }
    let origin = request_origin(&state)?;
    let now = Utc::now();
    sqlx::query(
        "UPDATE file_assets SET is_deleted = TRUE, deleted_at = $1, updated_by_id = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(now)
    .bind(user_id)
    .bind(now)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    enqueue_message(
        &pool,
        issue_activity_message(
            "attachment.activity.deleted",
            None,
            &issue_id,
            &user_id,
            &project_id,
            now.timestamp(),
            &origin,
        ),
    )
    .await;
    Ok(no_content())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::config::StorageSettings;

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

    fn file_data(key: &str, filename: &str, bytes: &[u8]) -> RequestData {
        let mut files: shared_body::FilesMap = BTreeMap::new();
        files.insert(
            key.to_owned(),
            vec![shared_body::FilePart {
                filename: filename.to_owned(),
                content_type: "text/plain".to_owned(),
                bytes: bytes.to_vec(),
                in_memory: true,
            }],
        );
        RequestData {
            value: Value::Object(Map::new()),
            files,
            is_html: true,
        }
    }

    fn label_errors(text: &str, partial: bool) -> String {
        let data = json_data(text);
        let object = object_or_errors(&data.value).expect("object");
        match validate_label_fields(&data, object, partial) {
            Ok(_) => String::new(),
            Err(errors) => errors.body(),
        }
    }

    // ---- read shapes ------------------------------------------------------

    #[test]
    fn attachment_wire_keys_match_contract_set_in_drf_order() {
        assert_eq!(APP_ATTACHMENT_FIELDS.len(), 25);
        assert_eq!(APP_ATTACHMENT_FIELDS[0], "id");
        // The declared `asset_url` sits right after the pk (probed).
        assert_eq!(APP_ATTACHMENT_FIELDS[1], "asset_url");
        assert_eq!(
            &APP_ATTACHMENT_FIELDS[2..],
            &[
                "created_at",
                "updated_at",
                "deleted_at",
                "attributes",
                "asset",
                "entity_type",
                "entity_identifier",
                "is_deleted",
                "is_archived",
                "external_id",
                "external_source",
                "size",
                "is_uploaded",
                "storage_metadata",
                "created_by",
                "updated_by",
                "user",
                "workspace",
                "draft_issue",
                "project",
                "issue",
                "comment",
                "page",
            ]
        );
        // `ASSET_KEYS` (`test_attachments.py`) as a sorted set equals.
        let mut keys = APP_ATTACHMENT_FIELDS.to_vec();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "asset",
                "asset_url",
                "attributes",
                "comment",
                "created_at",
                "created_by",
                "deleted_at",
                "draft_issue",
                "entity_identifier",
                "entity_type",
                "external_id",
                "external_source",
                "id",
                "is_archived",
                "is_deleted",
                "is_uploaded",
                "issue",
                "page",
                "project",
                "size",
                "storage_metadata",
                "updated_at",
                "updated_by",
                "user",
                "workspace",
            ]
        );
    }

    #[test]
    fn attachment_read_renders_seed_shape_byte_exact() {
        let row = AppAttachmentRow {
            id: Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("uuid"),
            asset_url: Some(
                "/api/assets/v2/workspaces/acme/projects/22222222-2222-4222-8222-222222222222/issues/33333333-3333-4333-8333-333333333333/attachments/11111111-1111-4111-8111-111111111111/"
                    .to_owned(),
            ),
            created_at: "2026-09-02T01:02:03Z".to_owned(),
            updated_at: "2026-09-02T01:02:04Z".to_owned(),
            deleted_at: None,
            attributes: serde_json::json!({"name": "seed.txt"}),
            asset: Some("seed.txt".to_owned()),
            entity_type: Some("ISSUE_ATTACHMENT".to_owned()),
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id: None,
            external_source: None,
            size: 12.0,
            is_uploaded: true,
            storage_metadata: None,
            created_by: Some(Uuid::parse_str("44444444-4444-4444-8444-444444444444").expect("uuid")),
            updated_by: None,
            user: Some(Uuid::parse_str("44444444-4444-4444-8444-444444444444").expect("uuid")),
            workspace: Some(Uuid::parse_str("55555555-5555-4555-8555-555555555555").expect("uuid")),
            draft_issue: None,
            project: Some(Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid")),
            issue: Some(Uuid::parse_str("33333333-3333-4333-8333-333333333333").expect("uuid")),
            comment: None,
            page: None,
        };
        let view = app_attachment_to_representation(&row);
        let bytes = serde_json::to_string(&view).expect("bytes");
        assert_eq!(
            bytes,
            r#"{"id":"11111111-1111-4111-8111-111111111111","asset_url":"/api/assets/v2/workspaces/acme/projects/22222222-2222-4222-8222-222222222222/issues/33333333-3333-4333-8333-333333333333/attachments/11111111-1111-4111-8111-111111111111/","created_at":"2026-09-02T01:02:03Z","updated_at":"2026-09-02T01:02:04Z","deleted_at":null,"attributes":{"name":"seed.txt"},"asset":"seed.txt","entity_type":"ISSUE_ATTACHMENT","entity_identifier":null,"is_deleted":false,"is_archived":false,"external_id":null,"external_source":null,"size":12.0,"is_uploaded":true,"storage_metadata":null,"created_by":"44444444-4444-4444-8444-444444444444","updated_by":null,"user":"44444444-4444-4444-8444-444444444444","workspace":"55555555-5555-4555-8555-555555555555","draft_issue":null,"project":"22222222-2222-4222-8222-222222222222","issue":"33333333-3333-4333-8333-333333333333","comment":null,"page":null}"#
        );
        // Key order is the wire order.
        let parsed: Map<String, Value> = serde_json::from_str(&bytes).expect("parse");
        let keys: Vec<&str> = parsed.keys().map(String::as_str).collect();
        assert_eq!(keys, APP_ATTACHMENT_FIELDS);
    }

    // ---- top-level shapes ---------------------------------------------------

    #[test]
    fn top_level_shape_errors_match_drf() {
        for (text, expected) in [
            ("null", r#"{"non_field_errors":["No data provided"]}"#),
            (
                "[1]",
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#,
            ),
            (
                r#""x""#,
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got str."]}"#,
            ),
            (
                "5",
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got int."]}"#,
            ),
            (
                "5.5",
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got float."]}"#,
            ),
            (
                "true",
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got bool."]}"#,
            ),
        ] {
            let data = json_data(text);
            let errors = object_or_errors(&data.value).expect_err("shape error");
            assert_eq!(errors.body(), expected, "{text}");
        }
    }

    // ---- label validation -----------------------------------------------------

    #[test]
    fn label_required_and_blank_rules() {
        assert_eq!(
            label_errors("{}", false),
            r#"{"name":["This field is required."]}"#
        );
        // `partial=True` skips required; unknown keys drop.
        let data = json_data(r#"{"description":"cycled"}"#);
        let object = object_or_errors(&data.value).expect("object");
        assert!(validate_label_fields(&data, object, true).is_ok());
        assert_eq!(
            label_errors(r#"{"name":""}"#, false),
            r#"{"name":["This field may not be blank."]}"#
        );
        assert_eq!(
            label_errors(r#"{"name":null}"#, false),
            r#"{"name":["This field may not be null."]}"#
        );
        // `color` is optional even on full writes; `""` is kept.
        let data = json_data(r#"{"name":"x"}"#);
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("valid");
        assert_eq!(write.name.as_deref(), Some("x"));
        assert!(write.color.is_none());
        let data = json_data(r#"{"name":"x","color":""}"#);
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("valid");
        assert_eq!(write.color.as_deref(), Some(""));
    }

    #[test]
    fn label_char_coercion_and_max_length() {
        // Ints/floats stringify; bools and containers fail (probed).
        let data = json_data(r#"{"name":5}"#);
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("valid");
        assert_eq!(write.name.as_deref(), Some("5"));
        assert_eq!(
            label_errors(r#"{"name":true}"#, false),
            r#"{"name":["Not a valid string."]}"#
        );
        assert_eq!(
            label_errors(r#"{"name":["x"]}"#, false),
            r#"{"name":["Not a valid string."]}"#
        );
        assert_eq!(
            label_errors(&format!(r#"{{"name":"{}"}}"#, "n".repeat(256)), false),
            r#"{"name":["Ensure this field has no more than 255 characters."]}"#
        );
        // Length counts code points, not bytes.
        let data = json_data(&format!(r#"{{"name":"{}"}}"#, "é".repeat(255)));
        let object = object_or_errors(&data.value).expect("object");
        assert!(validate_label_fields(&data, object, false).is_ok());
        assert_eq!(
            label_errors(
                &format!(r#"{{"name":"x","color":"{}"}}"#, "c".repeat(256)),
                false
            ),
            r#"{"color":["Ensure this field has no more than 255 characters."]}"#
        );
    }

    #[test]
    fn label_parent_uuid_arms() {
        let missing = "99999999-9999-4999-8999-999999999999";
        // Bad strings fail fancy-quote; missing UUIDs pass the pure half
        // (existence is the caller's probe).
        assert_eq!(
            label_errors(r#"{"name":"x","parent":"nope"}"#, false),
            "{\"parent\":[\"\u{201c}nope\u{201d} is not a valid UUID.\"]}"
        );
        let data = json_data(&format!(r#"{{"name":"x","parent":"{missing}"}}"#));
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("pure-valid");
        assert_eq!(
            write.parent,
            Some(Some(Uuid::parse_str(missing).expect("uuid")))
        );
        assert_eq!(
            label_errors(r#"{"name":"x","parent":true}"#, false),
            r#"{"parent":["Incorrect type. Expected pk value, received bool."]}"#
        );
        assert_eq!(
            label_errors(r#"{"name":"x","parent":5.5}"#, false),
            "{\"parent\":[\"\u{201c}5.5\u{201d} is not a valid UUID.\"]}"
        );
        assert_eq!(
            label_errors(r#"{"name":"x","parent":["x"]}"#, false),
            "{\"parent\":[\"\u{201c}['x']\u{201d} is not a valid UUID.\"]}"
        );
        // `""` and `null` validate to `None`.
        for text in [
            r#"{"name":"x","parent":""}"#,
            r#"{"name":"x","parent":null}"#,
        ] {
            let data = json_data(text);
            let object = object_or_errors(&data.value).expect("object");
            let write = validate_label_fields(&data, object, false).expect("valid");
            assert_eq!(write.parent, Some(None), "{text}");
        }
    }

    #[test]
    fn label_sort_order_number_arms() {
        assert_eq!(
            label_errors(r#"{"name":"x","sort_order":"abc"}"#, false),
            r#"{"sort_order":["A valid number is required."]}"#
        );
        assert_eq!(
            label_errors(r#"{"name":"x","sort_order":null}"#, false),
            r#"{"sort_order":["This field may not be null."]}"#
        );
        assert_eq!(
            label_errors(r#"{"name":"x","sort_order":""}"#, false),
            r#"{"sort_order":["A valid number is required."]}"#
        );
        let data = json_data(r#"{"name":"x","sort_order":true}"#);
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("valid");
        assert_eq!(write.sort_order, Some(1.0));
        let data = json_data(r#"{"name":"x","sort_order":"2.5"}"#);
        let object = object_or_errors(&data.value).expect("object");
        let write = validate_label_fields(&data, object, false).expect("valid");
        assert_eq!(write.sort_order, Some(2.5));
    }

    #[test]
    fn label_errors_accumulate_in_field_order() {
        let text = format!(r#"{{"parent":true,"color":"{}"}}"#, "c".repeat(256));
        assert_eq!(
            label_errors(&text, true),
            r#"{"parent":["Incorrect type. Expected pk value, received bool."],"color":["Ensure this field has no more than 255 characters."]}"#
        );
    }

    // ---- attachment validation --------------------------------------------------

    fn attach_errors(data: &RequestData) -> String {
        let object = object_or_errors(&data.value).expect("object");
        validate_attachment_fields(data, object).body()
    }

    #[test]
    fn v1_asset_file_arms() {
        // Missing file (the contract-pinned 400).
        let data = json_data("{}");
        assert_eq!(
            attach_errors(&data),
            r#"{"asset":["No file was submitted."]}"#
        );
        // Empty upload.
        let data = file_data("asset", "e.txt", b"");
        assert_eq!(
            attach_errors(&data),
            r#"{"asset":["The submitted file is empty."]}"#
        );
        // Text where a file belongs.
        let data = html_data(&[("asset", "just-a-string")]);
        assert_eq!(
            attach_errors(&data),
            r#"{"asset":["The submitted data was not a file. Check the encoding type on the form."]}"#
        );
        // A valid upload passes validation (the handler 500s after).
        let data = file_data("asset", "a.txt", b"hello");
        assert_eq!(attach_errors(&data), "{}");
    }

    #[test]
    fn v1_scalar_field_arms() {
        let base = file_data("asset", "a.txt", b"hello");
        let with = |key: &str, member: Value| {
            let mut data = RequestData {
                value: base.value.clone(),
                files: base.files.clone(),
                is_html: false,
            };
            data.value
                .as_object_mut()
                .expect("object")
                .insert(key.to_owned(), member);
            data
        };
        assert_eq!(
            attach_errors(&with("size", serde_json::json!("abc"))),
            r#"{"size":["A valid number is required."]}"#
        );
        assert_eq!(
            attach_errors(&with("size", Value::Null)),
            r#"{"size":["This field may not be null."]}"#
        );
        assert_eq!(
            attach_errors(&with("is_uploaded", serde_json::json!("nope"))),
            r#"{"is_uploaded":["Must be a valid boolean."]}"#
        );
        assert_eq!(
            attach_errors(&with("is_uploaded", serde_json::json!("yes"))),
            "{}"
        );
        assert_eq!(
            attach_errors(&with("entity_type", serde_json::json!("e".repeat(300)))),
            r#"{"entity_type":["Ensure this field has no more than 255 characters."]}"#
        );
        assert_eq!(
            attach_errors(&with("user", serde_json::json!("nope"))),
            "{\"user\":[\"\u{201c}nope\u{201d} is not a valid UUID.\"]}"
        );
        assert_eq!(
            attach_errors(&with("attributes", Value::Null)),
            r#"{"attributes":["This field may not be null."]}"#
        );
        // `storage_metadata` is nullable; `attributes` takes any JSON.
        assert_eq!(attach_errors(&with("storage_metadata", Value::Null)), "{}");
        assert_eq!(
            attach_errors(&with("attributes", serde_json::json!([1]))),
            "{}"
        );
        // HTML `""` parses as JSON and fails (the `get_value` override
        // bypasses the blank skip).
        let mut html = html_data(&[("attributes", "")]);
        html.files = base.files.clone();
        assert_eq!(
            attach_errors(&html),
            r#"{"attributes":["Value must be valid JSON."]}"#
        );
    }

    // ---- Python coercions ---------------------------------------------------------

    // The over-precise literal is deliberate: the same source text as
    // the CPython probe, so both sides parse the identical `f64`.
    #[allow(clippy::excessive_precision)]
    #[test]
    fn py_float_repr_matches_cpython() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (12.0, "12.0"),
            (-2.5, "-2.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-1e100, "-1e+100"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 3.0, "0.3333333333333333"),
            (123456789.123456789, "123456789.12345679"),
            (999999999999999.0, "999999999999999.0"),
            (1.5e-7, "1.5e-07"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (100.0, "100.0"),
            (0.5, "0.5"),
            (123.456, "123.456"),
            (1e21, "1e+21"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ] {
            assert_eq!(py_float_repr(value), expected, "{value}");
        }
        assert_eq!(py_float_repr(f64::NAN), "nan");
    }

    #[test]
    fn py_repr_value_matches_containers() {
        assert_eq!(py_repr_value(&Value::Null), "None");
        assert_eq!(py_repr_value(&serde_json::json!(true)), "True");
        assert_eq!(py_repr_value(&serde_json::json!(5)), "5");
        assert_eq!(py_repr_value(&serde_json::json!(["x"])), "['x']");
        assert_eq!(py_repr_value(&serde_json::json!({"x": 1})), "{'x': 1}");
        assert_eq!(
            invalid_uuid_message(&serde_json::json!({"x": 1})),
            "“{'x': 1}” is not a valid UUID."
        );
    }

    #[test]
    fn drf_float_parsing_matches_python() {
        assert_eq!(parse_py_float("2.5"), Some(2.5));
        assert_eq!(parse_py_float("  10  "), Some(10.0));
        assert_eq!(parse_py_float("1_0"), Some(10.0));
        assert_eq!(parse_py_float("INF"), Some(f64::INFINITY));
        assert_eq!(parse_py_float("-infinity"), Some(f64::NEG_INFINITY));
        assert!(parse_py_float("nan").is_some_and(|v| v.is_nan()));
        assert_eq!(parse_py_float(""), None);
        assert_eq!(parse_py_float("abc"), None);
        assert_eq!(parse_py_float("1__0"), None);
        assert_eq!(parse_py_float("1e999"), Some(f64::INFINITY));
    }

    #[test]
    fn drf_bool_sets_match_drf() {
        for text in ["t", "y", "yes", "true", "on", "1", "YES", "True", "ON"] {
            assert_eq!(
                drf_bool_of_value(&Value::String(text.to_owned())),
                Some(true),
                "{text}"
            );
        }
        for text in ["f", "n", "no", "false", "off", "0", "NO", "False"] {
            assert_eq!(
                drf_bool_of_value(&Value::String(text.to_owned())),
                Some(false),
                "{text}"
            );
        }
        assert_eq!(drf_bool_of_value(&serde_json::json!(1)), Some(true));
        assert_eq!(drf_bool_of_value(&serde_json::json!(0)), Some(false));
        assert_eq!(drf_bool_of_value(&serde_json::json!(1.0)), Some(true));
        assert_eq!(drf_bool_of_value(&serde_json::json!(0.0)), Some(false));
        assert_eq!(drf_bool_of_value(&serde_json::json!(2)), None);
        assert_eq!(drf_bool_of_value(&serde_json::json!("nope")), None);
        assert_eq!(drf_bool_of_value(&serde_json::json!([1])), None);
    }

    #[test]
    fn py_int_for_size_matches_int() {
        assert_eq!(
            py_int_for_size(&serde_json::json!(true)),
            Ok("1".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!(10.5)),
            Ok("10".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!(-10.5)),
            Ok("-10".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!(" 10 ")),
            Ok("10".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!("1_0")),
            Ok("10".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!("+7")),
            Ok("7".to_owned())
        );
        assert_eq!(
            py_int_for_size(&serde_json::json!("-0")),
            Ok("0".to_owned())
        );
        for bad in ["abc", "", "10.5", "0x10", "1__0", "_1"] {
            assert_eq!(
                py_int_for_size(&Value::String(bad.to_owned())),
                Err(()),
                "{bad}"
            );
        }
        assert_eq!(py_int_for_size(&Value::Null), Err(()));
        assert_eq!(py_int_for_size(&serde_json::json!([1])), Err(()));
        // Past the 4300-digit `int()` ceiling.
        assert_eq!(py_int_for_size(&Value::String("9".repeat(4301))), Err(()));
        assert!(py_int_for_size(&Value::String("9".repeat(4300))).is_ok());
        // min() outcomes: positives cap, negatives pass through.
        assert_eq!(min_with_limit("10", 5242880), "10");
        assert_eq!(min_with_limit("99999999", 5242880), "5242880");
        assert_eq!(min_with_limit("-5", 5242880), "-5");
        assert_eq!(min_with_limit(&"9".repeat(100), 5242880), "5242880");
        assert_eq!(
            min_with_limit(&format!("-{}", "9".repeat(100)), 5242880),
            format!("-{}", "9".repeat(100))
        );
    }

    // ---- presigning -----------------------------------------------------------------

    fn test_storage() -> StorageSettings {
        StorageSettings {
            use_minio: true,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 3600,
        }
    }

    fn test_now() -> DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("fixed time")
            .with_timezone(&Utc)
    }

    #[test]
    fn presigned_post_matches_botocore() {
        let post = presigned_post(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/ab12-shot.png",
            "image/png",
            "10",
            &test_now(),
        );
        let fields = post["fields"].as_object().expect("fields object");
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "policy",
                "x-amz-signature"
            ],
        );
        assert_eq!(post["url"], "http://127.0.0.1:8486/uploads");
        // The nine policy conditions, in botocore's order (cross-checked
        // against a live `generate_presigned_post` capture).
        let policy_b64 = fields["policy"].as_str().expect("policy");
        let policy_json = String::from_utf8(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, policy_b64)
                .expect("policy b64"),
        )
        .expect("policy utf8");
        let policy: Value = serde_json::from_str(&policy_json).expect("policy json");
        assert_eq!(policy["expiration"], "2026-09-28T13:00:00Z");
        assert_eq!(
            policy["conditions"],
            serde_json::json!([
                {"bucket": "uploads"},
                ["content-length-range", 1, 10],
                {"Content-Type": "image/png"},
                {"key": "ws-id/ab12-shot.png"},
                {"bucket": "uploads"},
                {"key": "ws-id/ab12-shot.png"},
                {"x-amz-algorithm": "AWS4-HMAC-SHA256"},
                {"x-amz-credential": "access-key/20260928/us-east-1/s3/aws4_request"},
                {"x-amz-date": "20260928T120000Z"},
            ])
        );
        // Self-consistency: the signature covers the policy bytes.
        let expected = hex(&hmac_sha256(
            &signing_key("secret-key", "20260928", "us-east-1"),
            policy_b64.as_bytes(),
        ));
        assert_eq!(
            fields["x-amz-signature"].as_str().expect("signature"),
            expected
        );
    }

    #[test]
    fn presigned_get_signs_attachment_disposition() {
        let url = presigned_get_url(
            &test_storage(),
            "http",
            "127.0.0.1:8486",
            "ws-id/ab12-shot.png",
            Filename::Name("shot.png".to_owned()),
            &test_now(),
        );
        assert!(
            url.starts_with("http://127.0.0.1:8486/uploads/ws-id/ab12-shot.png?"),
            "{url}"
        );
        assert!(url.contains("response-content-disposition=attachment"));
        assert!(url.contains("shot.png"));
        assert!(url.contains("X-Amz-Expires=3600"));
        let (query, signature) = url
            .split_once('?')
            .expect("query")
            .1
            .rsplit_once("X-Amz-Signature=")
            .expect("signature");
        let query = query.strip_suffix('&').expect("trailing amp");
        let scope = "20260928/us-east-1/s3/aws4_request";
        let canonical = format!(
            "GET\n/uploads/ws-id/ab12-shot.png\n{query}\nhost:127.0.0.1:8486\n\nhost\nUNSIGNED-PAYLOAD"
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n20260928T120000Z\n{scope}\n{}",
            sha256_hex(canonical.as_bytes())
        );
        let expected = hex(&hmac_sha256(
            &signing_key("secret-key", "20260928", "us-east-1"),
            to_sign.as_bytes(),
        ));
        assert_eq!(signature, expected);
    }

    #[test]
    fn disposition_shapes_match_storage_py() {
        assert_eq!(
            disposition_value(&Filename::Name("a b.png".to_owned())),
            "attachment; filename*=UTF-8''a%20b.png"
        );
        // `quote()` keeps `/` bare (its default `safe`).
        assert_eq!(
            disposition_value(&Filename::Name("a/b.png".to_owned())),
            "attachment; filename*=UTF-8''a/b.png"
        );
        assert_eq!(disposition_value(&Filename::Bare), "attachment");
        let first = disposition_value(&Filename::FreshHex);
        let second = disposition_value(&Filename::FreshHex);
        assert_ne!(first, second);
        for value in [first, second] {
            let name = value
                .strip_prefix("attachment; filename*=UTF-8''")
                .expect("prefix");
            assert_eq!(name.len(), 32);
            assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn endpoint_parts_cover_minio_endpoint_and_aws() {
        let minio = test_storage();
        assert_eq!(
            endpoint_parts(&minio, "http", "h:9"),
            ("http://h:9".to_owned(), "h:9".to_owned())
        );
        let mut custom = test_storage();
        custom.use_minio = false;
        custom.endpoint_url = Some("https://s3.example.com/prefix/".to_owned());
        assert_eq!(
            endpoint_parts(&custom, "http", "h:9"),
            (
                "https://s3.example.com/prefix".to_owned(),
                "s3.example.com".to_owned()
            )
        );
        let mut aws = test_storage();
        aws.use_minio = false;
        assert_eq!(
            endpoint_parts(&aws, "http", "h:9"),
            (
                "https://uploads.s3.us-east-1.amazonaws.com".to_owned(),
                "uploads.s3.us-east-1.amazonaws.com".to_owned()
            )
        );
    }

    #[test]
    fn py_json_string_matches_ensure_ascii() {
        assert_eq!(py_json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(py_json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(py_json_string("\u{0}"), "\"\\u0000\"");
        assert_eq!(py_json_string("é"), "\"\\u00e9\"");
        assert_eq!(py_json_string("𝄞"), "\"\\ud834\\udd1e\"");
    }

    // ---- enqueue payloads --------------------------------------------------------------

    #[test]
    fn issue_activity_message_carries_nine_kwargs() {
        let issue = Uuid::parse_str("33333333-3333-4333-8333-333333333333").expect("uuid");
        let actor = Uuid::parse_str("44444444-4444-4444-8444-444444444444").expect("uuid");
        let project = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid");
        let message = issue_activity_message(
            "attachment.activity.created",
            Some("{\"id\": 1}".to_owned()),
            &issue,
            &actor,
            &project,
            1759370000,
            "https://app.example",
        );
        assert_eq!(message.task, ISSUE_ACTIVITY_TASK);
        assert!(message.args.is_empty());
        let keys: Vec<&str> = message.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
                "origin"
            ]
        );
        assert_eq!(message.kwargs["type"], "attachment.activity.created");
        assert_eq!(message.kwargs["current_instance"], "{\"id\": 1}");
        assert_eq!(message.kwargs["epoch"], 1759370000);
        // The delete arm nulls the instance.
        let deleted = issue_activity_message(
            "attachment.activity.deleted",
            None,
            &issue,
            &actor,
            &project,
            1,
            "o",
        );
        assert_eq!(deleted.kwargs["current_instance"], Value::Null);
        // No `subscriber` / `intake` kwargs (defaults apply).
        assert_eq!(message.kwargs.len(), 9);
    }

    #[test]
    fn metadata_and_soft_delete_messages_match_delay_calls() {
        let asset = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("uuid");
        let meta = asset_metadata_message(&asset);
        assert_eq!(
            meta.task,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(meta.args, vec![Value::String(asset.to_string())]);
        assert!(meta.kwargs.is_empty());
        let soft = label_soft_delete_message(&asset);
        assert_eq!(
            soft.task,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
        assert_eq!(
            soft.args,
            vec![
                Value::String("db".to_owned()),
                Value::String("label".to_owned()),
                Value::String(asset.to_string()),
                Value::Null,
            ]
        );
        assert!(soft.kwargs.is_empty());
    }

    // ---- misc pure helpers ------------------------------------------------------------------

    #[test]
    fn redis_patterns_cover_both_invalidate_arms() {
        assert_eq!(
            labels_cache_pattern("acme", true),
            ":1:*/api/workspaces/acme/labels/*"
        );
        assert_eq!(
            labels_cache_pattern("acme", false),
            ":1:/api/workspaces/acme/labels/"
        );
    }

    #[test]
    fn scheme_of_prefers_forwarded_proto() {
        let mut headers = HeaderMap::new();
        assert_eq!(scheme_of(&headers), "http");
        headers.insert("x-forwarded-proto", "https".parse().expect("header"));
        assert_eq!(scheme_of(&headers), "https");
        headers.insert("x-forwarded-proto", "HTTPS, http".parse().expect("header"));
        assert_eq!(scheme_of(&headers), "https");
        headers.insert("x-forwarded-proto", "gopher".parse().expect("header"));
        assert_eq!(scheme_of(&headers), "http");
    }

    #[test]
    fn allowlist_covers_contract_types() {
        for mime in [
            "image/png",
            "text/plain",
            "application/pdf",
            "video/mp4",
            "application/zip",
        ] {
            assert!(ATTACHMENT_MIME_TYPES.contains(&mime), "{mime}");
        }
        assert!(!ATTACHMENT_MIME_TYPES.contains(&"application/x-empty"));
        assert!(!ATTACHMENT_MIME_TYPES.contains(&""));
    }
}
