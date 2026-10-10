//! Project scheduler occurrences endpoint (D-36, stage 5, PIDASHCONV-635).
//!
//! Ports `ProjectSchedulerOccurrencesEndpoint.get`
//! (`apps/api/pi_dash/app/views/scheduler/occurrences.py:63-208`), route
//! `apps/api/pi_dash/app/urls/scheduler.py:41-45`
//! (`workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/?from=&to=`).
//!
//! Wiring only, no new logic: the project-slug rewrite
//! ([`resolve_project_id`], a per-handler-file helper), gates from
//! [`super::gate`] (PIDASHCONV-632), window/expansion/merge from
//! `pidash_services::app_scheduler::occurrences` (PIDASHCONV-631).
//! Fixture: `rust-api/fixtures/app_scheduler/handlers/occurrences_io.golden.json`
//! (F36-12; trace: `rust-api/fixtures/app_scheduler/TRACE.md`).
//!
//! Request order (Django's order, preserved — F36-09 `check_order`):
//! session auth (anonymous callers 401 inside the gate), then the
//! project-slug rewrite for authenticated callers (404
//! [`PROJECT_SLUG_NOT_FOUND_BODY`]), then the decorator gate
//! ([`gate::resolve_gate`] with [`Gate::ProjectOpen`]: 403), then the
//! handler body: project existence check (404 [`PROJECT_NOT_FOUND_BODY`]),
//! window validation (the exact 400s), future-bindings read + RRULE
//! expansion, past-runs read, string sort, 200 envelope. Unknown slugs
//! 403 inside the gate, never 404. There is NO `SCHEDULER_ENABLED` check
//! on this route (ported quirk — the 4 CRUD routes 404 when disabled
//! while this endpoint keeps serving 200).
//!
//! The `expand_binding` closure closes over the merged jobs engine
//! (`pidash_jobs::tasks_ticker::fire_binding::coerce_iso_datetimes` +
//! `tasks_ticker::rrule::occurrences_between`, PIDASHCONV-205): this crate
//! already depends on both `services` and `jobs`, so the handler is the
//! seam — a services→jobs call would be a crate cycle (split-review fix).
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * NO feature-flag check (`occurrences.py:63-78` never calls
//!   `_feature_enabled`).
//! * Valid-UUID `project_id`s pass the rewrite unchecked — unknown UUIDs
//!   403 at the gate (F36-09 `check_order`, F36-12 `project_404` note);
//!   only non-UUID identifiers resolve via `Project.resolve`.
//! * The past slice is skipped entirely when `from >= past_end`
//!   (`occurrences.py:166`); past-driven cap overflow is silent (no
//!   `has_more`); the merge sorts by the `dtstart` STRING
//!   (`occurrences.py:196`) — all owned by the services layer.
//!
//! Documented approximations (no contract input covers them):
//!
//! * The authenticated-only rewrite runs behind the `app_pages` session
//!   peek: a stale session (deleted/inactive user, wrong backend, bad
//!   hash) with an unresolvable slug answers the rewrite 404 where Django
//!   401s without rewriting (the full auth runs after). Same edge the
//!   merged `app_pages` precedent accepts.
//! * A non-array `rdates`/`exdates` JSON value coerces to empty (the merged
//!   jobs `coerce_iso_datetimes`); Python raises `TypeError` (500) only for
//!   truthy non-iterables (numbers/bools) — model-violating data either way.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use pidash_jobs::tasks_ticker::fire_binding::coerce_iso_datetimes;
use pidash_jobs::tasks_ticker::rrule::occurrences_between;
use pidash_services::app_scheduler::occurrences::{self, FutureBindingRow, Occurrence, PastRunRow};

use super::gate::{self, Gate};
use crate::app_issues::{query_last, QueryMap};
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// `Project.resolve` miss (404, `db/models/project.py:213-219`):
/// unresolvable non-UUID identifiers raise `Http404("Project not found")`,
/// rendered through DRF `exception_handler`. Byte-pinned against F36-09 in
/// [`tests::rewrite_404_body_matches_f36_09`]. Generic by design — the
/// slug is never echoed back to clients.
pub const PROJECT_SLUG_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `get_object_or_404(Project, ...)` miss (404, `occurrences.py:78`).
/// Byte-pinned against F36-12 in [`tests::project_404_body_matches_f36_12`].
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"No Project matches the given query."}"#;

// ---------------------------------------------------------------------------
// Handler-level denial (gate denials render through `gate::Denial`)
// ---------------------------------------------------------------------------

/// What the occurrences handler answers without running the happy path.
/// Window rejections render inline from [`WindowError`], not through this.
pub enum Denial {
    /// 404, `Project.resolve` miss.
    ProjectNotFound,
    /// 404, `get_object_or_404(Project, ...)` miss.
    ObjectNotFound,
    /// Database failure: 500 [`gate::SERVER_ERROR_BODY`].
    ServerError,
}

fn json_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("occurrences-handler response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::ProjectNotFound => {
                json_response(StatusCode::NOT_FOUND, PROJECT_SLUG_NOT_FOUND_BODY)
            }
            Denial::ObjectNotFound => json_response(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, gate::SERVER_ERROR_BODY)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Decode rows (services structs gain no sqlx impls; the handler decodes)
// ---------------------------------------------------------------------------

/// Decode row for [`occurrences::future_bindings_sql`]: field names match
/// the SELECT aliases. Every column is NOT NULL per the model
/// (`db/models/scheduler.py`: `dtstart`/`tzid`/`rrule`/`rdates`/`exdates`
/// carry no `null=True`; empty strings fall back in the services layer —
/// Python `or`), so strict types are exact.
#[derive(Debug, Clone, sqlx::FromRow)]
struct FutureBindingRecord {
    id: uuid::Uuid,
    dtstart: DateTime<Utc>,
    tzid: String,
    rrule: String,
    rdates: Value,
    exdates: Value,
    scheduler_id: uuid::Uuid,
    scheduler_name: String,
    scheduler_color: String,
}

impl FutureBindingRecord {
    fn into_row(self) -> FutureBindingRow {
        FutureBindingRow {
            id: self.id,
            dtstart: self.dtstart,
            tzid: self.tzid,
            rrule: self.rrule,
            rdates: self.rdates,
            exdates: self.exdates,
            scheduler_id: self.scheduler_id,
            scheduler_name: self.scheduler_name,
            scheduler_color: self.scheduler_color,
        }
    }
}

/// Decode row for [`occurrences::past_runs_sql`]: field names match the
/// SELECT aliases. `status` is NOT NULL (`runner/models.py`, default
/// QUEUED); `started_at` is nullable but the range predicates exclude
/// NULLs (a decode failure is the unreachable 500, as in the services
/// layer); the filter excludes NULL bindings.
#[derive(Debug, Clone, sqlx::FromRow)]
struct PastRunRecord {
    run_id: uuid::Uuid,
    binding_id: uuid::Uuid,
    started_at: DateTime<Utc>,
    status: String,
    binding_tzid: String,
    scheduler_id: uuid::Uuid,
    scheduler_name: String,
    scheduler_color: String,
}

impl PastRunRecord {
    fn into_row(self) -> PastRunRow {
        PastRunRow {
            run_id: self.run_id,
            binding_id: self.binding_id,
            started_at: self.started_at,
            status: self.status,
            binding_tzid: self.binding_tzid,
            scheduler_id: self.scheduler_id,
            scheduler_name: self.scheduler_name,
            scheduler_color: self.scheduler_color,
        }
    }
}

/// The 200 envelope (`occurrences.py:201-207`), keys in Python dict order.
/// `next_window_start` serializes null when `None` (no `skip_serializing`);
/// rows render through the services [`Occurrence`] shape.
#[derive(Debug, Clone, Serialize)]
struct OccurrencesEnvelope {
    occurrences: Vec<Occurrence>,
    has_more: bool,
    next_window_start: Option<String>,
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

/// `request.user` from the Django session (the `app_pages` peek: no
/// session, no key, or a non-UUID id means anonymous). Decides only
/// whether the rewrite runs — anonymous callers fall through to
/// [`gate::resolve_gate`], which renders the 401.
fn actor_user_id(extension: Option<Extension<SessionHandle>>) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().map(str::to_owned))
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
}

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

/// `_rewrite_project_kwarg` + `Project.resolve` (`app/views/base.py:48-81`,
/// `db/models/project.py:192-219`): UUIDs pass through UNCHECKED (unknown
/// UUIDs 403 at the gate — F36-09 `check_order`); other identifiers match
/// `UPPER(identifier)` after trimming, scoped to this workspace's live
/// rows; misses raise `Http404("Project not found")`. Runs for
/// authenticated requests only — anonymous callers 401 inside the gate.
///
/// NOTE: this differs deliberately from the `app_pages` precedent, which
/// existence-checks UUIDs: F36-09 pins pass-through for these routes.
async fn resolve_project_id(pool: &PgPool, slug: &str, raw: &str) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// `get_object_or_404(Project, pk=project_id, workspace__slug=slug)`
/// (`occurrences.py:78`): the default manager excludes soft-deleted rows.
async fn project_exists(
    pool: &PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// The injected `expand_binding` closure body: coerce the raw JSON
/// `rdates`/`exdates` via the merged fire path, then expand via the merged
/// RRULE engine. Signature matches
/// [`occurrences::collect_future_occurrences`], which calls it once per
/// binding with the shrinking remaining cap.
#[allow(clippy::too_many_arguments)]
fn expand_binding(
    dtstart: DateTime<Utc>,
    rrule: &str,
    tzid: &str,
    rdates: &Value,
    exdates: &Value,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    cap: usize,
) -> (Vec<DateTime<Utc>>, bool) {
    let rdates = coerce_iso_datetimes(rdates);
    let exdates = coerce_iso_datetimes(exdates);
    occurrences_between(
        dtstart,
        rrule,
        tzid,
        &rdates,
        &exdates,
        window_start,
        window_end,
        cap,
    )
}

/// `GET /api/workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/`
/// (`occurrences.py:75-208`): the merged past + future calendar slice over
/// the `from`/`to` window (defaulting independently to ±30 days around
/// `now`), 200. Readable by any project role; anonymous 401s, denied
/// members 403, unknown identifiers 404 — all before the window or any
/// occurrence query runs.
pub async fn occurrences_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // The rewrite runs for authenticated requests only (the peek mirrors
    // `app_pages`: anonymous callers 401 inside the gate): the project
    // value is unused on the anonymous path, so a raw fallback stands in.
    let project_id = if actor_user_id(extension.clone()).is_some() {
        match resolve_project_id(pool, &slug, &project_raw).await {
            Ok(id) => id,
            Err(denial) => return denial.into_response(),
        }
    } else {
        project_raw
            .parse::<uuid::Uuid>()
            .unwrap_or(uuid::Uuid::nil())
    };
    if let Err(denial) = gate::resolve_gate(
        &state,
        &Gate::ProjectOpen,
        &slug,
        Some(&project_id),
        extension,
    )
    .await
    {
        return denial.into_response();
    }
    // NO `ensure_feature_enabled` call (ported quirk — see module docs).
    match project_exists(pool, &slug, &project_id).await {
        Ok(true) => {}
        Ok(false) => return Denial::ObjectNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    }
    // `now` is captured ONCE (`timezone.now()`, `:80`) and reused for the
    // window defaults and both slice bounds.
    let now = Utc::now();
    let from_raw = query_last(&query, "from");
    let to_raw = query_last(&query, "to");
    let (window_start, window_end) =
        match occurrences::resolve_window(from_raw.as_deref(), to_raw.as_deref(), now) {
            Ok(window) => window,
            Err(error) => {
                return (StatusCode::BAD_REQUEST, Json(error.body())).into_response();
            }
        };
    let bindings: Vec<FutureBindingRecord> =
        match sqlx::query_as(occurrences::future_bindings_sql())
            .bind(&slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return Denial::ServerError.into_response(),
        };
    let bindings: Vec<FutureBindingRow> = bindings
        .into_iter()
        .map(FutureBindingRecord::into_row)
        .collect();
    let future_start = occurrences::future_start(window_start, now);
    let (mut merged, truncated_at) = occurrences::collect_future_occurrences(
        &bindings,
        future_start,
        window_end,
        expand_binding,
    );
    let end = occurrences::past_end(window_end, now);
    if occurrences::past_query_needed(window_start, end) {
        let runs: Vec<PastRunRecord> = match sqlx::query_as(occurrences::past_runs_sql())
            .bind(&slug)
            .bind(project_id)
            .bind(window_start)
            .bind(end)
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let runs: Vec<PastRunRow> = runs.into_iter().map(PastRunRecord::into_row).collect();
        occurrences::collect_past_occurrences(&runs, &mut merged);
    }
    occurrences::sort_occurrences(&mut merged);
    let body = OccurrencesEnvelope {
        occurrences: merged,
        has_more: occurrences::has_more(truncated_at),
        next_window_start: occurrences::next_window_start(truncated_at),
    };
    (StatusCode::OK, Json(body)).into_response()
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An occurrences path: the owned GET serves from Rust, everything else
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

/// Occurrences route (`app/urls/scheduler.py:41-45`). Sibling D-36
/// handler files expose their own `routes()`; the module `routes()`
/// merges them (merges keep both sides).
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/occurrences/",
        owned(
            get(occurrences_list),
            &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pidash_services::app_scheduler::occurrences::WindowError;
    use serde_json::json;

    const FIXTURE_OCCURRENCES_IO: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/handlers/occurrences_io.golden.json"
    );
    const FIXTURE_GUARDS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/guards/permissions.golden.json"
    );

    fn fixture_json(path: &str) -> Value {
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn action() -> Value {
        fixture_json(FIXTURE_OCCURRENCES_IO)["actions"][0].clone()
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    // -- F36-12 bodies ----------------------------------------------------

    /// The project-check 404, byte for byte (`occurrences.py:78`).
    #[test]
    fn project_404_body_matches_f36_12() {
        let miss = &action()["project_404"];
        assert_eq!(miss["status"], 404);
        let expected = serde_json::to_string(&miss["body"]).expect("body JSON");
        assert_eq!(PROJECT_NOT_FOUND_BODY, expected);
    }

    /// The rewrite-miss 404 is byte-identical to the F36-09 pin (the same
    /// `Project.resolve` miss the guards fixture records).
    #[test]
    fn rewrite_404_body_matches_f36_09() {
        let guards = fixture_json(FIXTURE_GUARDS);
        let expected =
            serde_json::to_string(&guards["bodies"]["404_project_slug"]).expect("body JSON");
        assert_eq!(PROJECT_SLUG_NOT_FOUND_BODY, expected);
    }

    /// Both window 400s render byte-identically to F36-12.
    #[test]
    fn window_400_bodies_match_f36_12() {
        let errors = &action()["window_400s"];
        for (error, key) in [
            (WindowError::InvalidWindow, "invalid_window"),
            (WindowError::WindowTooLarge, "window_too_large"),
        ] {
            let pin = &errors[key];
            assert_eq!(pin["status"], 400);
            assert_eq!(error.status_code(), 400);
            let expected = serde_json::to_string(&pin["body"]).expect("body JSON");
            assert_eq!(
                serde_json::to_string(&error.body()).expect("render"),
                expected
            );
        }
    }

    /// The empty-project 200 renders exactly (key order + nulls).
    #[test]
    fn empty_envelope_matches_f36_12() {
        let valid = &action()["valid"];
        assert_eq!(valid["status"], 200);
        let body = OccurrencesEnvelope {
            occurrences: Vec::new(),
            has_more: false,
            next_window_start: None,
        };
        let expected = serde_json::to_string(&valid["body"]).expect("body JSON");
        assert_eq!(serde_json::to_string(&body).expect("render"), expected);
    }

    /// A non-empty envelope renders the 9-key rows in Python dict order
    /// with `+00:00` instants, empty-string fallbacks, and explicit nulls.
    #[test]
    fn nonempty_envelope_row_bytes() {
        let binding = FutureBindingRow {
            id: "11111111-1111-1111-1111-111111111111".parse().unwrap(),
            dtstart: utc(2024, 5, 1, 7, 0, 0),
            tzid: String::new(),
            rrule: String::new(),
            rdates: Value::Array(Vec::new()),
            exdates: Value::Array(Vec::new()),
            scheduler_id: "22222222-2222-2222-2222-222222222222".parse().unwrap(),
            scheduler_name: "Nightly".to_owned(),
            scheduler_color: String::new(),
        };
        let occ = Occurrence::scheduled(&binding, utc(2024, 5, 2, 7, 0, 0));
        let body = OccurrencesEnvelope {
            occurrences: vec![occ],
            has_more: true,
            next_window_start: Some("2024-05-02T07:00:00+00:00".to_owned()),
        };
        assert_eq!(
            serde_json::to_string(&body).expect("render"),
            "{\"occurrences\":[{\"binding_id\":\"11111111-1111-1111-1111-111111111111\",\
            \"scheduler_id\":\"22222222-2222-2222-2222-222222222222\",\
            \"scheduler_name\":\"Nightly\",\"scheduler_color\":\"#3b82f6\",\
            \"dtstart\":\"2024-05-02T07:00:00+00:00\",\"tzid\":\"UTC\",\
            \"kind\":\"scheduled\",\"agent_run_id\":null,\"status\":null}],\
            \"has_more\":true,\"next_window_start\":\"2024-05-02T07:00:00+00:00\"}"
        );
    }

    /// The occurrences row carries NO flag check (ported quirk): the gate
    /// table is the contract, and this handler never calls
    /// `ensure_feature_enabled`.
    #[test]
    fn occurrences_row_has_no_flag_check() {
        let row = gate::gate_for(
            "GET",
            "workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/",
        )
        .expect("occurrences gate row");
        assert_eq!(row.gate, Gate::ProjectOpen);
        assert!(!row.flag_check);
    }

    // -- expand_binding closure -------------------------------------------

    /// Single-shot expansion through the jobs-backed closure: the dtstart
    /// lands in-window, a garbage rdate item is skipped, and an exdate
    /// covering the dtstart empties the output.
    #[test]
    fn expand_binding_single_shot_and_coercion() {
        let dtstart = utc(2024, 5, 1, 7, 0, 0);
        let start = utc(2024, 5, 1, 0, 0, 0);
        let end = utc(2024, 5, 3, 0, 0, 0);
        let rdates = json!(["2024-05-02T07:00:00Z", "junk", 5]);
        let (out, hit) = expand_binding(
            dtstart,
            "",
            "UTC",
            &rdates,
            &Value::Array(Vec::new()),
            start,
            end,
            5000,
        );
        assert!(!hit);
        assert_eq!(out, vec![dtstart, utc(2024, 5, 2, 7, 0, 0)]);
        let exdates = json!(["2024-05-01T07:00:00Z"]);
        let (out, hit) = expand_binding(
            dtstart,
            "",
            "UTC",
            &Value::Array(Vec::new()),
            &exdates,
            start,
            end,
            5000,
        );
        assert!(!hit);
        assert!(out.is_empty());
    }

    /// RRULE expansion through the closure honors the window and reports
    /// the cap exactly like the engine contract the services layer pins.
    #[test]
    fn expand_binding_rrule_window_and_cap() {
        let dtstart = utc(2024, 5, 1, 7, 0, 0);
        let start = utc(2024, 5, 1, 7, 0, 0);
        let end = utc(2024, 5, 1, 10, 0, 0);
        let empty = Value::Array(Vec::new());
        let (out, hit) = expand_binding(
            dtstart,
            "FREQ=HOURLY",
            "UTC",
            &empty,
            &empty,
            start,
            end,
            5000,
        );
        assert!(!hit);
        assert_eq!(
            out,
            vec![
                utc(2024, 5, 1, 7, 0, 0),
                utc(2024, 5, 1, 8, 0, 0),
                utc(2024, 5, 1, 9, 0, 0),
                utc(2024, 5, 1, 10, 0, 0),
            ]
        );
        let (out, hit) =
            expand_binding(dtstart, "FREQ=HOURLY", "UTC", &empty, &empty, start, end, 2);
        assert!(hit);
        assert_eq!(
            out,
            vec![utc(2024, 5, 1, 7, 0, 0), utc(2024, 5, 1, 8, 0, 0)]
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
