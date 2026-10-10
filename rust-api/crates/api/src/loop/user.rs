#![forbid(unsafe_code)]

//! User auto-pm settings + per-job toggle handlers (D-03).
//!
//! Ports (all under `apps/api/pi_dash/`):
//!
//! * `loop/views.py:24-30` (`_master_enabled`: absent preference reads as
//!   enabled), `:33-40` (`_job_enabled_map`: live `enabled=False` rows with
//!   a job), `:42-49` (`_settings_payload`: master flag plus the enabled,
//!   non-deleted jobs in `public_name` order as whitelisted cards),
//!   `:52-61` (`_read_enabled`, via [`crate::r#loop::guards::read_enabled`]),
//!   `:64-80` (`AutoPMSettingsEndpoint` GET + PATCH), `:83-99`
//!   (`AutoPMJobEndpoint` PATCH).
//! * `loop/urls.py:9-12` (the two `users/me/auto-pm/` patterns, mounted by
//!   [`routes`]).
//!
//! Layering: the toggle-only body guard lives in
//! [`crate::r#loop::guards`], the settings reads in
//! `pidash_db::r#loop::queries`, the card shape in
//! `pidash_services::r#loop::shape`. This module owns the HTTP shell
//! (routes, session auth), the upsert SQL, and the response rendering.
//!
//! Auth is `IsAuthenticated` only — preferences are the requesting user's
//! own, so there is no workspace-role gate (guests and outsiders get 200;
//! proven by `test_user_routes_have_no_role_gate`). Identity is the
//! hash-verified DRF `SessionAuthentication` outcome
//! ([`crate::license::resolve_actor`]); anonymous callers get the exact
//! DRF 401 before anything else.
//!
//! Registration is the cutover granularity (same rule as the `app_issues`
//! and `license` families): the owned methods serve from Rust while every
//! other method on those paths proxies to Django, so DRF's
//! authenticate-before-method order (401-anon, 405-after-auth, metadata
//! OPTIONS) is preserved byte for byte. `HEAD` rides axum's `get`
//! handling on the settings path like Django's `GET`-backed `HEAD`; on
//! the job path (no GET in Django) `HEAD` proxies so Django's own 405
//! answers.
//!
//! [`routes`] merges the two owned paths; nothing else.
//!
//! Ported bugs: none in these units. (The known loop bugs both live
//! elsewhere: BUG-LOOP-1 in the admin write guard, BUG-LOOP-2 in the base
//! dispatch.)

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use crate::state::AppState;

use pidash_db::r#loop::models::loop_job::LoopJob;
use pidash_services::r#loop::shape::{public_job_payload, PublicJobRow};

/// `loop/urls.py:10` (under the `api/` include).
pub const SETTINGS_PATH: &str = "/api/users/me/auto-pm/";
/// `loop/urls.py:11` (under the `api/` include).
pub const JOB_PATH: &str = "/api/users/me/auto-pm/jobs/{slug}/";

/// Register the two owned user-surface paths. Sibling paths stay
/// unmatched and proxy to Django through the fallback.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, patch};
    Router::new()
        .route(
            SETTINGS_PATH,
            owned_settings(get(get_settings).patch(patch_settings)),
        )
        .route(JOB_PATH, owned_job(patch(patch_job)))
}

/// The settings path: GET + PATCH serve from Rust, everything else falls
/// through to Django (its 405-after-auth and metadata OPTIONS live
/// there). `HEAD` proxies explicitly: axum would auto-serve it from
/// `get`, but Django defines no `head` and 405s after auth.
fn owned_settings(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The per-job path: only PATCH exists in Django, so only PATCH is owned.
/// GET/HEAD/POST/PUT/DELETE/OPTIONS all proxy — Django answers its own
/// 405-after-auth (and metadata) for those, byte for byte.
fn owned_job(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .head(crate::edge::proxy)
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    Unauthorized,
    /// 400, DRF `ParseError` (unparseable body).
    BadDetail(String),
    /// 400, `{"error": …}` (view-inline guard bodies).
    BadError(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                crate::license::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::license::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401. Hash-verified DRF session semantics via the
/// shared license plumbing (read-only): bad session, unknown/inactive
/// user, or hash mismatch is anonymous (`BaseAPIView.authentication_classes
/// = [BaseSessionAuthentication]`, `permission_classes =
/// [IsAuthenticated]`).
async fn actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    // `resolve_actor` only ever errs with the license-layer 500, so the
    // mapping is exact; the small `Denial` error keeps
    // `clippy::result_large_err` quiet (same shape as
    // `license::require_admin`).
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// Parse a toggle body: JSON first (a parse failure is DRF's `ParseError`
/// 400), then the toggle-only guard (anything but exactly one boolean
/// `enabled` key is 400 `{"error":"invalid_payload"}`).
///
/// Python (`views.py:52-61`) folds a non-dict body into `{}` before the
/// key check; [`crate::r#loop::guards::read_enabled`] answers the same
/// `invalid_payload` for non-objects, so lists and scalars land identically.
fn parse_enabled(body: &[u8]) -> Result<bool, Denial> {
    let data: Value = match serde_json::from_slice(body) {
        Ok(data) => data,
        Err(err) => {
            return Err(Denial::BadDetail(format!("JSON parse error - {err}")));
        }
    };
    // The guard's `invalid_payload` body renders through [`Denial::BadError`]
    // (`{"error":"invalid_payload"}`, byte-identical to the guards
    // constant — pinned by `guard_bodies_match_golden_errors`).
    crate::r#loop::guards::read_enabled(&data)
        .map_err(|_| Denial::BadError("invalid_payload".to_owned()))
}

/// Render an exact `{"error": …}` body with its status. The guards module
/// owns the byte-exact constants; insertion order is preserved by the
/// crate's `serde_json/preserve_order`.
fn error_response(status: StatusCode, body: &Value) -> Response {
    let rendered = serde_json::to_string(body).expect("error body serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(rendered))
        .expect("error response")
}

// ---------------------------------------------------------------------------
// reads
// ---------------------------------------------------------------------------

/// `views.py:42-49`: the full settings payload — master flag plus the
/// enabled, non-deleted jobs in `public_name` order as whitelisted cards
/// (`name` is `public_name`; `prompt`, `min_role`, and the admin `name`
/// never leave the server). Pure over already-fetched rows so the golden
/// replay tests pin it without a database.
fn settings_body(
    master_enabled: bool,
    off_job_ids: &HashSet<uuid::Uuid>,
    jobs: &[LoopJob],
) -> Value {
    let cards: Vec<Value> = jobs
        .iter()
        .map(|job| {
            public_job_payload(
                &PublicJobRow {
                    slug: job.slug.as_str(),
                    public_name: job.public_name.as_str(),
                    public_description: job.public_description.as_str(),
                    rrule: job.rrule.as_str(),
                },
                !off_job_ids.contains(&job.id),
            )
        })
        .collect();
    serde_json::json!({"enabled": master_enabled, "jobs": cards})
}

/// The three settings reads (`views.py:24-49`): master flag (absent row =
/// enabled), off-job-id set, enabled jobs in display order.
async fn read_settings(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> Result<Value, Denial> {
    let master = pidash_db::r#loop::queries::fetch_master_enabled(pool, user_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let off: HashSet<uuid::Uuid> = pidash_db::r#loop::queries::fetch_off_job_ids(pool, user_id)
        .await
        .map_err(|_| Denial::ServerError)?
        .into_iter()
        .collect();
    let jobs = pidash_db::r#loop::queries::fetch_enabled_jobs(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(settings_body(master, &off, &jobs))
}

// ---------------------------------------------------------------------------
// upsert (`QuerySet.update_or_create`, `views.py:71-76,93-98`)
// ---------------------------------------------------------------------------

/// Live preference-row id for one (user, job-scope): `job_id IS NULL` is
/// the master switch, `job_id = $2` the per-job row. The doubled
/// `deleted_at IS NULL` mirrors the manager filter plus the explicit
/// `deleted_at__isnull=True` lookup, the same shape the queries layer
/// emits.
async fn live_preference_id(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    job_id: Option<uuid::Uuid>,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let row: Option<(uuid::Uuid,)> = match job_id {
        None => {
            sqlx::query_as(
                r#"SELECT id FROM loop_user_preferences
                   WHERE user_id = $1 AND job_id IS NULL
                     AND deleted_at IS NULL AND deleted_at IS NULL
                   LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(pool)
            .await?
        }
        Some(job) => {
            sqlx::query_as(
                r#"SELECT id FROM loop_user_preferences
                   WHERE user_id = $1 AND job_id = $2
                     AND deleted_at IS NULL AND deleted_at IS NULL
                   LIMIT 1"#,
            )
            .bind(user_id)
            .bind(job)
            .fetch_optional(pool)
            .await?
        }
    };
    Ok(row.map(|row| row.0))
}

/// `update_or_create(user, job, deleted_at__isnull=True,
/// defaults={"enabled"})`: a live row is updated in place (which flips a
/// re-toggle instead of duplicating — the partial unique constraints
/// would reject a second live row anyway); otherwise one row is inserted.
///
/// Audit stamping mirrors `BaseModel.save` via crum: on create
/// `created_by` is the request user and `updated_by` stays `None`; on
/// update `updated_by` becomes the request user and `created_by` is
/// untouched. `updated_at` is refreshed on update (`auto_now`); every
/// other column keeps its stored value, so the `UPDATE` names only the
/// three columns `save()` observably changes.
async fn upsert_preference(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    job_id: Option<uuid::Uuid>,
    enabled: bool,
) -> Result<(), Denial> {
    match live_preference_id(pool, user_id, job_id)
        .await
        .map_err(|_| Denial::ServerError)?
    {
        Some(id) => {
            let now = chrono::Utc::now();
            sqlx::query(
                r#"UPDATE loop_user_preferences
                   SET enabled = $1, updated_at = $2, updated_by_id = $3
                   WHERE id = $4"#,
            )
            .bind(enabled)
            .bind(now)
            .bind(user_id)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            Ok(())
        }
        None => {
            let now = chrono::Utc::now();
            sqlx::query(
                r#"INSERT INTO loop_user_preferences
                   (created_at, updated_at, created_by_id, updated_by_id,
                    deleted_at, id, user_id, job_id, enabled)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            )
            .bind(now)
            .bind(now)
            .bind(user_id)
            .bind(None::<uuid::Uuid>)
            .bind(None::<chrono::DateTime<chrono::Utc>>)
            .bind(uuid::Uuid::new_v4())
            .bind(user_id)
            .bind(job_id)
            .bind(enabled)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET users/me/auto-pm/` (`views.py:67-68`): the full settings payload.
async fn get_settings(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    match read_settings(&pool, actor.id).await {
        Ok(body) => crate::license::json_response(&body),
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH users/me/auto-pm/` (`views.py:70-78`): master upsert
/// (`job=None`) then the full settings payload.
async fn patch_settings(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let enabled = match parse_enabled(&body) {
        Ok(enabled) => enabled,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = upsert_preference(&pool, actor.id, None, enabled).await {
        return denial.into_response();
    }
    match read_settings(&pool, actor.id).await {
        Ok(payload) => crate::license::json_response(&payload),
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH users/me/auto-pm/jobs/<slug>/` (`views.py:86-99`): an unknown
/// or admin-disabled slug is 404 `{"error":"not_found"}` — the lookup
/// runs *before* the body guard, so a bad body on an unknown slug still
/// answers 404. Otherwise the per-job upsert lands, then the full
/// settings payload.
async fn patch_job(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // `filter(slug, enabled=True, deleted_at__isnull=True).first()`:
    // at most one live row per slug (partial unique), so `LIMIT 1` without
    // `ORDER BY` is the same row `.first()` (default `-created_at`) finds.
    // A database error is the generic 500; an empty result is the 404 below.
    let job: Option<(uuid::Uuid,)> = match sqlx::query_as(
        r#"SELECT id FROM loop_jobs
           WHERE slug = $1 AND enabled = TRUE
             AND deleted_at IS NULL AND deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(slug.as_str())
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let job_id = match job {
        Some((id,)) => id,
        None => {
            let body: Value = serde_json::from_str(crate::r#loop::guards::LOOP_NOT_FOUND_BODY)
                .expect("not_found const parses");
            return error_response(StatusCode::NOT_FOUND, &body);
        }
    };
    let enabled = match parse_enabled(&body) {
        Ok(enabled) => enabled,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = upsert_preference(&pool, actor.id, Some(job_id), enabled).await {
        return denial.into_response();
    }
    match read_settings(&pool, actor.id).await {
        Ok(payload) => crate::license::json_response(&payload),
        Err(denial) => denial.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/handlers")
    }

    fn golden() -> Value {
        let path = fixtures_dir().join("user_settings.golden.json");
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Field-for-field equality plus byte-identical replay in canonical
    /// (sorted-keys) form — the same hermetic comparison the services
    /// shape tests use, so the check holds with or without
    /// `serde_json/preserve_order` unification.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> =
                    map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(entries.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden output"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch (canonical sorted-keys form)"
        );
    }

    fn fx_job() -> LoopJob {
        // The recorded fixture row behind every FX-LOOP-06 user vector:
        // slug `fxloop-job`, public name `Pub name`, description `desc`,
        // `FREQ=DAILY…` rrule (label `daily`).
        let now =
            chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0).expect("epoch valid");
        LoopJob {
            id: uuid::Uuid::nil(),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            slug: "fxloop-job".to_owned(),
            name: "Admin name".to_owned(),
            public_name: "Pub name".to_owned(),
            public_description: "desc".to_owned(),
            prompt: "TOPSECRET PROMPT".to_owned(),
            min_role: 15,
            enabled: true,
            is_builtin: false,
            dtstart: now,
            rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0".to_owned(),
            tzid: "UTC".to_owned(),
        }
    }

    #[test]
    fn settings_get_replays_golden() {
        // `settings_get`: master on (no preference row), one enabled job,
        // no opt-outs.
        let job = fx_job();
        let produced = settings_body(true, &HashSet::new(), std::slice::from_ref(&job));
        assert_replay(
            &produced,
            golden()
                .get("settings_get")
                .and_then(|v| v.get("body"))
                .expect("body"),
        );
    }

    #[test]
    fn settings_patch_off_replays_golden() {
        // `settings_patch_off`: the master upsert landed (`enabled:
        // false`), the job card keeps its own effective state.
        let job = fx_job();
        let produced = settings_body(false, &HashSet::new(), std::slice::from_ref(&job));
        assert_replay(
            &produced,
            golden()
                .get("settings_patch_off")
                .and_then(|v| v.get("body"))
                .expect("body"),
        );
    }

    #[test]
    fn job_patch_off_replays_golden() {
        // `job_patch_off`: master on, the toggled job's card off — the
        // off-job-id set carries exactly the fixture job's id.
        let job = fx_job();
        let off: HashSet<uuid::Uuid> = [job.id].into_iter().collect();
        let produced = settings_body(true, &off, std::slice::from_ref(&job));
        assert_replay(
            &produced,
            golden()
                .get("job_patch_off")
                .and_then(|v| v.get("body"))
                .expect("body"),
        );
    }

    #[test]
    fn payload_never_leaks_admin_fields() {
        // Whitelist proof against the recorded GET body: no `prompt`,
        // `min_role`, admin `name`, `public_*`, or `rrule` key anywhere on
        // the wire, and exactly the `enabled` + `jobs` top-level keys.
        let job = fx_job();
        let produced = settings_body(true, &HashSet::new(), std::slice::from_ref(&job));
        let text = serde_json::to_string(&produced).expect("serializes");
        assert!(!text.contains("TOPSECRET"), "prompt leaked");
        assert!(!text.contains("min_role"), "min_role leaked");
        let top: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(top, vec!["enabled", "jobs"], "top-level keys in order");
        let card = produced
            .get("jobs")
            .and_then(Value::as_array)
            .and_then(|jobs| jobs.first())
            .expect("one card");
        let mut keys: Vec<&str> = card
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["description", "enabled", "interval_label", "name", "slug"],
            "exactly the 5 whitelisted card keys"
        );
    }

    #[test]
    fn guard_bodies_match_golden_errors() {
        // The 404 and 400 error bytes the handlers render are the guards
        // constants, pinned against the recorded vectors.
        let gold = golden();
        let not_found: Value =
            serde_json::from_str(crate::r#loop::guards::LOOP_NOT_FOUND_BODY).expect("const parses");
        assert_replay(
            &not_found,
            gold.get("job_patch_404")
                .and_then(|v| v.get("body"))
                .expect("body"),
        );
        let invalid: Value = serde_json::from_str(crate::r#loop::guards::INVALID_PAYLOAD_BODY)
            .expect("const parses");
        for vector in [
            "settings_patch_bad_type",
            "settings_patch_extra_key",
            "job_patch_bad_payload",
        ] {
            assert_replay(
                &invalid,
                gold.get(vector).and_then(|v| v.get("body")).expect("body"),
            );
        }
    }

    #[test]
    fn toggle_guard_vectors_hold() {
        // Every recorded invalid toggle body is rejected; both recorded
        // `enabled` values pass through.
        for raw in [
            r#"{"enabled":"yes"}"#,
            r#"{"enabled":1}"#,
            r#"{"enabled":null}"#,
            r#"{"foo":true}"#,
            r#"{"enabled":true,"extra":false}"#,
            r#"{}"#,
            r#"[true]"#,
        ] {
            let body: Value = serde_json::from_str(raw).expect("vector parses");
            assert!(
                crate::r#loop::guards::read_enabled(&body).is_err(),
                "must reject {raw}"
            );
        }
        for (raw, expected) in [
            (r#"{"enabled":true}"#, true),
            (r#"{"enabled":false}"#, false),
        ] {
            let body: Value = serde_json::from_str(raw).expect("vector parses");
            assert_eq!(crate::r#loop::guards::read_enabled(&body), Ok(expected));
        }
    }

    #[test]
    fn settings_response_is_compact_wire_ordered() {
        // The settings envelope renders compact JSON with `enabled`
        // before `jobs` (Python dict insertion order), the byte form the
        // handlers send.
        let job = fx_job();
        let body = settings_body(true, &HashSet::new(), std::slice::from_ref(&job));
        let wire = serde_json::to_string(&body).expect("serializes");
        assert_eq!(
            wire,
            r#"{"enabled":true,"jobs":[{"slug":"fxloop-job","name":"Pub name","description":"desc","interval_label":"daily","enabled":true}]}"#,
            "exact wire bytes"
        );
    }
}
