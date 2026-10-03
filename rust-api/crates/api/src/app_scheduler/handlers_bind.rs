//! Project scheduler-binding endpoints (D-36, stage 5, PIDASHCONV-634).
//!
//! Ports `ProjectSchedulerBindingListEndpoint` (GET list, POST install)
//! and `ProjectSchedulerBindingDetailEndpoint` (GET, PATCH, DELETE)
//! (`apps/api/pi_dash/app/views/scheduler/views.py:163-291`; routes
//! `apps/api/pi_dash/app/urls/scheduler.py:29-38`).
//!
//! Fixture: `rust-api/fixtures/app_scheduler/handlers/binding_io.golden.json`
//! (F36-11); input contracts F36-02 (shapes), F36-03 (validation), F36-05
//! (SQL), F36-09 (gates) are owned by their own sub-issues and already
//! green. Trace: `rust-api/fixtures/app_scheduler/TRACE.md`.
//!
//! Layering: this file wires the merged layers and adds no new logic —
//! gates from [`super::gate`], SQL + `next_run_at` decisions from
//! `pidash_services::app_scheduler::queries`, shapes + validation from
//! `pidash_services::app_scheduler::shape` — and supplies the two
//! jobs-backed closures those layers take as parameters: `rrule_validator`
//! over `pidash_jobs::tasks_ticker::rrule::validate_rrule_string` (the
//! error mapped to its message string) into shape validation, and
//! `next_fire_for_binding` (jobs `coerce_iso_datetimes` +
//! `next_fire_from_rrule`) into the queries install/patch writes. The api
//! crate already depends on both services and jobs, so the handlers are
//! the seam (a services→jobs call would be a crate cycle).
//!
//! Gate order (preserved): URL resolution (`<uuid:binding_id>` mismatch
//! proxies to Django's resolver 404, the `app_pages` precedent),
//! `BaseAPIView.initial` project rewrite (authenticated only) and session
//! auth, then the per-route gate ([`super::gate`]), then the flag guard,
//! then the view body. Anonymous callers 401 before any gate.
//!
//! Ported bugs and quirks (translate, don't redesign — also in the PR):
//!
//! * QUIRK-install-nondict-500 (`views.py:193`): install reads
//!   `request.data.get("scheduler")` before the serializer runs, so a
//!   non-object JSON body raises `AttributeError` → 500. PATCH feeds the
//!   body straight to the serializer → 400 `non_field_errors` instead.
//! * QUIRK-stale-pod-name (`serializers/scheduler.py:130` + F36-05 R1):
//!   object dereferences (the list join AND the detail/install/PATCH
//!   `binding.pod` traversal, which uses the unfiltered `_base_manager`)
//!   render a soft-deleted pod's stale `pod_name`; only the validation
//!   queryset (`Pod.objects`, the default manager) excludes tombstones
//!   (verified live against Django — an early cut rendered null on
//!   detail here).
//! * QUIRK-body-project-discarded (`views.py:204-212`): the body `project`
//!   must exist (field validation) but `save()` pins the URL project; a
//!   mismatched body project that passes the unique check `IntegrityError`s
//!   at save → 400 `{"error":"The payload is not valid"}`.
//! * QUIRK-project-required (DRF `get_uniqueness_extra_kwargs` forces
//!   `required=True` on the unique pair's fields): the body must carry
//!   `project` even though save pins the URL one — except explicit null
//!   passes (the unique check skips `None` values). The missing-key error
//!   is field-level, collected with every other field error.
//! * QUIRK-patch-unique-before-lock (DRF `run_validation`): the unique
//!   check runs before `validate()`, so a conflicting repoint 400s with
//!   the unique-set message instead of the scheduler/project lock message.
//! * QUIRK-next-run-asymmetry (F36-05 BR6, `views.py:218` vs `:272`):
//!   install writes the computed fire only when non-null AND different
//!   from stored; patch writes whenever non-null, recomputing on trigger
//!   key *presence* (`PATCH {rrule: <same>}` recomputes).
//! * QUIRK-tzid-500 (`serializers/scheduler.py:206-216`): malformed
//!   `ZoneInfo` keys (absolute, non-normalized, escaping) raise bare
//!   `ValueError`, uncaught → 500.
//! * QUIRK-empty-body (DRF `Request._parse`): content-length 0 validates
//!   as `{}` with the body ignored; a JSON body of only whitespace 400s
//!   with the exact `json.load` position message.
//! * QUIRK-orphan-run (`serializers/scheduler.py:175-179`): a binding whose
//!   `last_run_id` dangles (unreachable except races — the FK guards it)
//!   500s the list (`None.status` → `AttributeError`) but 404s the detail
//!   (`DoesNotExist` → `handle_exception`).
//! * QUIRK-install-double-now / patch-triple-now / uninstall-double-now:
//!   every `save()` samples `timezone.now()` afresh (`views.py:207-220`,
//!   `:263-274`, `:290`), so the INSERT, the recompute anchor, and each
//!   write-back carry distinct samples — ported as separate calls.
//!
//! Fixture erratum (F36-03 `pod_field_level`, proven against DRF 3.15.2 +
//! Django 4.2.30): the fixture claims `pod: "bogus"` escapes `is_valid`
//! into `handle_exception` (`{"error":"Please provide valid detail"}`),
//! but `Serializer.to_internal_value` catches the Django `ValidationError`
//! per-field → `{"pod": ["“bogus” is not a valid UUID."]}` (curly quotes
//! from `UUIDField.error_messages`). The escape only happens at the
//! view-level scheduler guard. True behavior is ported here.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::tasks_ticker::models::{scheduler, scheduler_binding};
use pidash_services::app_scheduler::{queries, shape};

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gate;
use super::{json_response, pool_of, resolve_project_id, session_authenticated};

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// `get_object_or_404(SchedulerBinding, ...)` miss (install never renders
/// this; detail GET/PATCH/uninstall do).
pub const NOT_FOUND_BINDING_BODY: &str =
    r#"{"detail":"No SchedulerBinding matches the given query."}"#;
/// `get_object_or_404(Project, ...)` miss on install (`views.py:192`;
/// reachable only when the row vanishes between the rewrite and the view).
pub const NOT_FOUND_PROJECT_BODY: &str = r#"{"detail":"No Project matches the given query."}"#;
/// Scheduler-guard miss on install (`views.py:194-199`): disabled,
/// foreign-workspace, soft-deleted, or absent-from-body id.
pub const NOT_FOUND_SCHEDULER_BODY: &str = r#"{"detail":"No Scheduler matches the given query."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (404): the orphaned
/// `last_run` dereference on detail paths (QUIRK-orphan-run).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s `IntegrityError` branch (400): the body/URL
/// project-mismatch unique violation at install save.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s Django-`ValidationError` branch (400): a malformed
/// (non-UUID) scheduler id at the install guard.
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// DRF `UniqueTogetherValidator` failure (install + PATCH).
pub const UNIQUE_BODY: &str =
    r#"{"non_field_errors":["The fields scheduler, project must make a unique set."]}"#;

// ---------------------------------------------------------------------------
// Denial
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body. Gate denials flow through
/// [`gate::Denial`]'s own renderer; the project rewrite answers directly.
#[derive(Debug)]
pub enum Denial {
    /// 404, `get_object_or_404` miss (binding/project/scheduler bodies).
    NotFound(&'static str),
    /// 404, `ObjectDoesNotExist` branch (orphaned `last_run` dereference).
    ObjectNotFound,
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// 400, `{"detail": ...}` (request-body parse errors).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline denials).
    BadError(String),
    /// 400, Django-`ValidationError` branch (malformed guard id).
    ValidDetail,
    /// 400, `IntegrityError` branch (unique violation at save).
    InvalidPayload,
    /// 500, generic branch (incl. QUIRK-install-nondict-500).
    ServerError,
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::NotFound(body) => (StatusCode::NOT_FOUND, (*body).to_owned()),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ValidDetail => (StatusCode::BAD_REQUEST, VALID_DETAIL_BODY.to_owned()),
            Denial::InvalidPayload => (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                gate::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        json_response(status, body)
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET .../scheduler-bindings/` (`views.py:172-186`): project + slug
/// scope, `select_related` joins, newest first, 200 array.
pub async fn binding_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (resolved, project_id) = match resolve_binding_request(
        pool,
        &state,
        &slug,
        &project_raw,
        gate::Gate::ProjectOpen,
        extension,
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return response,
    };
    let rows = match fetch_binding_list(pool, &project_id, &slug).await {
        Ok(rows) => rows,
        Err(denial) => return denial.into_response(),
    };
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match render_binding_row(row, &resolved.timezone) {
            Ok(body) => out.push_str(&body),
            Err(denial) => return denial.into_response(),
        }
    }
    out.push(']');
    json_response(StatusCode::OK, out)
}

/// `POST .../scheduler-bindings/` (`views.py:189-224`): project + scheduler
/// lookups, validation with project context, save with pinned
/// scheduler/project/workspace/actor, `next_run_at` compute + surface, 201.
pub async fn binding_install(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (resolved, project_id) = match resolve_binding_request(
        pool,
        &state,
        &slug,
        &project_raw,
        gate::Gate::ProjectAdmin,
        extension,
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return response,
    };
    // BR3 project lookup (`:192`; post-rewrite it only misses on races).
    let project = match fetch_install_project(pool, &project_id, &slug).await {
        Ok(Some(project)) => project,
        Ok(None) => return Denial::NotFound(NOT_FOUND_PROJECT_BODY).into_response(),
        Err(denial) => return denial.into_response(),
    };
    // `request.data` is first touched here (`:193`): the content-type gate
    // runs after the project lookup, exactly like Django's lazy `_parse`.
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    // QUIRK-install-nondict-500: `request.data.get` on a non-object body
    // raises `AttributeError` before the serializer ever runs.
    let Value::Object(body) = &data else {
        return Denial::ServerError.into_response();
    };
    // BR3 scheduler guard (`:194-199`): before validation, so a bad id
    // 404s (or 400s when malformed) instead of 400ing as a field error.
    let scheduler_id = match guard_scheduler_id(body.get("scheduler")) {
        Ok(Some(id)) => id,
        Ok(None) => return Denial::NotFound(NOT_FOUND_SCHEDULER_BODY).into_response(),
        Err(denial) => return denial.into_response(),
    };
    let sched = match fetch_install_scheduler(pool, &scheduler_id, &project.workspace_id).await {
        Ok(Some(sched)) => sched,
        Ok(None) => return Denial::NotFound(NOT_FOUND_SCHEDULER_BODY).into_response(),
        Err(denial) => return denial.into_response(),
    };
    // Serializer validation with the view-supplied project context.
    let attrs = match validate_binding_fields(pool, body, None, &resolved.timezone).await {
        Ok(attrs) => attrs,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = enforce_unique_install(pool, &scheduler_id, attrs.project.flatten()).await
    {
        return denial.into_response();
    }
    let install = InstallAttrs {
        scheduler_id,
        project_id,
        workspace_id: project.workspace_id,
        actor_id: resolved.user_id,
        attrs,
    };
    if let Err(denial) = validate_install_cross(&install, &project_id) {
        return denial.into_response();
    }
    let mut saved = match insert_binding(pool, &install).await {
        Ok(saved) => saved,
        Err(denial) => return denial.into_response(),
    };
    // Install recompute (`:216-220`): always compute after save, write back
    // only when non-null AND different from stored (NULL here).
    let now = Utc::now();
    let bundle = saved.recompute_bundle();
    let write =
        queries::install_next_run_at(&bundle, saved.row.next_run_at, now, next_fire_for_binding);
    if let Some(computed) = write {
        let stamp = Utc::now();
        if let Err(denial) = write_next_run_at(pool, &saved.row.id, &computed, &stamp).await {
            return denial.into_response();
        }
        saved.row.next_run_at = Some(computed);
        saved.row.updated_at = stamp;
    }
    let body = render_saved_binding(&saved, &sched, &resolved.timezone);
    json_response(StatusCode::CREATED, body)
}

/// `GET .../scheduler-bindings/<uuid>/` (`views.py:237-249`): 200 or 404.
pub async fn binding_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, binding_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(binding_id) = binding_raw.parse::<Uuid>() else {
        // URL resolution precedes auth: a non-UUID id proxies to Django's
        // resolver 404 (the `app_pages` precedent).
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (resolved, project_id) = match resolve_binding_request(
        pool,
        &state,
        &slug,
        &project_raw,
        gate::Gate::ProjectOpen,
        extension,
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return response,
    };
    let row = match fetch_binding_detail(pool, &binding_id, &project_id, &slug).await {
        Ok(Some(row)) => row,
        Ok(None) => return Denial::NotFound(NOT_FOUND_BINDING_BODY).into_response(),
        Err(denial) => return denial.into_response(),
    };
    match render_detail_binding(pool, &row, &resolved.timezone).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH .../scheduler-bindings/<uuid>/` (`views.py:252-278`): partial
/// validate, save, RRULE-bundle-key recompute, 200.
pub async fn binding_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, binding_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(binding_id) = binding_raw.parse::<Uuid>() else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (resolved, project_id) = match resolve_binding_request(
        pool,
        &state,
        &slug,
        &project_raw,
        gate::Gate::ProjectAdmin,
        extension,
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return response,
    };
    // The binding lookup (`:255-260`) precedes `request.data` access, so a
    // missing row 404s before any body parse error can fire.
    let current = match fetch_binding_detail(pool, &binding_id, &project_id, &slug).await {
        Ok(Some(row)) => row,
        Ok(None) => return Denial::NotFound(NOT_FOUND_BINDING_BODY).into_response(),
        Err(denial) => return denial.into_response(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    // Unlike install, the body feeds straight into the serializer: a
    // JSON `null` fails `validate_empty_values` with the serializer's
    // `null` message ("No data provided" — serializers override the
    // field-level "This field may not be null."), and any other
    // non-object body 400s with the standard `non_field_errors` message.
    let Value::Object(body) = &data else {
        let message = match &data {
            Value::Null => "No data provided".to_owned(),
            _ => format!(
                "Invalid data. Expected a dictionary, but got {}.",
                shape::json_type_name(&data)
            ),
        };
        let mut errors = Map::new();
        errors.insert(
            "non_field_errors".to_owned(),
            Value::Array(vec![Value::String(message)]),
        );
        return Denial::BadJson(Value::Object(errors)).into_response();
    };
    let attrs = match validate_binding_fields(pool, body, Some(&current), &resolved.timezone).await
    {
        Ok(attrs) => attrs,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = enforce_unique_patch(pool, &binding_id, &current, &attrs).await {
        return denial.into_response();
    }
    if let Err(denial) = validate_patch_cross(&current, &attrs) {
        return denial.into_response();
    }
    let stamp = Utc::now();
    if let Err(denial) = update_binding(
        pool,
        &binding_id,
        &current,
        &attrs,
        &resolved.user_id,
        &stamp,
    )
    .await
    {
        return denial.into_response();
    }
    // The response re-renders the in-memory row: saved values plus the
    // conditional recompute below.
    let mut row = patched_row(&current, &attrs, &resolved.user_id, &stamp);
    if queries::patch_triggers_recompute(&data) {
        // `:270`: refresh from the DB before computing, so the expansion
        // sees the just-saved row.
        let refreshed = match fetch_binding_detail(pool, &binding_id, &project_id, &slug).await {
            Ok(Some(refreshed)) => refreshed,
            // `refresh_from_db` on a vanished row raises bare
            // `DoesNotExist` (not `get_object_or_404`) → the
            // `ObjectDoesNotExist` 404 branch. Unreachable except races.
            Ok(None) => return Denial::ObjectNotFound.into_response(),
            Err(denial) => return denial.into_response(),
        };
        row = refreshed;
        let bundle = queries::RruleBundle {
            dtstart: row.dtstart,
            rrule: row.rrule.as_str(),
            tzid: row.tzid.as_str(),
            rdates: &row.rdates,
            exdates: &row.exdates,
        };
        let now = Utc::now();
        if let Some(computed) = queries::patch_next_run_at(&bundle, now, next_fire_for_binding) {
            let stamp = Utc::now();
            if let Err(denial) = write_next_run_at(pool, &row.id, &computed, &stamp).await {
                return denial.into_response();
            }
            row.next_run_at = Some(computed);
            row.updated_at = stamp;
        }
    }
    match render_detail_binding(pool, &row, &resolved.timezone).await {
        Ok(rendered) => json_response(StatusCode::OK, rendered),
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE .../scheduler-bindings/<uuid>/` (`views.py:281-291`): uninstall
/// (soft-delete), 204 with an empty body.
pub async fn binding_uninstall(
    State(state): State<AppState>,
    Path((slug, project_raw, binding_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(binding_id) = binding_raw.parse::<Uuid>() else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let (resolved, project_id) = match resolve_binding_request(
        pool,
        &state,
        &slug,
        &project_raw,
        gate::Gate::ProjectAdmin,
        extension,
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return response,
    };
    let exists = match fetch_binding_detail(pool, &binding_id, &project_id, &slug).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    if exists.is_none() {
        return Denial::NotFound(NOT_FOUND_BINDING_BODY).into_response();
    }
    // BR8: `binding.delete()` samples `now()` twice (ported quirk 5).
    let now = Utc::now();
    let now2 = Utc::now();
    if let Err(denial) = uninstall_binding(pool, &binding_id, &resolved.user_id, &now, &now2).await
    {
        return denial.into_response();
    }
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 response")
}

// ---------------------------------------------------------------------------
// Request context: pool → rewrite → gate → flag
// ---------------------------------------------------------------------------

/// Resolve the authenticated, authorized request context: the project
/// rewrite for authenticated callers (anonymous callers 401 inside the
/// gate instead of 404ing here), then [`gate::resolve_gate`], then the
/// flag guard. Returns the gate context plus the rewritten project id.
#[allow(clippy::result_large_err)]
async fn resolve_binding_request(
    pool: &PgPool,
    state: &AppState,
    slug: &str,
    project_raw: &str,
    kind: gate::Gate,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(gate::ResolvedGate, Uuid), Response> {
    let project_id = if session_authenticated(&extension) {
        Some(resolve_project_id(pool, slug, project_raw).await?)
    } else {
        // Anonymous: the rewrite is skipped (Django's `initial`); the
        // parsed id below is only a placeholder — `resolve_gate` 401s
        // before the tenant fetch can read it.
        project_raw.parse::<Uuid>().ok()
    };
    let resolved = gate::resolve_gate(state, &kind, slug, project_id.as_ref(), extension)
        .await
        .map_err(|denial| denial.into_response())?;
    gate::ensure_feature_enabled(state.settings()).map_err(|denial| denial.into_response())?;
    // Post-gate the caller is authenticated, so the rewrite above ran and
    // `project_id` is `Some` (a miss 404d there).
    match project_id {
        Some(id) => Ok((resolved, id)),
        None => Err(Denial::ServerError.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Request data (`request.data`, DRF `Request._parse`)
// ---------------------------------------------------------------------------

/// Read `request.data` for the write endpoints, in the exact order Django
/// parses it (lazily, at first access — the handlers call this after their
/// lookups, so lookup 404s precede body errors).
///
/// * Content-length 0 → `{}` with the body ignored (`stream is None`).
/// * `application/json` → parse; whitespace-only input 400s with the exact
///   `json.load` position message, other malformed input 400s with the
///   merged-handlers generic message (unpinned edge — CPython's error
///   texts are not reproducible from serde).
/// * Form/multipart/unknown media types → proxy to Django, which renders
///   the exact form behavior / 415 there. Safe: no write precedes
///   `request.data` access on either path.
#[allow(clippy::result_large_err)]
async fn read_request_data(state: &AppState, req: Request) -> Result<Value, Response> {
    let content_length = req
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(Value::Object(Map::new()));
    }
    let raw_type = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // `parse_header_parameters`: the main type lowercases; parameters are
    // ignored for parser selection (`_MediaType.match`).
    let main = raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if main != "application/json" {
        return Err(crate::edge::proxy(State(state.clone()), req).await);
    }
    let (_parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| Denial::ServerError.into_response())?;
    if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        // `json.load` of whitespace-only input: `Expecting value` at the
        // first unconsumed position (line 1, column `n + 1`, char `n`).
        let position = bytes.len();
        return Err(Denial::BadDetail(format!(
            "JSON parse error - Expecting value: line 1 column {} (char {})",
            position + 1,
            position
        ))
        .into_response());
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => Err(Denial::BadDetail("JSON parse error".to_owned()).into_response()),
    }
}

// ---------------------------------------------------------------------------
// Python-compat value helpers (DRF 3.15.2 + Django 4.2 field semantics)
// ---------------------------------------------------------------------------

/// Python `str.strip()` with no args: Unicode whitespace plus
/// `\x1c`–`\x1f` (same predicate as the shape layer's private `py_strip`;
/// services is read-only, so it is mirrored here, not imported).
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python `str(value)` over a JSON value, for the `% (value)s`
/// interpolations in the UUID / choice messages. Strings pass through
/// verbatim; integers render decimal; floats echo their input text (the
/// api crate builds serde_json with `arbitrary_precision`, so
/// `Number::to_string` echoes the source spelling — CPython would print
/// `1e+16` where an input spelled `1e16`, an unpinned edge); containers
/// render with Python element spelling (`True`/`False`/`None`, single
/// quotes — approximated for exotic strings, see `py_repr_string`).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_string(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr(value)` over a JSON value (elements of `py_str`
/// containers; numbers/bools/`None` spell like `py_str`).
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_string(text),
        Value::Array(_) | Value::Object(_) => py_str(value),
        _ => py_str(value),
    }
}

/// Python `repr` of a string: single quotes unless the value contains `'`
/// but no `"` (then double quotes); short escapes for backslash and
/// `\n`/`\r`/`\t`; other controls as `\x`/`\u`/`\U`. Non-ASCII printables
/// pass through (CPython `repr` would escape some of them — an unpinned
/// edge; only container-element messages reach here).
fn py_repr_string(value: &str) -> String {
    let double_quoted = value.contains('\'') && !value.contains('"');
    let mut out = String::with_capacity(value.len() + 2);
    out.push(if double_quoted { '"' } else { '\'' });
    for c in value.chars() {
        match c {
            '\'' if !double_quoted => out.push_str("\\'"),
            '"' if double_quoted => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let code = c as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(if double_quoted { '"' } else { '\'' });
    out
}

/// `int(text, 16)` (CPython `long_from_string`): surrounding Unicode
/// whitespace stripped, one optional sign, hex digits with `_` separators
/// allowed strictly between digits, value in `0..2^128`. This is the tail
/// of `uuid.UUID(hex=...)` after its prefix/brace/dash surgery.
fn int_hex16(text: &str) -> Option<u128> {
    let trimmed = text.trim_matches(|c: char| {
        c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c) || c == '\u{85}'
    });
    let (negative, digits) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: u128 = 0;
    let mut prev_underscore = true; // leading `_` is invalid
    for byte in digits.bytes() {
        if byte == b'_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let digit = (byte as char).to_digit(16)?;
        prev_underscore = false;
        value = value.checked_mul(16)?.checked_add(u128::from(digit))?;
    }
    if prev_underscore {
        return None; // trailing `_` is invalid
    }
    if negative {
        return None; // range check `0 <= v` fails
    }
    Some(value)
}

/// `uuid.UUID(hex=...)` (CPython `uuid.py`): strip every `urn:`/`uuid:`
/// (all occurrences, case-sensitive), strip `{}` ends, drop `-`, require
/// 32 characters, then `int(hex, 16)` with its whitespace/sign/underscore
/// leniency and the `0..2^128` range check.
fn uuid_from_hex_forms(text: &str) -> Option<Uuid> {
    let no_prefix = text.replace("urn:", "").replace("uuid:", "");
    // `str.strip('{}')` strips ALL leading/trailing braces, not one pair.
    let stripped = no_prefix.trim_matches(['{', '}']);
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.len() != 32 {
        return None;
    }
    int_hex16(&compact).map(Uuid::from_u128)
}

/// `UUIDField.to_python` (Django 4.2 `fields/__init__.py`): integers take
/// the `int=` form (range-checked), everything else the `hex=` form.
/// `None` is handled by the caller (nullability); bools take the `int=`
/// form here — the related-field `isinstance(data, bool)` guard that
/// rejects them lives one level up, in [`parse_pk_value`], and does NOT
/// apply to the ORM-level guard lookup.
fn django_uuid_to_python(value: &Value) -> Option<Uuid> {
    match value {
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                if signed < 0 {
                    return None;
                }
                return Some(Uuid::from_u128(signed as u128));
            }
            if let Some(unsigned) = number.as_u64() {
                return Some(Uuid::from_u128(u128::from(unsigned)));
            }
            // Out-of-`u64` integers (arbitrary_precision keeps the text):
            // valid iff they fit `u128` (a leading `-` fails the parse).
            number.to_string().parse::<u128>().ok().map(Uuid::from_u128)
        }
        Value::String(text) => uuid_from_hex_forms(text),
        // `uuid.UUID(hex=<float/dict/list>)` raises `AttributeError`
        // (no `.replace`), which `to_python` converts to the same
        // `ValidationError` as a bad string.
        _ => None,
    }
}

/// `PrimaryKeyRelatedField.to_internal_value` (DRF `relations.py`):
/// bools fail `incorrect_type` up front; otherwise the Django
/// `to_python` above decides, and a failure renders the curly-quote
/// `is not a valid UUID` message as a field error (the serializer's
/// per-field `DjangoValidationError` catch — proven, not the
/// `handle_exception` path the F36-03 note claims).
fn parse_pk_value(value: &Value) -> Result<Uuid, String> {
    if value.is_boolean() {
        return Err(format!(
            "Incorrect type. Expected pk value, received {}.",
            shape::json_type_name(value)
        ));
    }
    django_uuid_to_python(value)
        .ok_or_else(|| format!("\u{201c}{}\u{201d} is not a valid UUID.", py_str(value)))
}

/// The install scheduler-guard id (`views.py:193-199`): `request.data.get`
/// at ORM level — absent/null means `pk=None` (matches no row → 404), and
/// there is NO `incorrect_type` guard (bools take the `int=` form, floats
/// and containers fail `to_python`). A `to_python` failure escapes the
/// view into `handle_exception` → 400 [`VALID_DETAIL_BODY`].
fn guard_scheduler_id(value: Option<&Value>) -> Result<Option<Uuid>, Denial> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(true)) => Ok(Some(Uuid::from_u128(1))),
        Some(Value::Bool(false)) => Ok(Some(Uuid::from_u128(0))),
        Some(other) => match django_uuid_to_python(other) {
            Some(id) => Ok(Some(id)),
            None => Err(Denial::ValidDetail),
        },
    }
}

// ---------------------------------------------------------------------------
// DRF field input (field-level `to_internal_value` + validators)
// ---------------------------------------------------------------------------

/// `CharField` input (`fields.py`): the blank gate on the raw value, then
/// numeric coercion via `str()` (`bool`/composites fail `invalid`), then
/// `trim_whitespace`, then the field validators in order (`max_length`
/// first when present, then the null-characters gate). Surrogate
/// validation is unreachable over JSON (serde rejects lone surrogates;
/// valid UTF-8 has none). Returns the coerced value or every message.
fn validate_char_input(
    value: &Value,
    allow_blank: bool,
    max_length: Option<usize>,
) -> Result<String, Vec<String>> {
    // The blank gate (`CharField.run_validation`): `data == ''` or the
    // stripped `str(data)` is empty. `str()` of a JSON scalar is its
    // `py_str`; containers stringify too (`str(data)` never fails here).
    let blank = match value {
        Value::String(text) => text.is_empty() || py_strip(text).is_empty(),
        _ => py_strip(&py_str(value)).is_empty(),
    };
    if blank {
        if allow_blank {
            return Ok(String::new());
        }
        return Err(vec!["This field may not be blank.".to_owned()]);
    }
    // `to_internal_value`: numerics coerce, bools and composites fail.
    let coerced = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(vec!["Not a valid string.".to_owned()]),
    };
    // `trim_whitespace=True` (the default on all three binding text fields).
    let trimmed = py_strip(&coerced).to_owned();
    // Field validators, in order, accumulating every failure.
    let mut errors = Vec::new();
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            errors.push(format!(
                "Ensure this field has no more than {max} characters."
            ));
        }
    }
    if trimmed.contains('\u{0}') {
        errors.push("Null characters are not allowed.".to_owned());
    }
    if errors.is_empty() {
        Ok(trimmed)
    } else {
        Err(errors)
    }
}

/// DRF `BooleanField` input: the `TRUE_VALUES` / `FALSE_VALUES` sets with
/// case-insensitive string matching (`1`/`1.0` are true — `1.0 == 1` in
/// the set lookup; `0`/`0.0` are false). `None` never reaches here
/// (`validate_empty_values` fails `null` first); `''`/`'null'` fail
/// `invalid` the same way (the `NULL_VALUES` branch needs `allow_null`).
fn validate_boolean_input(value: &Value) -> Result<bool, String> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                match int {
                    1 => return Ok(true),
                    0 => return Ok(false),
                    _ => return Err(INVALID.to_owned()),
                }
            }
            if let Some(uint) = number.as_u64() {
                match uint {
                    1 => return Ok(true),
                    0 => return Ok(false),
                    _ => return Err(INVALID.to_owned()),
                }
            }
            match number.as_f64() {
                Some(1.0) => Ok(true),
                Some(0.0) => Ok(false),
                _ => Err(INVALID.to_owned()),
            }
        }
        Value::String(text) => {
            let lowered = text.to_lowercase();
            match lowered.as_str() {
                "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
                "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
                _ => Err(INVALID.to_owned()),
            }
        }
        _ => Err(INVALID.to_owned()),
    }
}

/// DRF `ChoiceField` input for `outcome_mode`: `str(data)` looked up in
/// the three choices, else `"..." is not a valid choice.` with the Python
/// `str()` spelling of the input.
fn validate_outcome_mode_input(value: &Value) -> Result<String, String> {
    const CHOICES: &[&str] = &["create_issue", "apply_fix", "fix_and_review"];
    let text = match value {
        Value::String(text) => text.clone(),
        _ => py_str(value),
    };
    if CHOICES.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(format!("\"{text}\" is not a valid choice."))
    }
}

// ---------------------------------------------------------------------------
// DRF `DateTimeField` input (Django `parse_datetime` + `enforce_timezone`)
// ---------------------------------------------------------------------------
// Mirrors the `app_modules` `resolve_link_datetime_input` precedent
// (Django 4.2 on Python 3.12): `fromisoformat` first (strict, full
// consumption), then the `datetime_re` fallback; aware inputs keep their
// instant, naive inputs attach the request zone (`TimezoneMixin` +
// `USE_TZ`), gaps take the pre-transition offset and folds take fold 0
// (DRF's `valid_datetime` gate is a no-op for zoneinfo zones — verified
// against the shipped DRF by that precedent).

/// DRF `DateTimeField` input for `dtstart`: `Err` is the exact field
/// message (`invalid`, or `overflow` when the aware instant falls outside
/// Python's representable range).
fn validate_dtstart_input(value: &Value, timezone: &Tz) -> Result<DateTime<Utc>, String> {
    use chrono::{Datelike, MappedLocalTime, TimeZone};
    const INVALID: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
    const OVERFLOW: &str = "Datetime value out of range.";
    let Value::String(text) = value else {
        return Err(INVALID.to_owned());
    };
    let Some((naive, offset)) = parse_binding_datetime(text) else {
        return Err(INVALID.to_owned());
    };
    // Python datetimes start at year 1; chrono would also build year 0.
    if naive.date().year() < 1 {
        return Err(INVALID.to_owned());
    }
    match offset {
        Some(offset) => {
            // Aware inputs keep their instant (`astimezone`); an instant
            // outside `0001-01-01..9999-12-31` overflows there instead.
            let instant = naive.and_utc() - offset;
            if !(1..=9999).contains(&instant.date_naive().year()) {
                return Err(OVERFLOW.to_owned());
            }
            Ok(instant)
        }
        // Naive inputs attach the request zone (`get_current_timezone`,
        // activated from `user_timezone` by `TimezoneMixin`).
        None => match timezone.from_local_datetime(&naive) {
            MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
                Ok(local.to_utc())
            }
            MappedLocalTime::None => Ok(pre_transition_instant(timezone, &naive)),
        },
    }
}

/// Instant for a wall time inside a DST gap: the pre-transition offset
/// applied to the wall time (Python's fold-0 `zoneinfo` attach). Walks
/// back in 15-minute steps to the last mappable wall time; falls back to
/// UTC past the cap.
fn pre_transition_instant(timezone: &Tz, naive: &chrono::NaiveDateTime) -> DateTime<Utc> {
    use chrono::{MappedLocalTime, TimeZone};
    let mut probe = *naive;
    for _ in 0..300 {
        probe = match probe.checked_sub_signed(chrono::Duration::minutes(15)) {
            Some(previous) => previous,
            None => break,
        };
        if let MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) =
            timezone.from_local_datetime(&probe)
        {
            let east = local.naive_local() - local.naive_utc();
            return naive.and_utc() - east;
        }
    }
    naive.and_utc()
}

/// Django `parse_datetime` (Django 4.2 on Python 3.12): `fromisoformat`
/// first (strict, full consumption), then the `datetime_re` fallback.
/// Returns the naive wall time plus the fixed UTC offset (`None` when the
/// input carries none). `ValueError`-vs-`None` is wire-invisible (both
/// render the `invalid` message), so both are `None` here.
fn parse_binding_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    parse_fromiso_datetime(text).or_else(|| parse_regex_datetime(text))
}

/// Two ASCII digits as a number.
fn two_digits(text: &str) -> Option<u32> {
    if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// One or two ASCII digits as a number.
fn one_two_digits(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Fractional seconds (`[.,]` + digits) as microseconds: the first six
/// digits, right-padded with zeros. `fromisoformat` takes any digit run;
/// the regex path caps the run before calling.
fn frac_micros(frac: &str) -> u32 {
    let mut digits: String = frac.chars().take(6).collect();
    while digits.len() < 6 {
        digits.push('0');
    }
    digits.parse().unwrap_or(0)
}

/// `fromisoformat` date head: extended `YYYY-MM-DD`, basic `YYYYMMDD`,
/// or ISO week `YYYY-Www[-D]` / `YYYYWww[D]` (day defaults to Monday).
/// Returns the calendar date plus the unconsumed tail.
fn parse_fromiso_date(text: &str) -> Option<(chrono::NaiveDate, &str)> {
    use chrono::Datelike;
    let bytes = text.as_bytes();
    if bytes.len() >= 8 && bytes[4] == b'-' && bytes[5] == b'W' {
        // Extended week date (`YYYY-Www[-D]`, day defaults to Monday).
        let year: i32 = text.get(..4)?.parse().ok()?;
        let week = two_digits(text.get(6..8)?)?;
        let (day, tail) = match text.as_bytes().get(8) {
            Some(b'-') => (text.get(9..10)?.parse::<u32>().ok()?, text.get(10..)?),
            _ => (1, text.get(8..)?),
        };
        if !(1..=7).contains(&day) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        let date = date.checked_add_signed(chrono::Duration::days(i64::from(day - 1)))?;
        if date.iso_week().week() != week {
            return None;
        }
        return Some((date, tail));
    }
    if bytes.len() >= 10 && bytes[4] == b'-' {
        let year: i32 = text.get(..4)?.parse().ok()?;
        let month = two_digits(text.get(5..7)?)?;
        if text.as_bytes().get(7) != Some(&b'-') {
            return None;
        }
        let day = two_digits(text.get(8..10)?)?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, text.get(10..)?));
    }
    if bytes.len() >= 5 && bytes[4] == b'W' {
        // Basic week date.
        let year: i32 = text.get(..4)?.parse().ok()?;
        let week = two_digits(text.get(5..7)?)?;
        let (day, tail) = match text.as_bytes().get(7) {
            Some(b) if b.is_ascii_digit() => (text.get(7..8)?.parse::<u32>().ok()?, text.get(8..)?),
            _ => (1, text.get(7..)?),
        };
        if !(1..=7).contains(&day) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        let date = date.checked_add_signed(chrono::Duration::days(i64::from(day - 1)))?;
        if date.iso_week().week() != week {
            return None;
        }
        return Some((date, tail));
    }
    if bytes.len() >= 8 && bytes[..8].iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text.get(..4)?.parse().ok()?;
        let month = two_digits(text.get(4..6)?)?;
        let day = two_digits(text.get(6..8)?)?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, text.get(8..)?));
    }
    None
}

/// `fromisoformat` time + zone tail (strict two-digit parts): extended
/// `HH[:MM[:SS]]`, basic `HHMM[SS]` / `HHMM` / `HH`, an optional
/// `[.,]`-fraction (any length), and an optional `Z` / numeric offset.
/// Full consumption.
fn parse_fromiso_time(
    date: chrono::NaiveDate,
    text: &str,
) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    // Basic vs extended: a colon after the hour forces extended.
    let (hour, minute, second, tail) = if text.len() >= 3 && text.as_bytes()[2] == b':' {
        let hour = two_digits(text.get(..2)?)?;
        let minute = two_digits(text.get(3..5)?)?;
        let (second, tail) = if text.as_bytes().get(5) == Some(&b':') {
            (two_digits(text.get(6..8)?)?, text.get(8..)?)
        } else {
            (0, text.get(5..)?)
        };
        (hour, minute, second, tail)
    } else {
        let digits: usize = text.bytes().take_while(u8::is_ascii_digit).count();
        if !matches!(digits, 2 | 4 | 6) {
            return None;
        }
        let hour = two_digits(text.get(..2)?)?;
        let minute = if digits >= 4 {
            two_digits(text.get(2..4)?)?
        } else {
            0
        };
        let second = if digits == 6 {
            two_digits(text.get(4..6)?)?
        } else {
            0
        };
        (hour, minute, second, text.get(digits..)?)
    };
    let (micro, tail) = match tail.as_bytes().first() {
        Some(b'.' | b',') => {
            let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
            if frac == 0 {
                return None;
            }
            (frac_micros(tail.get(1..1 + frac)?), tail.get(1 + frac..)?)
        }
        _ => (0, tail),
    };
    let offset = parse_fromiso_offset(tail)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

/// `fromisoformat` zone tail: `Z`, `±HH[:MM[:SS]]`, `±HHMM[SS]` or
/// `±HH`, with an optional `[.,]`-fraction that always means fractional
/// *seconds* — dropped when hour, minute and second are all zero (the
/// `timezone.utc` fast path; verified against CPython 3.12 by the
/// `app_modules` precedent). Components carry no range check; only the
/// total must stay strictly inside a day.
fn parse_fromiso_offset(text: &str) -> Option<Option<chrono::Duration>> {
    if text.is_empty() {
        return Some(None);
    }
    if text == "Z" {
        return Some(Some(chrono::Duration::zero()));
    }
    let (sign, tail) = match text.as_bytes().first() {
        Some(b'+') => (1i64, text.get(1..)?),
        Some(b'-') => (-1i64, text.get(1..)?),
        _ => return None,
    };
    let hours = two_digits(tail.get(..2)?)?;
    let tail = tail.get(2..)?;
    let (minutes, seconds, tail) = if let Some(rest) = tail.strip_prefix(':') {
        let minutes = two_digits(rest.get(..2)?)?;
        let rest = rest.get(2..)?;
        if let Some(rest) = rest.strip_prefix(':') {
            (minutes, two_digits(rest.get(..2)?)?, rest.get(2..)?)
        } else {
            (minutes, 0, rest)
        }
    } else {
        let digits: usize = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits != 0 && digits != 2 && digits != 4 {
            return None;
        }
        let minutes = if digits >= 2 {
            two_digits(tail.get(..2)?)?
        } else {
            0
        };
        let seconds = if digits == 4 {
            two_digits(tail.get(2..4)?)?
        } else {
            0
        };
        (minutes, seconds, tail.get(digits..)?)
    };
    let (frac_us, tail) = match tail.as_bytes().first() {
        Some(b'.' | b',') => {
            let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
            if frac == 0 {
                return None;
            }
            (
                i64::from(frac_micros(tail.get(1..1 + frac)?)),
                tail.get(1 + frac..)?,
            )
        }
        _ => (0, tail),
    };
    if !tail.is_empty() {
        return None;
    }
    let mut total_us =
        (i64::from(hours) * 3600 + i64::from(minutes) * 60 + i64::from(seconds)) * 1_000_000;
    if hours != 0 || minutes != 0 || seconds != 0 {
        total_us += frac_us;
    }
    if total_us.abs() >= 86_400_000_000 {
        return None;
    }
    Some(Some(chrono::Duration::microseconds(sign * total_us)))
}

/// `fromisoformat` whole: date, then end-of-input (midnight) or exactly
/// one separator character — any character, even a digit — plus the time.
fn parse_fromiso_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    let (date, tail) = parse_fromiso_date(text)?;
    if tail.is_empty() {
        return Some((
            chrono::NaiveDateTime::new(date, chrono::NaiveTime::MIN),
            None,
        ));
    }
    let mut chars = tail.char_indices();
    chars.next()?;
    let time = match chars.next() {
        Some((at, _)) => tail.get(at..)?,
        None => "",
    };
    if time.is_empty() {
        return None;
    }
    parse_fromiso_time(date, time)
}

/// Python `re` `\s` over `str`: ASCII whitespace plus the Unicode spaces.
fn is_python_space(char: char) -> bool {
    char.is_whitespace() || matches!(char, '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{1f}')
}

/// The `datetime_re` fallback (`django/utils/dateparse.py`):
/// `YYYY-M-D[T ]H:M[:S[.,f{1,12}]]`, 1-2 digit parts, optional whitespace
/// before an optional `Z` / `±HH[[:]MM]` zone, then end (the `$` anchor
/// also tolerates one trailing newline). Values run through the
/// `datetime` constructor; the zone total must stay strictly inside a day.
fn parse_regex_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    let (date_part, clock) = text.split_once(['T', ' '])?;
    let mut date_pieces = date_part.split('-');
    let (year_s, month_s, day_s) = (
        date_pieces.next()?,
        date_pieces.next()?,
        date_pieces.next()?,
    );
    if date_pieces.next().is_some() {
        return None;
    }
    if year_s.len() != 4
        || !year_s.bytes().all(|b| b.is_ascii_digit())
        || month_s.is_empty()
        || month_s.len() > 2
        || day_s.is_empty()
        || day_s.len() > 2
    {
        return None;
    }
    let (year, month, day): (i32, u32, u32) = (
        year_s.parse().ok()?,
        month_s.parse().ok()?,
        day_s.parse().ok()?,
    );
    let (hour_min, mut tail) = match clock.split_once(':') {
        Some((hour_s, rest)) => {
            let hour = one_two_digits(hour_s)?;
            // Minute digits, then seconds / fraction / zone tail.
            let minute_len: usize = rest.bytes().take_while(u8::is_ascii_digit).count();
            if minute_len == 0 || minute_len > 2 {
                return None;
            }
            let minute: u32 = rest.get(..minute_len)?.parse().ok()?;
            ((hour, minute), rest.get(minute_len..)?)
        }
        None => return None,
    };
    let (hour, minute) = hour_min;
    let (second, micro) = if let Some(rest) = tail.strip_prefix(':') {
        let second_len: usize = rest.bytes().take_while(u8::is_ascii_digit).count();
        if second_len == 0 || second_len > 2 {
            return None;
        }
        let second: u32 = rest.get(..second_len)?.parse().ok()?;
        tail = rest.get(second_len..)?;
        match tail.as_bytes().first() {
            Some(b'.' | b',') => {
                let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
                if !(1..=12).contains(&frac) {
                    return None;
                }
                let micro = frac_micros(tail.get(1..1 + frac)?);
                tail = tail.get(1 + frac..)?;
                (second, micro)
            }
            _ => (second, 0),
        }
    } else {
        (0, 0)
    };
    let spaces: usize = tail.chars().take_while(|c| is_python_space(*c)).count();
    tail = tail.get(tail.chars().take(spaces).map(char::len_utf8).sum::<usize>()..)?;
    let offset = if tail.is_empty() || tail == "\n" {
        None
    } else if tail == "Z" || tail == "Z\n" {
        Some(chrono::Duration::zero())
    } else {
        let zone = tail.strip_suffix('\n').unwrap_or(tail);
        let (sign, digits) = match zone.as_bytes().first() {
            Some(b'+') => (1i64, zone.get(1..)?),
            Some(b'-') => (-1i64, zone.get(1..)?),
            _ => return None,
        };
        let (hours, minutes) = if let Some((hour_s, minute_s)) = digits.split_once(':') {
            (two_digits(hour_s)?, two_digits(minute_s)?)
        } else if digits.len() == 2 {
            (two_digits(digits)?, 0)
        } else if digits.len() == 4 {
            (two_digits(digits.get(..2)?)?, two_digits(digits.get(2..)?)?)
        } else {
            return None;
        };
        let total = sign * (i64::from(hours) * 60 + i64::from(minutes));
        if total.abs() >= 24 * 60 {
            return None;
        }
        Some(chrono::Duration::minutes(total))
    };
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

// ---------------------------------------------------------------------------
// Serializer validation, field level (`to_internal_value` + `validate_*`)
// ---------------------------------------------------------------------------

/// A validated `pod` value: the active row the queryset returned (existence
/// + `project_id` for the cross-field check + `name` for the response).
struct PodChoice {
    id: Uuid,
    project_id: Uuid,
    name: String,
}

/// Field-level validated attrs: `None` means the key was absent (partial
/// PATCH skips it; on create, absent optionals take model defaults at
/// save and absent requireds already 400d). `project`/`pod` need three
/// states (absent / explicit null / value), hence the nesting.
#[derive(Default)]
struct BindingAttrs {
    scheduler: Option<Uuid>,
    project: Option<Option<Uuid>>,
    dtstart: Option<DateTime<Utc>>,
    tzid: Option<String>,
    rrule: Option<String>,
    rdates: Option<Vec<String>>,
    exdates: Option<Vec<String>>,
    extra_context: Option<String>,
    enabled: Option<bool>,
    outcome_mode: Option<String>,
    pod: Option<Option<PodChoice>>,
}

/// One collected field error, in `_writable_fields` order: a message list,
/// or the nested `{field: message}` dict the ISO-list validators raise.
enum FieldError {
    List(Vec<String>),
    Nested(String),
}

/// Validate one body field-level, in writable-field order, collecting
/// every failure like `to_internal_value` (object-level validators run
/// only when this passes). `instance` is `Some` on PATCH (partial: absent
/// keys skip) and `None` on install. Read-only keys in the body are
/// silently ignored (they are not writable fields); unknown keys too.
async fn validate_binding_fields(
    pool: &PgPool,
    body: &Map<String, Value>,
    instance: Option<&scheduler_binding::SchedulerBinding>,
    timezone: &Tz,
) -> Result<BindingAttrs, Denial> {
    let partial = instance.is_some();
    let rrule_validator: &(dyn Fn(&str) -> Result<(), String> + Send + Sync) = &rrule_validator;
    let mut attrs = BindingAttrs::default();
    // (field, error) in `_writable_fields` order: scheduler, project,
    // dtstart, tzid, rrule, rdates, exdates, extra_context, enabled,
    // outcome_mode, pod.
    let mut errors: Vec<(&str, FieldError)> = Vec::new();

    // scheduler: required PK (guard-passed on install, so the re-lookup
    // below always hits there — Django still issues it in field
    // validation, so this does too).
    match body.get("scheduler") {
        None if partial => {}
        None => errors.push((
            "scheduler",
            FieldError::List(vec!["This field is required.".to_owned()]),
        )),
        Some(Value::Null) => {
            errors.push((
                "scheduler",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match parse_pk_value(value) {
            Err(message) => errors.push(("scheduler", FieldError::List(vec![message]))),
            Ok(id) => match fetch_active_scheduler_id(pool, &id).await {
                Err(denial) => return Err(denial),
                Ok(false) => errors.push((
                    "scheduler",
                    FieldError::List(vec![format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        pk_echo(value)
                    )]),
                )),
                Ok(true) => attrs.scheduler = Some(id),
            },
        },
    }

    // project: allow_null (null model FK) but `required=True` — DRF's
    // `get_uniqueness_extra_kwargs` forces `required` on the unique pair's
    // fields (QUIRK-project-required), so a missing key 400s here,
    // collected with every other field error (the unique validator's own
    // `enforce_required_fields` never fires: `to_internal_value` raises
    // first). Explicit null passes (the unique check skips `None`).
    match body.get("project") {
        None if partial => {}
        None => errors.push((
            "project",
            FieldError::List(vec!["This field is required.".to_owned()]),
        )),
        Some(Value::Null) => attrs.project = Some(None),
        Some(value) => match parse_pk_value(value) {
            Err(message) => errors.push(("project", FieldError::List(vec![message]))),
            Ok(id) => match fetch_active_project_id(pool, &id).await {
                Err(denial) => return Err(denial),
                Ok(false) => errors.push((
                    "project",
                    FieldError::List(vec![format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        pk_echo(value)
                    )]),
                )),
                Ok(true) => attrs.project = Some(Some(id)),
            },
        },
    }

    // dtstart: required DRF DateTimeField.
    match body.get("dtstart") {
        None if partial => {}
        None => errors.push((
            "dtstart",
            FieldError::List(vec!["This field is required.".to_owned()]),
        )),
        Some(Value::Null) => {
            errors.push((
                "dtstart",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_dtstart_input(value, timezone) {
            Ok(instant) => attrs.dtstart = Some(instant),
            Err(message) => errors.push(("dtstart", FieldError::List(vec![message]))),
        },
    }

    // tzid: CharField(max_length=64), then `validate_tzid`. A malformed
    // zone key raises bare `ValueError` in Python — uncaught, i.e. it
    // escapes the whole validation as a 500 (QUIRK-tzid-500).
    match body.get("tzid") {
        None => {}
        Some(Value::Null) => {
            errors.push((
                "tzid",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_char_input(value, false, Some(64)) {
            Err(messages) => errors.push(("tzid", FieldError::List(messages))),
            Ok(coerced) => match shape::validate_tzid(&coerced) {
                Ok(canonical) => attrs.tzid = Some(canonical),
                Err(shape::TzidError::Unknown(message)) => {
                    errors.push(("tzid", FieldError::List(vec![message])));
                }
                Err(shape::TzidError::InvalidKey(_)) => return Err(Denial::ServerError),
            },
        },
    }

    // rrule: blank-allowed CharField, then `validate_rrule` with the jobs
    // verdict closure.
    match body.get("rrule") {
        None => {}
        Some(Value::Null) => {
            errors.push((
                "rrule",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_char_input(value, true, None) {
            Err(messages) => errors.push(("rrule", FieldError::List(messages))),
            Ok(coerced) => match shape::validate_rrule(&coerced, rrule_validator) {
                Ok(canonical) => attrs.rrule = Some(canonical),
                Err(message) => errors.push(("rrule", FieldError::List(vec![message]))),
            },
        },
    }

    // rdates/exdates: JSONField (null rejected) + the shared ISO-list
    // validator, whose `{field: message}` raise renders nested.
    for field in ["rdates", "exdates"] {
        match body.get(field) {
            None => {}
            Some(Value::Null) => {
                errors.push((
                    field,
                    FieldError::List(vec!["This field may not be null.".to_owned()]),
                ));
            }
            Some(value) => match shape::validate_iso_datetime_list(value, field) {
                Ok(normalized) => {
                    if field == "rdates" {
                        attrs.rdates = Some(normalized);
                    } else {
                        attrs.exdates = Some(normalized);
                    }
                }
                Err(error) => errors.push((field, FieldError::Nested(error.message))),
            },
        }
    }

    // extra_context: blank-allowed CharField + the 16 KiB cap.
    match body.get("extra_context") {
        None => {}
        Some(Value::Null) => {
            errors.push((
                "extra_context",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_char_input(value, true, None) {
            Err(messages) => errors.push(("extra_context", FieldError::List(messages))),
            Ok(coerced) => match shape::validate_extra_context(&coerced) {
                Ok(()) => attrs.extra_context = Some(coerced),
                Err(message) => errors.push(("extra_context", FieldError::List(vec![message]))),
            },
        },
    }

    // enabled: DRF BooleanField value sets.
    match body.get("enabled") {
        None => {}
        Some(Value::Null) => {
            errors.push((
                "enabled",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_boolean_input(value) {
            Ok(flag) => attrs.enabled = Some(flag),
            Err(message) => errors.push(("enabled", FieldError::List(vec![message]))),
        },
    }

    // outcome_mode: DRF ChoiceField over the three modes.
    match body.get("outcome_mode") {
        None => {}
        Some(Value::Null) => {
            errors.push((
                "outcome_mode",
                FieldError::List(vec!["This field may not be null.".to_owned()]),
            ));
        }
        Some(value) => match validate_outcome_mode_input(value) {
            Ok(mode) => attrs.outcome_mode = Some(mode),
            Err(message) => errors.push(("outcome_mode", FieldError::List(vec![message]))),
        },
    }

    // pod: explicit null passes (no queryset hit, cross-field skipped);
    // otherwise the active-pod queryset decides existence.
    match body.get("pod") {
        None => {}
        Some(Value::Null) => attrs.pod = Some(None),
        Some(value) => match parse_pk_value(value) {
            Err(message) => errors.push(("pod", FieldError::List(vec![message]))),
            Ok(id) => match fetch_active_pod(pool, &id).await {
                Err(denial) => return Err(denial),
                Ok(None) => errors.push((
                    "pod",
                    FieldError::List(vec![format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        pk_echo(value)
                    )]),
                )),
                Ok(Some(choice)) => attrs.pod = Some(Some(choice)),
            },
        },
    }

    if errors.is_empty() {
        return Ok(attrs);
    }
    let mut body = Map::with_capacity(errors.len());
    for (field, error) in errors {
        let rendered = match error {
            FieldError::List(messages) => {
                Value::Array(messages.into_iter().map(Value::String).collect())
            }
            // `as_serializer_error` passes nested dicts through with the
            // bare inner string (no list wrap).
            FieldError::Nested(message) => {
                let mut inner = Map::with_capacity(1);
                inner.insert(field.to_owned(), Value::String(message));
                Value::Object(inner)
            }
        };
        body.insert(field.to_owned(), rendered);
    }
    Err(Denial::BadJson(Value::Object(body)))
}

/// The `{pk_value}` echo in `does_not_exist`: the original input via
/// `str()` — only strings (verbatim) and integers (decimal) ever reach
/// the lookup; every other JSON type fails earlier with its own message.
fn pk_echo(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => py_str(value),
    }
}

// ---------------------------------------------------------------------------
// Serializer validation, object level (`UniqueTogetherValidator` + `validate`)
// ---------------------------------------------------------------------------

/// Install unique validation (DRF `UniqueTogetherValidator.__call__`):
/// the EXISTS check over the body (scheduler, project) pair — skipped
/// when any checked value is `None` (explicit-null project). Missing keys
/// never reach here: `project` is field-level `required`
/// (QUIRK-project-required), and the install guard 404s a missing
/// `scheduler` before validation runs.
async fn enforce_unique_install(
    pool: &PgPool,
    scheduler_id: &Uuid,
    body_project: Option<Uuid>,
) -> Result<(), Denial> {
    // Explicit null skips the check ("ignore validation if any field is
    // `None`"); `save()` pins the URL project instead.
    let Some(project_id) = body_project else {
        return Ok(());
    };
    if unique_binding_exists(pool, &project_id, scheduler_id, None).await? {
        return Err(unique_denial());
    }
    Ok(())
}

/// Shared unique-denial body (`as_serializer_error` over the validator's
/// bare message).
fn unique_denial() -> Denial {
    let mut body = Map::with_capacity(1);
    body.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(
            "The fields scheduler, project must make a unique set.".to_owned(),
        )]),
    );
    Denial::BadJson(Value::Object(body))
}

/// PATCH unique validation: missing keys fill from the instance, and only
/// *changed* values are checked (an unchanged pair skips the query
/// entirely); the current row is excluded, and any `None` among the
/// checked values skips the check.
async fn enforce_unique_patch(
    pool: &PgPool,
    binding_id: &Uuid,
    current: &scheduler_binding::SchedulerBinding,
    attrs: &BindingAttrs,
) -> Result<(), Denial> {
    let scheduler_changed = attrs.scheduler.is_some_and(|id| id != current.scheduler_id);
    let project_changed = attrs
        .project
        .is_some_and(|maybe| maybe != current.project_id);
    if !scheduler_changed && !project_changed {
        return Ok(());
    }
    // Changed values, `None`-skipped (`PATCH {project: null}` never
    // conflicts — the lock below rejects it instead). Only the project
    // half can be `None` (the scheduler field rejects null).
    let scheduler_val = attrs.scheduler.unwrap_or(current.scheduler_id);
    let project_val = attrs.project.unwrap_or(current.project_id);
    if project_changed && project_val.is_none() {
        return Ok(());
    }
    // Both values are `Some` here: unchanged keys filled from the
    // instance... except a NULL stored project with an unchanged key —
    // impossible via API writes (install always pins), and a `None`
    // filter value would match no row anyway (`= NULL` never holds), so
    // the check is skipped exactly like Django's queryset filter.
    let (Some(scheduler_use), Some(project_use)) = (Some(scheduler_val), project_val) else {
        return Ok(());
    };
    if unique_binding_exists(pool, &project_use, &scheduler_use, Some(binding_id)).await? {
        return Err(unique_denial());
    }
    Ok(())
}

/// Install cross-field `validate()` (`serializers/scheduler.py:235-267`;
/// the lock half only runs on update): the rrule+dtstart re-check, then
/// the pod-must-belong-to-project check with the view-supplied URL project
/// as context.
fn validate_install_cross(install: &InstallAttrs, url_project: &Uuid) -> Result<(), Denial> {
    let rrule_validator: &(dyn Fn(&str) -> Result<(), String> + Send + Sync) = &rrule_validator;
    if let Err(message) =
        shape::validate_cross_rrule(install.attrs.rrule.as_deref(), None, rrule_validator)
    {
        return Err(single_field_denial("rrule", &message));
    }
    let pod_project = install
        .attrs
        .pod
        .as_ref()
        .and_then(|maybe| maybe.as_ref())
        .map(|pod| pod.project_id);
    if let Err(message) = shape::validate_pod_project(
        pod_project,
        None,
        Some(*url_project),
        install.attrs.project.flatten(),
    ) {
        return Err(single_field_denial("pod", message));
    }
    Ok(())
}

/// PATCH cross-field `validate()`: the scheduler/project update lock
/// (scheduler first), then the rrule re-check (attrs-first,
/// instance-second), then the pod check (no view context on update —
/// instance first, body second).
fn validate_patch_cross(
    current: &scheduler_binding::SchedulerBinding,
    attrs: &BindingAttrs,
) -> Result<(), Denial> {
    if let Some(incoming) = attrs.scheduler {
        if let Err(message) =
            shape::validate_locked_field("scheduler", Some(incoming), current.scheduler_id)
        {
            return Err(single_field_denial("scheduler", &message));
        }
    }
    if let Some(incoming) = attrs.project {
        // `attrs[locked] != getattr(self.instance, locked)` — an explicit
        // null counts as present (and differs from any stored id).
        let locked = match (incoming, current.project_id) {
            (None, None) => None,
            (None, Some(_)) => Some(shape::lock_error("project")),
            (Some(value), Some(stored)) => {
                shape::validate_locked_field("project", Some(value), stored).err()
            }
            // A NULL stored project (hand-made rows only — installs always
            // pin) never equals an incoming id: `attrs != getattr`
            // rejects, exactly like Python's `uuid != None`.
            (Some(_), None) => Some(shape::lock_error("project")),
        };
        if let Some(message) = locked {
            return Err(single_field_denial("project", &message));
        }
    }
    let rrule_validator: &(dyn Fn(&str) -> Result<(), String> + Send + Sync) = &rrule_validator;
    if let Err(message) = shape::validate_cross_rrule(
        attrs.rrule.as_deref(),
        Some(current.rrule.as_str()),
        rrule_validator,
    ) {
        return Err(single_field_denial("rrule", &message));
    }
    let pod_project = attrs
        .pod
        .as_ref()
        .and_then(|maybe| maybe.as_ref())
        .map(|pod| pod.project_id);
    if let Err(message) = shape::validate_pod_project(
        pod_project,
        current.project_id,
        None,
        attrs.project.flatten(),
    ) {
        return Err(single_field_denial("pod", message));
    }
    Ok(())
}

/// The `validate()` raise shape: `{field: message}` through
/// `as_serializer_error` list-wraps the bare message.
fn single_field_denial(field: &str, message: &str) -> Denial {
    let mut body = Map::with_capacity(1);
    body.insert(
        field.to_owned(),
        Value::Array(vec![Value::String(message.to_owned())]),
    );
    Denial::BadJson(Value::Object(body))
}

// ---------------------------------------------------------------------------
// SQL execution (`queries.rs` statements via `:name` → `$n`)
// ---------------------------------------------------------------------------

/// Translate one [`queries`] statement's symbolic `:name` placeholders to
/// positional `$n` in the statement's `*_PARAMS` order (first appearance).
/// The scanner matches identifier boundaries, so `:now` never collides
/// with `:now2`; `::` casts pass through untouched.
fn translate_placeholders(sql: &str, params: &[&str]) -> String {
    debug_assert!(sql.is_ascii(), "fixture SQL is ASCII-only");
    let mut out = String::with_capacity(sql.len() + 16);
    let bytes = sql.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b':' {
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > index + 1 {
                let name = &sql[index + 1..end];
                if let Some(position) = params.iter().position(|param| *param == name) {
                    out.push('$');
                    out.push_str(&(position + 1).to_string());
                    index = end;
                    continue;
                }
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// True for Postgres integrity-constraint violations (`23xxx`) — Django's
/// `IntegrityError`, mapped by `handle_exception` to 400
/// [`INVALID_PAYLOAD_BODY`]. Every other database failure is a 500.
fn is_integrity_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db
            .code()
            .as_deref()
            .is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

fn db_denial(error: sqlx::Error) -> Denial {
    if is_integrity_violation(&error) {
        Denial::InvalidPayload
    } else {
        Denial::ServerError
    }
}

/// BR3 install project lookup: `(id, workspace_id)` or `None`.
struct InstallProject {
    workspace_id: Uuid,
}

async fn fetch_install_project(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<InstallProject>, Denial> {
    let sql = translate_placeholders(
        queries::INSTALL_PROJECT_LOOKUP_SQL,
        queries::INSTALL_PROJECT_LOOKUP_PARAMS,
    );
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(&sql)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(_id, workspace_id)| InstallProject { workspace_id }))
}

/// BR3 install scheduler guard: the full row or `None` (disabled,
/// foreign-workspace, or soft-deleted ids miss).
async fn fetch_install_scheduler(
    pool: &PgPool,
    scheduler_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<Option<scheduler::Scheduler>, Denial> {
    let sql = translate_placeholders(
        queries::INSTALL_SCHEDULER_GUARD_SQL,
        queries::INSTALL_SCHEDULER_GUARD_PARAMS,
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(scheduler_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| decode_scheduler(&row)).transpose()
}

/// BR4 unique check, with or without the PATCH self-row exclusion.
async fn unique_binding_exists(
    pool: &PgPool,
    project_id: &Uuid,
    scheduler_id: &Uuid,
    exclude_binding: Option<&Uuid>,
) -> Result<bool, Denial> {
    let sql = queries::binding_unique_check_sql(exclude_binding.is_some());
    let params = if exclude_binding.is_some() {
        queries::BINDING_UNIQUE_CHECK_PATCH_PARAMS
    } else {
        queries::BINDING_UNIQUE_CHECK_PARAMS
    };
    let sql = translate_placeholders(&sql, params);
    let mut query = sqlx::query_as::<_, (i32,)>(&sql)
        .bind(project_id)
        .bind(scheduler_id);
    if let Some(binding_id) = exclude_binding {
        query = query.bind(binding_id);
    }
    let row: Option<(i32,)> = query
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// BR1 binding list, newest first.
async fn fetch_binding_list(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
) -> Result<Vec<queries::BindingListRow>, Denial> {
    let sql = translate_placeholders(queries::BINDING_LIST_SQL, queries::BINDING_LIST_PARAMS);
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(project_id)
        .bind(slug)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter().map(decode_binding_list_row).collect()
}

/// BR2 binding detail lookup (GET/PATCH/uninstall share the shape).
async fn fetch_binding_detail(
    pool: &PgPool,
    binding_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<scheduler_binding::SchedulerBinding>, Denial> {
    let sql = translate_placeholders(queries::BINDING_DETAIL_SQL, queries::BINDING_DETAIL_PARAMS);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(binding_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| decode_binding(&row)).transpose()
}

// ---------------------------------------------------------------------------
// Row decode (fixture SELECT order → the `queries`/`db` row structs)
// ---------------------------------------------------------------------------

/// Column counts per BR1 segment (bindings 22 + schedulers 14 +
/// `AGENT_RUN_COLUMNS` 41 + `POD_COLUMNS` 10 = 87; pinned in tests).
const BINDING_COLUMN_COUNT: usize = 22;
const SCHEDULER_COLUMN_COUNT: usize = 14;

/// Decode one `scheduler_bindings` row in fixture SELECT order
/// (created_at … pod_id).
fn decode_binding(
    row: &sqlx::postgres::PgRow,
) -> Result<scheduler_binding::SchedulerBinding, Denial> {
    Ok(scheduler_binding::SchedulerBinding {
        created_at: row.try_get(0).map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get(1).map_err(|_| Denial::ServerError)?,
        created_by_id: row.try_get(2).map_err(|_| Denial::ServerError)?,
        updated_by_id: row.try_get(3).map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get(4).map_err(|_| Denial::ServerError)?,
        id: row.try_get(5).map_err(|_| Denial::ServerError)?,
        workspace_id: row.try_get(6).map_err(|_| Denial::ServerError)?,
        project_id: row.try_get(7).map_err(|_| Denial::ServerError)?,
        scheduler_id: row.try_get(8).map_err(|_| Denial::ServerError)?,
        dtstart: row.try_get(9).map_err(|_| Denial::ServerError)?,
        tzid: row.try_get(10).map_err(|_| Denial::ServerError)?,
        rrule: row.try_get(11).map_err(|_| Denial::ServerError)?,
        rdates: row.try_get(12).map_err(|_| Denial::ServerError)?,
        exdates: row.try_get(13).map_err(|_| Denial::ServerError)?,
        extra_context: row.try_get(14).map_err(|_| Denial::ServerError)?,
        enabled: row.try_get(15).map_err(|_| Denial::ServerError)?,
        outcome_mode: row.try_get(16).map_err(|_| Denial::ServerError)?,
        next_run_at: row.try_get(17).map_err(|_| Denial::ServerError)?,
        last_run_id: row.try_get(18).map_err(|_| Denial::ServerError)?,
        last_error: row.try_get(19).map_err(|_| Denial::ServerError)?,
        actor_id: row.try_get(20).map_err(|_| Denial::ServerError)?,
        pod_id: row.try_get(21).map_err(|_| Denial::ServerError)?,
    })
}

/// Decode one `schedulers` row in fixture SELECT order at `base`.
fn decode_scheduler_at(
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<scheduler::Scheduler, Denial> {
    Ok(scheduler::Scheduler {
        created_at: row.try_get(base).map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get(base + 1).map_err(|_| Denial::ServerError)?,
        created_by_id: row.try_get(base + 2).map_err(|_| Denial::ServerError)?,
        updated_by_id: row.try_get(base + 3).map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get(base + 4).map_err(|_| Denial::ServerError)?,
        id: row.try_get(base + 5).map_err(|_| Denial::ServerError)?,
        workspace_id: row.try_get(base + 6).map_err(|_| Denial::ServerError)?,
        slug: row.try_get(base + 7).map_err(|_| Denial::ServerError)?,
        name: row.try_get(base + 8).map_err(|_| Denial::ServerError)?,
        description: row.try_get(base + 9).map_err(|_| Denial::ServerError)?,
        prompt: row.try_get(base + 10).map_err(|_| Denial::ServerError)?,
        source: row.try_get(base + 11).map_err(|_| Denial::ServerError)?,
        is_enabled: row.try_get(base + 12).map_err(|_| Denial::ServerError)?,
        color: row.try_get(base + 13).map_err(|_| Denial::ServerError)?,
    })
}

fn decode_scheduler(row: &sqlx::postgres::PgRow) -> Result<scheduler::Scheduler, Denial> {
    decode_scheduler_at(row, 0)
}

/// Decode one `agent_run` row in `AGENT_RUN_COLUMNS` order at `base`.
fn decode_agent_run_at(
    row: &sqlx::postgres::PgRow,
    base: usize,
) -> Result<queries::AgentRunRow, Denial> {
    Ok(queries::AgentRunRow {
        id: row.try_get(base).map_err(|_| Denial::ServerError)?,
        workspace_id: row.try_get(base + 1).map_err(|_| Denial::ServerError)?,
        owner_id: row.try_get(base + 2).map_err(|_| Denial::ServerError)?,
        created_by_id: row.try_get(base + 3).map_err(|_| Denial::ServerError)?,
        pod_id: row.try_get(base + 4).map_err(|_| Denial::ServerError)?,
        runner_id: row.try_get(base + 5).map_err(|_| Denial::ServerError)?,
        pinned_runner_id: row.try_get(base + 6).map_err(|_| Denial::ServerError)?,
        work_item_id: row.try_get(base + 7).map_err(|_| Denial::ServerError)?,
        scheduler_binding_id: row.try_get(base + 8).map_err(|_| Denial::ServerError)?,
        parent_run_id: row.try_get(base + 9).map_err(|_| Denial::ServerError)?,
        status: row.try_get(base + 10).map_err(|_| Denial::ServerError)?,
        executor_kind: row.try_get(base + 11).map_err(|_| Denial::ServerError)?,
        dispatch_attempts: row.try_get(base + 12).map_err(|_| Denial::ServerError)?,
        cancel_requested_at: row.try_get(base + 13).map_err(|_| Denial::ServerError)?,
        cancel_reason: row.try_get(base + 14).map_err(|_| Denial::ServerError)?,
        error_code: row.try_get(base + 15).map_err(|_| Denial::ServerError)?,
        tool_plan: row.try_get(base + 16).map_err(|_| Denial::ServerError)?,
        terminal_hooks_applied_at: row.try_get(base + 17).map_err(|_| Denial::ServerError)?,
        terminal_capacity_released_at: row.try_get(base + 18).map_err(|_| Denial::ServerError)?,
        prompt: row.try_get(base + 19).map_err(|_| Denial::ServerError)?,
        trigger: row.try_get(base + 20).map_err(|_| Denial::ServerError)?,
        prompt_manifest: row.try_get(base + 21).map_err(|_| Denial::ServerError)?,
        phase_kind: row.try_get(base + 22).map_err(|_| Denial::ServerError)?,
        run_config: row.try_get(base + 23).map_err(|_| Denial::ServerError)?,
        required_capabilities: row.try_get(base + 24).map_err(|_| Denial::ServerError)?,
        thread_id: row.try_get(base + 25).map_err(|_| Denial::ServerError)?,
        agent_metadata: row.try_get(base + 26).map_err(|_| Denial::ServerError)?,
        lease_expires_at: row.try_get(base + 27).map_err(|_| Denial::ServerError)?,
        done_payload: row.try_get(base + 28).map_err(|_| Denial::ServerError)?,
        error: row.try_get(base + 29).map_err(|_| Denial::ServerError)?,
        refusal_category: row.try_get(base + 30).map_err(|_| Denial::ServerError)?,
        llm_model: row.try_get(base + 31).map_err(|_| Denial::ServerError)?,
        usage: row.try_get(base + 32).map_err(|_| Denial::ServerError)?,
        input_tokens: row.try_get(base + 33).map_err(|_| Denial::ServerError)?,
        output_tokens: row.try_get(base + 34).map_err(|_| Denial::ServerError)?,
        total_tokens: row.try_get(base + 35).map_err(|_| Denial::ServerError)?,
        created_at: row.try_get(base + 36).map_err(|_| Denial::ServerError)?,
        assigned_at: row.try_get(base + 37).map_err(|_| Denial::ServerError)?,
        queue_position: row.try_get(base + 38).map_err(|_| Denial::ServerError)?,
        started_at: row.try_get(base + 39).map_err(|_| Denial::ServerError)?,
        ended_at: row.try_get(base + 40).map_err(|_| Denial::ServerError)?,
    })
}

/// Decode one `pod` row in `POD_COLUMNS` order at `base`.
fn decode_pod_at(row: &sqlx::postgres::PgRow, base: usize) -> Result<queries::PodRow, Denial> {
    Ok(queries::PodRow {
        id: row.try_get(base).map_err(|_| Denial::ServerError)?,
        workspace_id: row.try_get(base + 1).map_err(|_| Denial::ServerError)?,
        project_id: row.try_get(base + 2).map_err(|_| Denial::ServerError)?,
        name: row.try_get(base + 3).map_err(|_| Denial::ServerError)?,
        description: row.try_get(base + 4).map_err(|_| Denial::ServerError)?,
        created_by_id: row.try_get(base + 5).map_err(|_| Denial::ServerError)?,
        is_default: row.try_get(base + 6).map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get(base + 7).map_err(|_| Denial::ServerError)?,
        created_at: row.try_get(base + 8).map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get(base + 9).map_err(|_| Denial::ServerError)?,
    })
}

/// Decode one BR1 row: the binding, its INNER-joined scheduler, and the
/// LEFT-joined last run / pod (`None` exactly when the joined `id` is
/// NULL — every joined column is NULL then).
fn decode_binding_list_row(row: &sqlx::postgres::PgRow) -> Result<queries::BindingListRow, Denial> {
    let binding = decode_binding(row)?;
    let scheduler_base = BINDING_COLUMN_COUNT;
    let run_base = scheduler_base + SCHEDULER_COLUMN_COUNT;
    let pod_base = run_base + queries::AGENT_RUN_COLUMNS.len();
    let scheduler = decode_scheduler_at(row, scheduler_base)?;
    let last_run = {
        let id: Option<Uuid> = row.try_get(run_base).map_err(|_| Denial::ServerError)?;
        if id.is_some() {
            Some(decode_agent_run_at(row, run_base)?)
        } else {
            None
        }
    };
    let pod = {
        let id: Option<Uuid> = row.try_get(pod_base).map_err(|_| Denial::ServerError)?;
        if id.is_some() {
            Some(decode_pod_at(row, pod_base)?)
        } else {
            None
        }
    };
    Ok(queries::BindingListRow {
        binding,
        scheduler,
        last_run,
        pod,
    })
}

// ---------------------------------------------------------------------------
// Field-validation lookups (related-field querysets)
// ---------------------------------------------------------------------------

/// Scheduler field queryset (`Scheduler._default_manager`): any
/// non-deleted row, enabled or not, in any workspace (the install guard's
/// workspace/enabled scoping does NOT apply here).
async fn fetch_active_scheduler_id(pool: &PgPool, id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM schedulers s WHERE s.id = $1 AND s.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Project field queryset (`Project._default_manager`): any non-deleted
/// row in any workspace (QUIRK-body-project-discarded: the body project
/// need not match the URL project — it is validated, then discarded).
async fn fetch_active_project_id(pool: &PgPool, id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects p WHERE p.id = $1 AND p.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Pod field queryset (`Pod.objects.filter(deleted_at__isnull=True)`,
/// F36-02 `pod_queryset_rule`): the active row's id/project/name, or
/// `None` when unknown or soft-deleted (both report `does_not_exist`).
async fn fetch_active_pod(pool: &PgPool, id: &Uuid) -> Result<Option<PodChoice>, Denial> {
    let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        r#"SELECT p.id, p.project_id, p.name FROM pod p
           WHERE p.id = $1 AND p.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(id, project_id, name)| PodChoice {
        id,
        project_id,
        name,
    }))
}

// ---------------------------------------------------------------------------
// Writes (BR5/BR7/BR6/BR8)
// ---------------------------------------------------------------------------

/// Install save inputs: the guard-pinned ids plus the validated attrs
/// (the body project is validated but discarded — `save()` pins the URL
/// project).
struct InstallAttrs {
    scheduler_id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    actor_id: Uuid,
    attrs: BindingAttrs,
}

/// The just-inserted row in memory (what the 201 re-renders) plus the
/// validated pod name (the serializer's cached `pod` dereference).
struct SavedBinding {
    row: scheduler_binding::SchedulerBinding,
    pod_name: Option<String>,
}

impl SavedBinding {
    /// The recompute bundle over the just-saved row (stored values incl.
    /// defaults for omitted keys).
    fn recompute_bundle(&self) -> queries::RruleBundle<'_> {
        queries::RruleBundle {
            dtstart: self.row.dtstart,
            rrule: self.row.rrule.as_str(),
            tzid: self.row.tzid.as_str(),
            rdates: &self.row.rdates,
            exdates: &self.row.exdates,
        }
    }
}

fn iso_list_value(items: &[String]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| Value::String(item.clone()))
            .collect(),
    )
}

/// BR5 install INSERT (`:207-212`): validated fields + pinned
/// scheduler/project/workspace/actor; model defaults for omitted keys;
/// `created_by` = request user (crum), `updated_by` NULL.
async fn insert_binding(pool: &PgPool, install: &InstallAttrs) -> Result<SavedBinding, Denial> {
    let sql = translate_placeholders(queries::BINDING_INSERT_SQL, queries::BINDING_INSERT_PARAMS);
    let id = Uuid::new_v4();
    let now = Utc::now();
    let attrs = &install.attrs;
    // `dtstart` is required, so it is always `Some` here.
    let dtstart = attrs.dtstart.ok_or(Denial::ServerError)?;
    let tzid = attrs.tzid.clone().unwrap_or_else(|| "UTC".to_owned());
    let rrule = attrs.rrule.clone().unwrap_or_default();
    let rdates = attrs.rdates.clone().unwrap_or_default();
    let exdates = attrs.exdates.clone().unwrap_or_default();
    let extra_context = attrs.extra_context.clone().unwrap_or_default();
    let enabled = attrs.enabled.unwrap_or(true);
    let outcome_mode = attrs
        .outcome_mode
        .clone()
        .unwrap_or_else(|| "create_issue".to_owned());
    let pod_id = attrs
        .pod
        .as_ref()
        .and_then(|maybe| maybe.as_ref())
        .map(|pod| pod.id);
    let pod_name = attrs
        .pod
        .as_ref()
        .and_then(|maybe| maybe.as_ref())
        .map(|pod| pod.name.clone());
    sqlx::query(&sql)
        .bind(id)
        .bind(now)
        .bind(install.actor_id)
        .bind(install.workspace_id)
        .bind(install.project_id)
        .bind(install.scheduler_id)
        .bind(dtstart)
        .bind(&tzid)
        .bind(&rrule)
        .bind(iso_list_value(&rdates))
        .bind(iso_list_value(&exdates))
        .bind(&extra_context)
        .bind(enabled)
        .bind(&outcome_mode)
        .bind(install.actor_id)
        .bind(pod_id)
        .execute(pool)
        .await
        .map_err(db_denial)?;
    Ok(SavedBinding {
        row: scheduler_binding::SchedulerBinding {
            id,
            created_at: now,
            updated_at: now,
            created_by_id: Some(install.actor_id),
            updated_by_id: None,
            deleted_at: None,
            workspace_id: install.workspace_id,
            project_id: Some(install.project_id),
            scheduler_id: install.scheduler_id,
            dtstart,
            tzid,
            rrule,
            rdates: iso_list_value(&rdates),
            exdates: iso_list_value(&exdates),
            extra_context,
            enabled,
            outcome_mode,
            next_run_at: None,
            last_run_id: None,
            last_error: String::new(),
            actor_id: Some(install.actor_id),
            pod_id,
        },
        pod_name,
    })
}

/// BR7 PATCH save (`:263`): writable columns (provided values, untouched
/// ones rewritten) + `updated_at`/`updated_by` refresh.
async fn update_binding(
    pool: &PgPool,
    binding_id: &Uuid,
    current: &scheduler_binding::SchedulerBinding,
    attrs: &BindingAttrs,
    user_id: &Uuid,
    stamp: &DateTime<Utc>,
) -> Result<(), Denial> {
    let sql = translate_placeholders(queries::BINDING_PATCH_SQL, queries::BINDING_PATCH_PARAMS);
    let dtstart = attrs.dtstart.unwrap_or(current.dtstart);
    let tzid = attrs.tzid.clone().unwrap_or_else(|| current.tzid.clone());
    let rrule = attrs.rrule.clone().unwrap_or_else(|| current.rrule.clone());
    let rdates = attrs
        .rdates
        .clone()
        .map(|items| iso_list_value(&items))
        .unwrap_or_else(|| current.rdates.clone());
    let exdates = attrs
        .exdates
        .clone()
        .map(|items| iso_list_value(&items))
        .unwrap_or_else(|| current.exdates.clone());
    let extra_context = attrs
        .extra_context
        .clone()
        .unwrap_or_else(|| current.extra_context.clone());
    let enabled = attrs.enabled.unwrap_or(current.enabled);
    let outcome_mode = attrs
        .outcome_mode
        .clone()
        .unwrap_or_else(|| current.outcome_mode.clone());
    let pod_id = attrs
        .pod
        .as_ref()
        .map_or(current.pod_id, |maybe| maybe.as_ref().map(|pod| pod.id));
    sqlx::query(&sql)
        .bind(stamp)
        .bind(user_id)
        .bind(dtstart)
        .bind(&tzid)
        .bind(&rrule)
        .bind(&rdates)
        .bind(&exdates)
        .bind(&extra_context)
        .bind(enabled)
        .bind(&outcome_mode)
        .bind(pod_id)
        .bind(binding_id)
        .execute(pool)
        .await
        .map_err(db_denial)?;
    Ok(())
}

/// The in-memory row after a PATCH save (what the 200 re-renders when no
/// recompute runs): saved values + the save stamp.
fn patched_row(
    current: &scheduler_binding::SchedulerBinding,
    attrs: &BindingAttrs,
    user_id: &Uuid,
    stamp: &DateTime<Utc>,
) -> scheduler_binding::SchedulerBinding {
    scheduler_binding::SchedulerBinding {
        updated_at: *stamp,
        updated_by_id: Some(*user_id),
        dtstart: attrs.dtstart.unwrap_or(current.dtstart),
        tzid: attrs.tzid.clone().unwrap_or_else(|| current.tzid.clone()),
        rrule: attrs.rrule.clone().unwrap_or_else(|| current.rrule.clone()),
        rdates: attrs
            .rdates
            .clone()
            .map(|items| iso_list_value(&items))
            .unwrap_or_else(|| current.rdates.clone()),
        exdates: attrs
            .exdates
            .clone()
            .map(|items| iso_list_value(&items))
            .unwrap_or_else(|| current.exdates.clone()),
        extra_context: attrs
            .extra_context
            .clone()
            .unwrap_or_else(|| current.extra_context.clone()),
        enabled: attrs.enabled.unwrap_or(current.enabled),
        outcome_mode: attrs
            .outcome_mode
            .clone()
            .unwrap_or_else(|| current.outcome_mode.clone()),
        pod_id: attrs
            .pod
            .as_ref()
            .map_or(current.pod_id, |maybe| maybe.as_ref().map(|pod| pod.id)),
        ..current.clone()
    }
}

/// BR6 `next_run_at` write-back: `next_run_at` + `updated_at` only
/// (`updated_by` is NOT rewritten).
async fn write_next_run_at(
    pool: &PgPool,
    binding_id: &Uuid,
    computed: &DateTime<Utc>,
    stamp: &DateTime<Utc>,
) -> Result<(), Denial> {
    let sql = translate_placeholders(
        queries::NEXT_RUN_AT_WRITEBACK_SQL,
        queries::NEXT_RUN_AT_WRITEBACK_PARAMS,
    );
    sqlx::query(&sql)
        .bind(computed)
        .bind(stamp)
        .bind(binding_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// BR8 uninstall (`:290`): `binding.delete()` soft-delete with two `now()`
/// samples (ported quirk 5).
async fn uninstall_binding(
    pool: &PgPool,
    binding_id: &Uuid,
    user_id: &Uuid,
    now: &DateTime<Utc>,
    now2: &DateTime<Utc>,
) -> Result<(), Denial> {
    let sql = translate_placeholders(
        queries::BINDING_UNINSTALL_SQL,
        queries::BINDING_UNINSTALL_PARAMS,
    );
    sqlx::query(&sql)
        .bind(now)
        .bind(now2)
        .bind(user_id)
        .bind(binding_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Detail display fetches (the serializer's per-object dereferences)
// ---------------------------------------------------------------------------

/// Scheduler display columns for one id, through the BASE manager:
/// forward-FK dereferences (`ForwardManyToOneDescriptor`) use
/// `_base_manager`, which carries no tombstone filter — a soft-deleted
/// row still renders its values (verified live against Django). A miss
/// (hard-deleted row, unreachable except races) renders nulls.
async fn fetch_scheduler_display(
    pool: &PgPool,
    scheduler_id: &Uuid,
) -> Result<Option<(String, String, String)>, Denial> {
    let row: Option<(String, String, String)> =
        sqlx::query_as(r#"SELECT s.slug, s.name, s.color FROM schedulers s WHERE s.id = $1"#)
            .bind(scheduler_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row)
}

/// Last-run display columns for one id (`agent_run` has no `deleted_at`
/// column). A miss with `last_run_id` set is QUIRK-orphan-run's detail
/// half: `DoesNotExist` escapes `get_last_run_*` → 404 error body.
async fn fetch_last_run_display(
    pool: &PgPool,
    last_run_id: &Uuid,
) -> Result<(String, Option<DateTime<Utc>>), Denial> {
    let row: Option<(String, Option<DateTime<Utc>>)> =
        sqlx::query_as(r#"SELECT r.status, r.ended_at FROM agent_run r WHERE r.id = $1"#)
            .bind(last_run_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::ObjectNotFound)
}

/// Pod display name for one id, through the BASE manager (same
/// `_base_manager` rule as the scheduler fetch): soft-deleted rows still
/// render their stale name on every path — only the validation queryset
/// (`Pod.objects`, the default manager) excludes tombstones.
async fn fetch_pod_display(pool: &PgPool, pod_id: &Uuid) -> Result<Option<String>, Denial> {
    let row: Option<(String,)> = sqlx::query_as(r#"SELECT p.name FROM pod p WHERE p.id = $1"#)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

// ---------------------------------------------------------------------------
// Render (`SchedulerBindingSerializer.to_representation`)
// ---------------------------------------------------------------------------

fn render_uuid(id: &Uuid) -> Value {
    Value::String(id.to_string())
}

fn render_uuid_opt(id: &Option<Uuid>) -> Value {
    id.map(|id| render_uuid(&id)).unwrap_or(Value::Null)
}

fn render_datetime_opt(instant: &Option<DateTime<Utc>>, timezone: &Tz) -> Value {
    instant
        .map(|instant| Value::String(crate::serializer::render_datetime_in(&instant, timezone)))
        .unwrap_or(Value::Null)
}

/// Assemble one binding row in `BINDING_SERIALIZER_FIELDS` order (pinned
/// against [`shape::BINDING_SERIALIZER_FIELDS`] in tests).
#[allow(clippy::too_many_arguments)]
fn render_binding_object(
    id: &Uuid,
    scheduler_id: &Uuid,
    scheduler_display: &Option<(String, String, String)>,
    project_id: &Option<Uuid>,
    workspace_id: &Uuid,
    dtstart: &DateTime<Utc>,
    tzid: &str,
    rrule: &str,
    rdates: &Value,
    exdates: &Value,
    extra_context: &str,
    enabled: bool,
    outcome_mode: &str,
    pod_id: &Option<Uuid>,
    pod_name: &Option<String>,
    next_run_at: &Option<DateTime<Utc>>,
    last_run_id: &Option<Uuid>,
    last_run_status: Option<&str>,
    last_run_ended_at: Option<String>,
    last_error: &str,
    actor_id: &Option<Uuid>,
    created_at: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
    timezone: &Tz,
) -> String {
    let mut row = Map::with_capacity(shape::BINDING_SERIALIZER_FIELDS.len());
    row.insert("id".to_owned(), render_uuid(id));
    row.insert("scheduler".to_owned(), render_uuid(scheduler_id));
    let (slug, name, color) = match scheduler_display {
        Some((slug, name, color)) => (
            Value::String(slug.clone()),
            Value::String(name.clone()),
            Value::String(color.clone()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
    };
    row.insert("scheduler_slug".to_owned(), slug);
    row.insert("scheduler_name".to_owned(), name);
    row.insert("scheduler_color".to_owned(), color);
    row.insert("project".to_owned(), render_uuid_opt(project_id));
    row.insert("workspace".to_owned(), render_uuid(workspace_id));
    row.insert(
        "dtstart".to_owned(),
        Value::String(crate::serializer::render_datetime_in(dtstart, timezone)),
    );
    row.insert("tzid".to_owned(), Value::String(tzid.to_owned()));
    row.insert("rrule".to_owned(), Value::String(rrule.to_owned()));
    row.insert("rdates".to_owned(), rdates.clone());
    row.insert("exdates".to_owned(), exdates.clone());
    row.insert(
        "extra_context".to_owned(),
        Value::String(extra_context.to_owned()),
    );
    row.insert("enabled".to_owned(), Value::Bool(enabled));
    row.insert(
        "outcome_mode".to_owned(),
        Value::String(outcome_mode.to_owned()),
    );
    row.insert("pod".to_owned(), render_uuid_opt(pod_id));
    row.insert(
        "pod_name".to_owned(),
        pod_name
            .as_ref()
            .map(|name| Value::String(name.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "next_run_at".to_owned(),
        render_datetime_opt(next_run_at, timezone),
    );
    row.insert("last_run".to_owned(), render_uuid_opt(last_run_id));
    row.insert(
        "last_run_status".to_owned(),
        last_run_status
            .map(|status| Value::String(status.to_owned()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "last_run_ended_at".to_owned(),
        last_run_ended_at.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "last_error".to_owned(),
        Value::String(last_error.to_owned()),
    );
    row.insert("actor".to_owned(), render_uuid_opt(actor_id));
    row.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(created_at, timezone)),
    );
    row.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(updated_at, timezone)),
    );
    serde_json::to_string(&Value::Object(row)).expect("serializable row")
}

/// Render one BR1 list row: the joined scheduler is always present
/// (INNER), the joined pod renders stale names for soft-deleted rows
/// (QUIRK-list-detail-pod's list half), and an orphaned `last_run_id`
/// 500s (QUIRK-orphan-run's list half: `None.status` → `AttributeError`).
fn render_binding_row(row: &queries::BindingListRow, timezone: &Tz) -> Result<String, Denial> {
    let binding = &row.binding;
    let scheduler_display = Some((
        row.scheduler.slug.clone(),
        row.scheduler.name.clone(),
        row.scheduler.color.clone(),
    ));
    let pod_name: Option<String> = match (&binding.pod_id, &row.pod) {
        (None, _) => None,
        (Some(_), Some(pod)) => {
            shape::pod_display_name(binding.pod_id, pod.name.as_str()).map(str::to_owned)
        }
        // Impossible except races (the FK guards it): Django renders the
        // id with a null name (attname renders, traversal defaults).
        (Some(_), None) => None,
    };
    // `get_last_run_status` / `get_last_run_ended_at`
    // (`obj.last_run.* if obj.last_run_id else None` — the
    // `shape::last_run_*` gate, decided by this match).
    let (last_run_status, last_run_ended_at) = match (&binding.last_run_id, &row.last_run) {
        (None, _) => (None, None),
        (Some(_), Some(run)) => (
            shape::last_run_status(binding.last_run_id, run.status.as_str()).map(str::to_owned),
            run.ended_at
                .map(|ended| crate::serializer::render_datetime_in(&ended, timezone)),
        ),
        (Some(_), None) => return Err(Denial::ServerError),
    };
    Ok(render_binding_object(
        &binding.id,
        &binding.scheduler_id,
        &scheduler_display,
        &binding.project_id,
        &binding.workspace_id,
        &binding.dtstart,
        binding.tzid.as_str(),
        binding.rrule.as_str(),
        &binding.rdates,
        &binding.exdates,
        binding.extra_context.as_str(),
        binding.enabled,
        binding.outcome_mode.as_str(),
        &binding.pod_id,
        &pod_name,
        &binding.next_run_at,
        &binding.last_run_id,
        last_run_status.as_deref(),
        last_run_ended_at,
        binding.last_error.as_str(),
        &binding.actor_id,
        &binding.created_at,
        &binding.updated_at,
        timezone,
    ))
}

/// Render one detail/install/PATCH row: the BR2 row plus the serializer's
/// per-object dereferences (default-manager semantics — the detail half
/// of QUIRK-list-detail-pod and QUIRK-orphan-run).
async fn render_detail_binding(
    pool: &PgPool,
    row: &scheduler_binding::SchedulerBinding,
    timezone: &Tz,
) -> Result<String, Denial> {
    let scheduler_display = fetch_scheduler_display(pool, &row.scheduler_id).await?;
    // Same `last_run.* if last_run_id else None` gate as the list path.
    let (last_run_status, last_run_ended_at) = match row.last_run_id {
        None => (None, None),
        Some(last_run_id) => {
            let (status, ended_at) = fetch_last_run_display(pool, &last_run_id).await?;
            (
                shape::last_run_status(row.last_run_id, status.as_str()).map(str::to_owned),
                ended_at.map(|ended| crate::serializer::render_datetime_in(&ended, timezone)),
            )
        }
    };
    let pod_name = match row.pod_id {
        None => None,
        Some(pod_id) => fetch_pod_display(pool, &pod_id).await?,
    };
    Ok(render_binding_object(
        &row.id,
        &row.scheduler_id,
        &scheduler_display,
        &row.project_id,
        &row.workspace_id,
        &row.dtstart,
        row.tzid.as_str(),
        row.rrule.as_str(),
        &row.rdates,
        &row.exdates,
        row.extra_context.as_str(),
        row.enabled,
        row.outcome_mode.as_str(),
        &row.pod_id,
        &pod_name,
        &row.next_run_at,
        &row.last_run_id,
        last_run_status.as_deref(),
        last_run_ended_at,
        row.last_error.as_str(),
        &row.actor_id,
        &row.created_at,
        &row.updated_at,
        timezone,
    ))
}

/// Render the install 201 from the in-memory saved row: the guard
/// scheduler's display columns, the validated pod name (the cached
/// dereference), and null run state.
fn render_saved_binding(
    saved: &SavedBinding,
    sched: &scheduler::Scheduler,
    timezone: &Tz,
) -> String {
    let row = &saved.row;
    let scheduler_display = Some((sched.slug.clone(), sched.name.clone(), sched.color.clone()));
    render_binding_object(
        &row.id,
        &row.scheduler_id,
        &scheduler_display,
        &row.project_id,
        &row.workspace_id,
        &row.dtstart,
        row.tzid.as_str(),
        row.rrule.as_str(),
        &row.rdates,
        &row.exdates,
        row.extra_context.as_str(),
        row.enabled,
        row.outcome_mode.as_str(),
        &row.pod_id,
        &saved.pod_name,
        &row.next_run_at,
        &row.last_run_id,
        None,
        None,
        row.last_error.as_str(),
        &row.actor_id,
        &row.created_at,
        &row.updated_at,
        timezone,
    )
}

// ---------------------------------------------------------------------------
// Jobs-backed closures (the handlers seam — services must not name jobs)
// ---------------------------------------------------------------------------

/// `rrule_validator` over
/// `pidash_jobs::tasks_ticker::rrule::validate_rrule_string`: only the
/// message string crosses the seam (all Python uses is `str(e)`), into
/// shape validation (field-level + the cross-field re-check).
fn rrule_validator(value: &str) -> Result<(), String> {
    pidash_jobs::tasks_ticker::rrule::validate_rrule_string(value)
        .map_err(|error| error.message().to_owned())
}

/// `next_fire_for_binding` over jobs `coerce_iso_datetimes` +
/// `next_fire_from_rrule`: the exact `_next_fire_for_binding` call shape
/// (`bgtasks/scheduler.py:70-83`) into the queries install/patch writes.
/// The `rrule or ""` guard is a no-op on `&str`; the `tzid or "UTC"`
/// guard is unobservable (expansion runs in UTC) but ported, not dropped.
fn next_fire_for_binding(
    bundle: &queries::RruleBundle<'_>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let tzid = if bundle.tzid.is_empty() {
        "UTC"
    } else {
        bundle.tzid
    };
    pidash_jobs::tasks_ticker::rrule::next_fire_from_rrule(
        bundle.dtstart,
        bundle.rrule,
        tzid,
        &pidash_jobs::tasks_ticker::fire_binding::coerce_iso_datetimes(bundle.rdates),
        &pidash_jobs::tasks_ticker::fire_binding::coerce_iso_datetimes(bundle.exdates),
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // -- Exact bodies (F36-11 §action + the contract suites) ----------------

    #[test]
    fn not_found_bodies_are_byte_exact() {
        for (body, tail) in [
            (
                NOT_FOUND_BINDING_BODY,
                "No SchedulerBinding matches the given query.",
            ),
            (
                NOT_FOUND_PROJECT_BODY,
                "No Project matches the given query.",
            ),
            (
                NOT_FOUND_SCHEDULER_BODY,
                "No Scheduler matches the given query.",
            ),
        ] {
            assert_eq!(
                body,
                format!("{{\"detail\":{}}}", json_string(tail)),
                "lowercase detail, compact separators"
            );
        }
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            INVALID_PAYLOAD_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            VALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
    }

    #[test]
    fn unique_denial_matches_const() {
        let Denial::BadJson(body) = unique_denial() else {
            panic!("unique denial is a 400 json body");
        };
        assert_eq!(
            serde_json::to_string(&body).expect("serializable"),
            UNIQUE_BODY
        );
        // The install-duplicate contract pin, byte for byte.
        assert_eq!(
            UNIQUE_BODY,
            r#"{"non_field_errors":["The fields scheduler, project must make a unique set."]}"#
        );
    }

    #[test]
    fn denial_statuses_match_f36_11() {
        let cases: Vec<(Denial, StatusCode)> = vec![
            (
                Denial::NotFound(NOT_FOUND_BINDING_BODY),
                StatusCode::NOT_FOUND,
            ),
            (Denial::ObjectNotFound, StatusCode::NOT_FOUND),
            (
                Denial::BadJson(Value::Object(Map::new())),
                StatusCode::BAD_REQUEST,
            ),
            (Denial::BadDetail("x".to_owned()), StatusCode::BAD_REQUEST),
            (Denial::BadError("x".to_owned()), StatusCode::BAD_REQUEST),
            (Denial::ValidDetail, StatusCode::BAD_REQUEST),
            (Denial::InvalidPayload, StatusCode::BAD_REQUEST),
            (Denial::ServerError, StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (denial, status) in cases {
            assert_eq!(denial.status_and_body().0, status);
        }
    }

    // -- Placeholder translation -------------------------------------------

    #[test]
    fn placeholders_translate_in_params_order() {
        // Every statement: no `:name` survives, `$n` numbering follows the
        // PARAMS list, and `:now`/`:now2` never collide.
        let statements = [
            (queries::BINDING_LIST_SQL, queries::BINDING_LIST_PARAMS),
            (queries::BINDING_DETAIL_SQL, queries::BINDING_DETAIL_PARAMS),
            (
                queries::INSTALL_PROJECT_LOOKUP_SQL,
                queries::INSTALL_PROJECT_LOOKUP_PARAMS,
            ),
            (
                queries::INSTALL_SCHEDULER_GUARD_SQL,
                queries::INSTALL_SCHEDULER_GUARD_PARAMS,
            ),
            (
                queries::BINDING_UNIQUE_CHECK_SQL,
                queries::BINDING_UNIQUE_CHECK_PARAMS,
            ),
            (queries::BINDING_INSERT_SQL, queries::BINDING_INSERT_PARAMS),
            (queries::BINDING_PATCH_SQL, queries::BINDING_PATCH_PARAMS),
            (
                queries::NEXT_RUN_AT_WRITEBACK_SQL,
                queries::NEXT_RUN_AT_WRITEBACK_PARAMS,
            ),
            (
                queries::BINDING_UNINSTALL_SQL,
                queries::BINDING_UNINSTALL_PARAMS,
            ),
        ];
        for (sql, params) in statements {
            let translated = translate_placeholders(sql, params);
            for (index, name) in params.iter().enumerate() {
                assert!(
                    translated.contains(&format!("${}", index + 1)),
                    "missing ${} for :{name} in {translated}",
                    index + 1
                );
            }
            // No symbolic placeholder survives (casts would show as `::`).
            let mut rest = translated.as_str();
            while let Some(at) = rest.find(':') {
                let after = rest[at + 1..].chars().next().unwrap_or(' ');
                assert!(
                    after == ':' || !after.is_ascii_alphanumeric() && after != '_',
                    "untranslated placeholder near {:?}",
                    &rest[at..rest.len().min(at + 12)]
                );
                rest = &rest[at + 1..];
            }
        }
        // The `:now`/`:now2` pair translates distinctly.
        let uninstall = translate_placeholders(
            queries::BINDING_UNINSTALL_SQL,
            queries::BINDING_UNINSTALL_PARAMS,
        );
        assert!(uninstall.contains("$1") && uninstall.contains("$2"));
        assert!(!uninstall.contains(":now"));
        // The PATCH unique-check variant gains the exclusion bind.
        let patch_check = translate_placeholders(
            &queries::binding_unique_check_sql(true),
            queries::BINDING_UNIQUE_CHECK_PATCH_PARAMS,
        );
        assert!(patch_check.contains("$3"));
    }

    // -- BR1 column layout ---------------------------------------------------

    #[test]
    fn br1_segment_layout_matches_layer_consts() {
        assert_eq!(BINDING_COLUMN_COUNT, 22);
        assert_eq!(SCHEDULER_COLUMN_COUNT, 14);
        assert_eq!(queries::AGENT_RUN_COLUMNS.len(), 41);
        assert_eq!(queries::POD_COLUMNS.len(), 10);
        assert_eq!(
            BINDING_COLUMN_COUNT
                + SCHEDULER_COLUMN_COUNT
                + queries::AGENT_RUN_COLUMNS.len()
                + queries::POD_COLUMNS.len(),
            87
        );
        // The display columns the render reads, by layer-const position.
        let run_base = BINDING_COLUMN_COUNT + SCHEDULER_COLUMN_COUNT;
        let pod_base = run_base + queries::AGENT_RUN_COLUMNS.len();
        assert_eq!(queries::AGENT_RUN_COLUMNS[0], "id");
        assert_eq!(queries::AGENT_RUN_COLUMNS[10], "status");
        assert_eq!(queries::AGENT_RUN_COLUMNS[40], "ended_at");
        assert_eq!(queries::POD_COLUMNS[0], "id");
        assert_eq!(queries::POD_COLUMNS[3], "name");
        assert_eq!(run_base, 36);
        assert_eq!(pod_base, 77);
    }

    // -- Render ---------------------------------------------------------------

    fn utc(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
    }

    #[test]
    fn render_key_order_matches_serializer_fields() {
        let id = Uuid::parse_str("68ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        let rendered = render_binding_object(
            &id,
            &id,
            &Some(("s".to_owned(), "n".to_owned(), "c".to_owned())),
            &Some(id),
            &id,
            &utc(2026, 1, 2, 3, 4, 5),
            "UTC",
            "",
            &Value::Array(vec![]),
            &Value::Array(vec![]),
            "",
            true,
            "create_issue",
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            "",
            &Some(id),
            &utc(2026, 1, 2, 3, 4, 5),
            &utc(2026, 1, 2, 3, 4, 5),
            &chrono_tz::UTC,
        );
        let parsed: Value = serde_json::from_str(&rendered).expect("valid json");
        let keys: Vec<&str> = parsed
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, shape::BINDING_SERIALIZER_FIELDS);
        // Spot values: Z datetimes, verbatim scalars, null run state.
        assert_eq!(parsed["dtstart"], "2026-01-02T03:04:05Z");
        assert_eq!(parsed["id"], "68ad4deb-fc7c-4531-b5ce-376263af21e3");
        assert_eq!(parsed["tzid"], "UTC");
        assert_eq!(parsed["enabled"], true);
        assert_eq!(parsed["last_run"], Value::Null);
        assert_eq!(parsed["pod_name"], Value::Null);
    }

    #[test]
    fn render_keeps_request_zone_offset() {
        let zone: Tz = "America/New_York".parse().unwrap();
        let rendered = render_binding_object(
            &Uuid::nil(),
            &Uuid::nil(),
            &None,
            &None,
            &Uuid::nil(),
            &utc(2026, 7, 2, 12, 0, 0),
            "UTC",
            "",
            &Value::Array(vec![]),
            &Value::Array(vec![]),
            "",
            true,
            "create_issue",
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            "",
            &None,
            &utc(2026, 7, 2, 12, 0, 0),
            &utc(2026, 7, 2, 12, 0, 0),
            &zone,
        );
        let parsed: Value = serde_json::from_str(&rendered).expect("valid json");
        // July in New York is EDT (-04:00); only +00:00 renders `Z`.
        assert_eq!(parsed["dtstart"], "2026-07-02T08:00:00-04:00");
        assert_eq!(parsed["scheduler_slug"], Value::Null);
    }

    // -- UUID parsing (`UUIDField.to_python` + the related-field guard) -----

    #[test]
    fn uuid_hex_forms_match_cpython() {
        let canonical = "68ad4deb-fc7c-4531-b5ce-376263af21e3";
        let expected = Uuid::parse_str(canonical).unwrap();
        // All `uuid.UUID(hex=...)` spellings.
        for text in [
            canonical,
            "68ad4debfc7c4531b5ce376263af21e3",
            "{68ad4deb-fc7c-4531-b5ce-376263af21e3}",
            "urn:uuid:68ad4deb-fc7c-4531-b5ce-376263af21e3",
            "68AD4DEB-FC7C-4531-B5CE-376263AF21E3",
        ] {
            assert_eq!(uuid_from_hex_forms(text), Some(expected), "parses: {text}");
        }
        // `int(hex, 16)` leniency: surrounding whitespace, one sign,
        // inter-digit underscores — then the 0..2^128 range check.
        assert_eq!(
            uuid_from_hex_forms(" 68ad4debfc7c4531b5ce376263af21e3"),
            None,
            "33 chars breaks the 32-char count"
        );
        let padded_32 = " 68ad4debfc7c4531b5ce376263af21e";
        assert_eq!(padded_32.len(), 32);
        assert!(
            uuid_from_hex_forms(padded_32).is_some(),
            "int() strips surrounding whitespace"
        );
        assert!(
            uuid_from_hex_forms("-68ad4debfc7c4531b5ce376263af21e").is_none(),
            "negative fails the range check"
        );
        assert!(
            uuid_from_hex_forms("68ad_4debfc7c4531b5ce376263af21e").is_some(),
            "inter-digit underscores parse (31 hex + `_` = 32 chars)"
        );
        assert!(
            uuid_from_hex_forms("_68ad4debfc7c4531b5ce376263af21e").is_none(),
            "leading underscore fails"
        );
        for bad in ["bogus", "5", "", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
            assert_eq!(uuid_from_hex_forms(bad), None, "rejects: {bad}");
        }
    }

    #[test]
    fn pk_parse_matches_drf_relations() {
        let canonical = "68ad4deb-fc7c-4531-b5ce-376263af21e3";
        let expected = Uuid::parse_str(canonical).unwrap();
        assert_eq!(
            parse_pk_value(&Value::String(canonical.to_owned())),
            Ok(expected)
        );
        // `UUID(int=...)` form (non-negative, in range).
        assert_eq!(
            parse_pk_value(&serde_json::json!(5)),
            Ok(Uuid::from_u128(5))
        );
        assert!(parse_pk_value(&serde_json::json!(-1)).is_err());
        // Bools fail `incorrect_type` before `to_python` runs.
        assert_eq!(
            parse_pk_value(&Value::Bool(true)),
            Err("Incorrect type. Expected pk value, received bool.".to_owned())
        );
        // Non-UUID input renders the curly-quote field message (the F36-03
        // erratum: caught per-field, never the `handle_exception` path).
        assert_eq!(
            parse_pk_value(&Value::String("bogus".to_owned())),
            Err("\u{201c}bogus\u{201d} is not a valid UUID.".to_owned())
        );
        assert_eq!(
            parse_pk_value(&serde_json::json!(5.0)),
            Err("\u{201c}5.0\u{201d} is not a valid UUID.".to_owned())
        );
    }

    #[test]
    fn guard_parse_has_no_bool_check() {
        // The install guard is ORM-level: bools take the `int=` form,
        // floats/containers fail `to_python` → 400 `VALID_DETAIL_BODY`.
        assert_eq!(
            guard_scheduler_id(Some(&Value::Bool(true))).unwrap(),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(guard_scheduler_id(None).unwrap(), None);
        assert_eq!(
            guard_scheduler_id(Some(&Value::Null)).unwrap(),
            None,
            "explicit null is pk=None → guard miss → 404"
        );
        assert!(matches!(
            guard_scheduler_id(Some(&serde_json::json!("bogus"))),
            Err(Denial::ValidDetail)
        ));
        assert!(matches!(
            guard_scheduler_id(Some(&serde_json::json!(5.0))),
            Err(Denial::ValidDetail)
        ));
    }

    // -- Python spellings -----------------------------------------------------

    #[test]
    fn py_spellings_match_str_and_repr() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&serde_json::json!(5)), "5");
        assert_eq!(py_str(&Value::String("bogus".to_owned())), "bogus");
        assert_eq!(
            py_str(&serde_json::json!({"a": 1, "b": [true, null]})),
            "{'a': 1, 'b': [True, None]}"
        );
        assert_eq!(py_repr_string("it's"), "\"it's\"");
        assert_eq!(py_repr_string("say \"hi\""), "'say \"hi\"'");
    }

    // -- Char / boolean / choice input ----------------------------------------

    #[test]
    fn char_input_matches_drf_charfield() {
        // Blank gate.
        assert_eq!(
            validate_char_input(&Value::String("".to_owned()), false, None),
            Err(vec!["This field may not be blank.".to_owned()])
        );
        assert_eq!(
            validate_char_input(&Value::String("   ".to_owned()), true, None),
            Ok(String::new())
        );
        // Numeric coercion + trim; bools/composites fail `invalid`.
        assert_eq!(
            validate_char_input(&serde_json::json!(5), true, None),
            Ok("5".to_owned())
        );
        assert_eq!(
            validate_char_input(&Value::String("  x  ".to_owned()), true, None),
            Ok("x".to_owned())
        );
        assert_eq!(
            validate_char_input(&Value::Bool(true), true, None),
            Err(vec!["Not a valid string.".to_owned()])
        );
        assert_eq!(
            validate_char_input(&serde_json::json!([]), true, None),
            Err(vec!["Not a valid string.".to_owned()])
        );
        // Validators accumulate in order on the TRIMMED value.
        let long = format!("  {}  ", "x".repeat(70));
        assert_eq!(
            validate_char_input(&Value::String(long), false, Some(64)),
            Err(vec![
                "Ensure this field has no more than 64 characters.".to_owned()
            ])
        );
        let nul = format!("{}ok\u{0}!", "y".repeat(70));
        assert_eq!(
            validate_char_input(&Value::String(nul), false, Some(64)),
            Err(vec![
                "Ensure this field has no more than 64 characters.".to_owned(),
                "Null characters are not allowed.".to_owned(),
            ])
        );
    }

    #[test]
    fn boolean_input_matches_drf_sets() {
        for (input, expected) in [
            (serde_json::json!(true), true),
            (serde_json::json!("YES"), true),
            (serde_json::json!("On"), true),
            (serde_json::json!("1"), true),
            (serde_json::json!(1), true),
            (serde_json::json!(1.0), true),
            (serde_json::json!(false), false),
            (serde_json::json!("n"), false),
            (serde_json::json!("Off"), false),
            (serde_json::json!("0"), false),
            (serde_json::json!(0), false),
            (serde_json::json!(0.0), false),
        ] {
            assert_eq!(validate_boolean_input(&input), Ok(expected), "{input}");
        }
        for input in [
            serde_json::json!(""),
            serde_json::json!("null"),
            serde_json::json!(" yes"),
            serde_json::json!(2),
            serde_json::json!(0.5),
            serde_json::json!([]),
        ] {
            assert_eq!(
                validate_boolean_input(&input),
                Err("Must be a valid boolean.".to_owned()),
                "{input}"
            );
        }
    }

    #[test]
    fn outcome_mode_matches_choicefield() {
        for mode in ["create_issue", "apply_fix", "fix_and_review"] {
            assert_eq!(
                validate_outcome_mode_input(&Value::String(mode.to_owned())),
                Ok(mode.to_owned())
            );
        }
        assert_eq!(
            validate_outcome_mode_input(&Value::String("explode".to_owned())),
            Err("\"explode\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            validate_outcome_mode_input(&Value::Bool(true)),
            Err("\"True\" is not a valid choice.".to_owned()),
            "Python str() spelling of the input"
        );
    }

    // -- DateTime input ---------------------------------------------------------

    #[test]
    fn dtstart_matches_f36_03_cases() {
        let utc_tz: Tz = chrono_tz::UTC;
        // `Z` keeps its instant.
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-01-01T00:00:00Z".to_owned()), &utc_tz),
            Ok(utc(2024, 1, 1, 0, 0, 0))
        );
        // Offsets convert to UTC.
        assert_eq!(
            validate_dtstart_input(
                &Value::String("2024-01-01T12:00:00+05:00".to_owned()),
                &utc_tz
            ),
            Ok(utc(2024, 1, 1, 7, 0, 0))
        );
        // Naive input attaches the request zone (UTC here).
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-01-01T00:00:00".to_owned()), &utc_tz),
            Ok(utc(2024, 1, 1, 0, 0, 0))
        );
        // Non-strings and garbage render `invalid`.
        let invalid = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
        for input in [
            serde_json::json!(5),
            serde_json::json!(true),
            Value::String("not-a-date".to_owned()),
            Value::String("2024-13-01T00:00:00Z".to_owned()),
        ] {
            assert_eq!(
                validate_dtstart_input(&input, &utc_tz),
                Err(invalid.to_owned()),
                "{input}"
            );
        }
        // Aware instants outside 0001..9999 overflow.
        assert_eq!(
            validate_dtstart_input(
                &Value::String("0001-01-01T00:00:00+14:00".to_owned()),
                &utc_tz
            ),
            Err("Datetime value out of range.".to_owned())
        );
    }

    #[test]
    fn dtstart_naive_attaches_request_zone() {
        let eastern: Tz = "America/New_York".parse().unwrap();
        // January in New York is EST (-05:00).
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-01-01T00:00:00".to_owned()), &eastern),
            Ok(utc(2024, 1, 1, 5, 0, 0))
        );
        // Aware inputs keep their instant regardless of the request zone.
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-01-01T00:00:00Z".to_owned()), &eastern),
            Ok(utc(2024, 1, 1, 0, 0, 0))
        );
        // Spring-forward gap takes the pre-transition offset (EST):
        // 2024-03-10 02:30 wall → 07:30Z.
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-03-10T02:30:00".to_owned()), &eastern),
            Ok(utc(2024, 3, 10, 7, 30, 0))
        );
        // Fall-back fold takes fold 0 (EDT): 2024-11-03 01:30 → 05:30Z.
        assert_eq!(
            validate_dtstart_input(&Value::String("2024-11-03T01:30:00".to_owned()), &eastern),
            Ok(utc(2024, 11, 3, 5, 30, 0))
        );
    }

    #[test]
    fn dtstart_accepts_fromiso_and_regex_spellings() {
        let utc_tz: Tz = chrono_tz::UTC;
        for (input, expected) in [
            ("2024-01-01 12:00:00", utc(2024, 1, 1, 12, 0, 0)),
            ("20240101T120000Z", utc(2024, 1, 1, 12, 0, 0)),
            ("2024-01-01", utc(2024, 1, 1, 0, 0, 0)),
            ("2024-W01-1", utc(2024, 1, 1, 0, 0, 0)),
            ("2024-1-2T3:04:05", utc(2024, 1, 2, 3, 4, 5)),
            (
                "2024-01-01T12:00:00,500000",
                Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0)
                    .unwrap()
                    .checked_add_signed(chrono::Duration::microseconds(500_000))
                    .unwrap(),
            ),
        ] {
            assert_eq!(
                validate_dtstart_input(&Value::String(input.to_owned()), &utc_tz),
                Ok(expected),
                "{input}"
            );
        }
    }

    // -- Jobs seams -------------------------------------------------------------

    #[test]
    fn rrule_closure_reports_jobs_messages() {
        assert_eq!(rrule_validator("FREQ=DAILY"), Ok(()));
        assert_eq!(rrule_validator(""), Ok(()));
        // F36-03 pins, through the seam (only the message crosses).
        assert_eq!(
            rrule_validator("FREQ=NEVER"),
            Err("invalid RRULE: invalid 'FREQ': NEVER".to_owned())
        );
        // NOTE: the F36-03 `rrule_field_level` SECONDLY pin
        // ("invalid RRULE: SECONDLY frequency is not allowed") misquotes
        // the source — `bgtasks/_rrule.py` raises its own message below
        // (verified against the Python source; the jobs port matches it).
        assert_eq!(
            rrule_validator("FREQ=SECONDLY"),
            Err(
                "FREQ=SECONDLY is not allowed (allowed: ['DAILY', 'HOURLY', 'MINUTELY', 'MONTHLY', 'WEEKLY', 'YEARLY'])"
                    .to_owned()
            )
        );
        // NOTE: same F36-03 misquote class — the true message is
        // dateutil's unpack `ValueError` wrapped in `invalid RRULE:`
        // (verified against dateutil 2.8.2; the jobs port matches it).
        assert_eq!(
            rrule_validator("garbage"),
            Err("invalid RRULE: not enough values to unpack (expected 2, got 1)".to_owned())
        );
    }

    #[test]
    fn next_fire_closure_matches_install_write_shape() {
        // Hourly cadence anchored an hour ago fires at the next top of hour.
        let now = utc(2026, 5, 4, 12, 34, 56);
        let bundle = queries::RruleBundle {
            dtstart: utc(2026, 5, 4, 11, 0, 0),
            rrule: "FREQ=HOURLY;INTERVAL=1",
            tzid: "UTC",
            rdates: &Value::Array(vec![]),
            exdates: &Value::Array(vec![]),
        };
        assert_eq!(
            next_fire_for_binding(&bundle, now),
            Some(utc(2026, 5, 4, 13, 0, 0))
        );
        // Empty tzid takes the `or "UTC"` guard (same expansion).
        let bundle = queries::RruleBundle { tzid: "", ..bundle };
        assert_eq!(
            next_fire_for_binding(&bundle, now),
            Some(utc(2026, 5, 4, 13, 0, 0))
        );
        // Single-shot past with no lists never fires.
        let bundle = queries::RruleBundle {
            dtstart: utc(2026, 5, 4, 10, 0, 0),
            rrule: "",
            tzid: "",
            ..bundle
        };
        assert_eq!(next_fire_for_binding(&bundle, now), None);
    }

    // -- Cross-field validation (no DB) ------------------------------------------

    fn install_attrs_for_cross(
        pod: Option<Option<PodChoice>>,
        project: Option<Option<Uuid>>,
    ) -> InstallAttrs {
        InstallAttrs {
            scheduler_id: Uuid::nil(),
            project_id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            actor_id: Uuid::nil(),
            attrs: BindingAttrs {
                project,
                pod,
                ..BindingAttrs::default()
            },
        }
    }

    // -- Review pins (PIDASHCONV-634 review): unpinned edges proved against
    // DRF 3.15.2 + Django 4.2.30 source. Both use a lazy pool and issue no
    // query, so they run offline.

    #[tokio::test]
    async fn install_missing_project_collects_with_other_field_errors() {
        // `{}` on install: scheduler + project + dtstart are all
        // field-level `required` (project via the uniqueness extra
        // kwargs), collected in writable-field order — not just the
        // non-project errors.
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").unwrap();
        let utc_tz: Tz = chrono_tz::UTC;
        let body = Map::new();
        let Err(Denial::BadJson(errors)) =
            validate_binding_fields(&pool, &body, None, &utc_tz).await
        else {
            panic!("empty install body must 400");
        };
        assert_eq!(
            errors,
            serde_json::json!({
                "scheduler": ["This field is required."],
                "project": ["This field is required."],
                "dtstart": ["This field is required."],
            })
        );
        // Key order follows `_writable_fields`, not the JSON object.
        let keys: Vec<&str> = errors
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["scheduler", "project", "dtstart"]);
    }

    #[tokio::test]
    async fn rewrite_passes_uuid_through_unchecked() {
        // `_rewrite_project_kwarg` returns UUIDs untouched (no existence
        // check — the gate 403s unknown ones); only non-UUID identifiers
        // hit the database. A lazy pool proves no query runs.
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").unwrap();
        let id = Uuid::parse_str("68ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        let resolved = super::super::resolve_project_id(&pool, "ws", &id.to_string())
            .await
            .expect("uuid passes through");
        assert_eq!(resolved, id);
    }

    #[test]
    fn install_cross_rejects_foreign_pod() {
        let url_project = Uuid::parse_str("68ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        let foreign = Uuid::parse_str("78ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        let install = install_attrs_for_cross(
            Some(Some(PodChoice {
                id: Uuid::nil(),
                project_id: foreign,
                name: "p".to_owned(),
            })),
            Some(Some(url_project)),
        );
        let Err(Denial::BadJson(body)) = validate_install_cross(&install, &url_project) else {
            panic!("foreign pod must 400");
        };
        assert_eq!(body, serde_json::json!({"pod": [shape::POD_PROJECT_ERROR]}));
    }
}
